use std::mem::size_of;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Duration;

use windows::core::PCWSTR;
use windows::Win32::Foundation::FILETIME;
use windows::Win32::System::Performance::{
    PdhAddEnglishCounterW, PdhCloseQuery, PdhCollectQueryData, PdhGetFormattedCounterArrayW,
    PdhOpenQueryW, PDH_CSTATUS_NEW_DATA, PDH_CSTATUS_VALID_DATA, PDH_FMT_COUNTERVALUE_ITEM_W,
    PDH_FMT_DOUBLE, PDH_MORE_DATA,
};
use windows::Win32::System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS};
use windows::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
use windows::Win32::System::Threading::GetSystemTimes;

use crate::log;

#[derive(Clone, Copy, Debug, Default)]
pub struct StatsSnapshot {
    pub cpu: f32,
    pub gpu: Option<f32>,
    pub ram: f32,
    pub battery: Option<f32>,
    pub charging: bool,
}

pub struct SystemMonitor {
    latest: Arc<Mutex<StatsSnapshot>>,
    stop: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
}

impl SystemMonitor {
    pub fn new() -> Self {
        let latest = Arc::new(Mutex::new(StatsSnapshot::default()));
        let stop = Arc::new(AtomicBool::new(false));
        let thread_latest = latest.clone();
        let thread_stop = stop.clone();
        let handle = std::thread::spawn(move || {
            let mut previous: Option<(u64, u64)> = None;
            let gpu = GpuCounter::new();
            while !thread_stop.load(Ordering::Relaxed) {
                let cpu = sample_cpu(&mut previous);
                let ram = sample_ram();
                let (battery, charging) = sample_battery();
                let gpu_value = gpu.as_ref().and_then(|counter| counter.sample());
                if let Ok(mut snapshot) = thread_latest.lock() {
                    *snapshot = StatsSnapshot {
                        cpu,
                        gpu: gpu_value,
                        ram,
                        battery,
                        charging,
                    };
                }
                for _ in 0..10 {
                    if thread_stop.load(Ordering::Relaxed) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(100));
                }
            }
        });
        Self {
            latest,
            stop,
            handle: Some(handle),
        }
    }

    pub fn snapshot(&self) -> StatsSnapshot {
        self.latest
            .lock()
            .map(|snapshot| *snapshot)
            .unwrap_or_default()
    }
}

impl Drop for SystemMonitor {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn sample_cpu(previous: &mut Option<(u64, u64)>) -> f32 {
    let mut idle = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result = unsafe { GetSystemTimes(Some(&mut idle), Some(&mut kernel), Some(&mut user)) };
    if result.is_err() {
        return 0.0;
    }
    let idle = filetime(idle);
    let total = filetime(kernel) + filetime(user);
    let Some((prev_idle, prev_total)) = *previous else {
        *previous = Some((idle, total));
        return 0.0;
    };
    *previous = Some((idle, total));
    let idle_delta = idle.saturating_sub(prev_idle);
    let total_delta = total.saturating_sub(prev_total);
    if total_delta == 0 {
        return 0.0;
    }
    ((1.0 - idle_delta as f32 / total_delta as f32).clamp(0.0, 1.0) * 100.0).clamp(0.0, 100.0)
}

fn sample_ram() -> f32 {
    let mut memory = MEMORYSTATUSEX {
        dwLength: size_of::<MEMORYSTATUSEX>() as u32,
        ..Default::default()
    };
    if unsafe { GlobalMemoryStatusEx(&mut memory) }.is_ok() {
        memory.dwMemoryLoad as f32
    } else {
        0.0
    }
}

fn sample_battery() -> (Option<f32>, bool) {
    let mut status = SYSTEM_POWER_STATUS::default();
    if unsafe { GetSystemPowerStatus(&mut status) }.is_err() {
        return (None, false);
    }
    let charging = status.ACLineStatus == 1;
    let percent = if status.BatteryLifePercent == 255 {
        None
    } else {
        Some(status.BatteryLifePercent as f32)
    };
    (percent, charging)
}

fn filetime(value: FILETIME) -> u64 {
    ((value.dwHighDateTime as u64) << 32) | value.dwLowDateTime as u64
}

struct GpuCounter {
    query: isize,
    counter: isize,
}

impl GpuCounter {
    fn new() -> Option<Self> {
        unsafe {
            let mut query = 0isize;
            if PdhOpenQueryW(PCWSTR::null(), 0, &mut query) != 0 {
                log::warn("GPU counter: PdhOpenQueryW failed");
                return None;
            }
            let mut counter = 0isize;
            let status = PdhAddEnglishCounterW(
                query,
                windows::core::w!("\\GPU Engine(*)\\Utilization Percentage"),
                0,
                &mut counter,
            );
            if status != 0 {
                let _ = PdhCloseQuery(query);
                log::warn(&format!(
                    "GPU counter unavailable (PdhAddEnglishCounterW={status})"
                ));
                return None;
            }
            Some(Self { query, counter })
        }
    }

    fn sample(&self) -> Option<f32> {
        unsafe {
            if PdhCollectQueryData(self.query) != 0 {
                return None;
            }
            let mut size = 0u32;
            let mut count = 0u32;
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                None,
            );
            if status != PDH_MORE_DATA && status != 0 {
                return None;
            }
            if size == 0 || count == 0 {
                return Some(0.0);
            }
            let mut buffer = vec![0u64; (size as usize).div_ceil(8)];
            let status = PdhGetFormattedCounterArrayW(
                self.counter,
                PDH_FMT_DOUBLE,
                &mut size,
                &mut count,
                Some(buffer.as_mut_ptr() as *mut PDH_FMT_COUNTERVALUE_ITEM_W),
            );
            if status != 0 {
                return None;
            }
            let items = buffer.as_ptr() as *const PDH_FMT_COUNTERVALUE_ITEM_W;
            let mut maximum = 0.0f64;
            for index in 0..count as usize {
                let item = &*items.add(index);
                let value = item.FmtValue;
                if value.CStatus == PDH_CSTATUS_VALID_DATA || value.CStatus == PDH_CSTATUS_NEW_DATA
                {
                    let value = value.Anonymous.doubleValue;
                    if value.is_finite() {
                        maximum = maximum.max(value);
                    }
                }
            }
            Some(maximum.clamp(0.0, 100.0) as f32)
        }
    }
}

impl Drop for GpuCounter {
    fn drop(&mut self) {
        unsafe {
            let _ = PdhCloseQuery(self.query);
        }
    }
}
