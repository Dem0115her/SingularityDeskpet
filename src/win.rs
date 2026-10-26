use std::ffi::c_void;
use std::mem::size_of;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{BOOL, COLORREF, HANDLE, HINSTANCE, HWND, POINT, RECT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, GetMonitorInfoW,
    MonitorFromWindow, ReleaseDC, SelectObject, AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO,
    BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ, MONITORINFO,
    MONITOR_DEFAULTTONEAREST,
};
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateIconFromResourceEx, DestroyIcon, GetClassNameW, GetCursorPos, GetForegroundWindow,
    GetWindowLongPtrW, GetWindowRect, IsWindowVisible, LoadIconW, MessageBoxW, SetWindowLongPtrW,
    SetWindowPos, UpdateLayeredWindow, GWL_EXSTYLE, HWND_TOPMOST, IDI_APPLICATION, IMAGE_FLAGS,
    MB_ICONERROR, MB_OK, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    ULW_ALPHA, WDA_EXCLUDEFROMCAPTURE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    WS_EX_TOPMOST, WS_EX_TRANSPARENT,
};

use crate::log;

pub fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn set_process_dpi_aware() {
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }
}

pub fn fatal_message(message: &str) {
    let text = wide(message);
    let caption = wide("奇点 · 启动失败");
    unsafe {
        MessageBoxW(
            HWND::default(),
            PCWSTR(text.as_ptr()),
            PCWSTR(caption.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
    }
}

pub fn cursor_pos() -> Option<POINT> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point).ok().map(|_| point) }
}

pub fn monitor_work_area(hwnd: HWND) -> RECT {
    unsafe {
        let monitor = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if GetMonitorInfoW(monitor, &mut info).as_bool() {
            info.rcWork
        } else {
            RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
            }
        }
    }
}

pub fn is_fullscreen_foreground(own: HWND) -> bool {
    unsafe {
        let fg = GetForegroundWindow();
        if fg.0.is_null() || fg == own {
            return false;
        }
        if !IsWindowVisible(fg).as_bool() {
            return false;
        }
        let mut class = [0u16; 256];
        let len = GetClassNameW(fg, &mut class);
        if len > 0 {
            let name = String::from_utf16_lossy(&class[..len as usize]);
            if matches!(
                name.as_str(),
                "Progman"
                    | "WorkerW"
                    | "Shell_TrayWnd"
                    | "Shell_SecondaryTrayWnd"
                    | "Windows.UI.Core.CoreWindow"
            ) {
                return false;
            }
        }
        let mut rect = RECT::default();
        if GetWindowRect(fg, &mut rect).is_err() {
            return false;
        }
        let monitor = MonitorFromWindow(fg, MONITOR_DEFAULTTONEAREST);
        let mut info = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..Default::default()
        };
        if !GetMonitorInfoW(monitor, &mut info).as_bool() {
            return false;
        }
        let screen = info.rcMonitor;
        (rect.left - screen.left).abs() <= 2
            && (rect.top - screen.top).abs() <= 2
            && (screen.right - rect.right).abs() <= 2
            && (screen.bottom - rect.bottom).abs() <= 2
    }
}

pub fn bring_to_top(hwnd: HWND) {
    unsafe {
        let _ = SetWindowPos(
            hwnd,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

pub fn set_click_through(hwnd: HWND, enabled: bool) {
    unsafe {
        let mut style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let bits = WS_EX_LAYERED.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0 | WS_EX_TOPMOST.0;
        style &= bits | WS_EX_TRANSPARENT.0;
        if enabled {
            style |= WS_EX_TRANSPARENT.0;
        } else {
            style &= !WS_EX_TRANSPARENT.0;
        }
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style as isize);
        let _ = SetWindowPos(
            hwnd,
            HWND::default(),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE | SWP_FRAMECHANGED,
        );
    }
}

pub fn exclude_from_capture(hwnd: HWND) {
    if std::env::var_os("SINGULARITY_ALLOW_CAPTURE").is_some() {
        return;
    }
    unsafe {
        let _ = windows::Win32::UI::WindowsAndMessaging::SetWindowDisplayAffinity(
            hwnd,
            WDA_EXCLUDEFROMCAPTURE,
        );
    }
}

pub fn load_app_icon() -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    const ICO: &[u8] = include_bytes!("../assets/app.ico");
    if ICO.len() < 6 {
        return fallback_icon();
    }
    let count = u16::from_le_bytes([ICO[4], ICO[5]]) as usize;
    let mut best: Option<(usize, u32, u32, usize, usize)> = None;
    for i in 0..count {
        let base = 6 + i * 16;
        if base + 16 > ICO.len() {
            break;
        }
        let width = if ICO[base] == 0 {
            256
        } else {
            ICO[base] as u32
        };
        let height = if ICO[base + 1] == 0 {
            256
        } else {
            ICO[base + 1] as u32
        };
        let bytes =
            u32::from_le_bytes([ICO[base + 8], ICO[base + 9], ICO[base + 10], ICO[base + 11]])
                as usize;
        let offset = u32::from_le_bytes([
            ICO[base + 12],
            ICO[base + 13],
            ICO[base + 14],
            ICO[base + 15],
        ]) as usize;
        if offset + bytes > ICO.len() {
            continue;
        }
        let area = width * height;
        if best.map(|b| area > b.1 * b.2).unwrap_or(true) {
            best = Some((i, width, height, offset, bytes));
        }
    }
    if let Some((_, width, height, offset, bytes)) = best {
        let data = &ICO[offset..offset + bytes];
        if let Ok(icon) = unsafe {
            CreateIconFromResourceEx(
                data,
                BOOL(1),
                0x0003_0000,
                width as i32,
                height as i32,
                IMAGE_FLAGS(0),
            )
        } {
            return Some(icon);
        }
    }
    fallback_icon()
}

fn fallback_icon() -> Option<windows::Win32::UI::WindowsAndMessaging::HICON> {
    unsafe { LoadIconW(HINSTANCE::default(), IDI_APPLICATION).ok() }
}

pub fn destroy_icon(icon: windows::Win32::UI::WindowsAndMessaging::HICON) {
    unsafe {
        let _ = DestroyIcon(icon);
    }
}

pub struct LayeredSurface {
    hwnd: HWND,
    screen_dc: HDC,
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl LayeredSurface {
    pub fn new(hwnd: HWND, width: i32, height: i32) -> Result<Self, String> {
        let width = width.max(16);
        let height = height.max(16);
        unsafe {
            let screen_dc = GetDC(HWND::default());
            if screen_dc.0.is_null() {
                return Err("GetDC 失败".into());
            }
            let mut mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.0.is_null() {
                mem_dc = CreateCompatibleDC(HDC::default());
            }
            if mem_dc.0.is_null() {
                let _ = ReleaseDC(HWND::default(), screen_dc);
                return Err("CreateCompatibleDC 失败".into());
            }
            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width;
            info.bmiHeader.biHeight = -height;
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB.0;
            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = CreateDIBSection(
                screen_dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                HANDLE::default(),
                0,
            )
            .map_err(|e| format!("CreateDIBSection 失败: {e}"))?;
            let old_bitmap = SelectObject(mem_dc, bitmap);
            Ok(Self {
                hwnd,
                screen_dc,
                mem_dc,
                bitmap,
                old_bitmap,
                bits: bits as *mut u8,
                width,
                height,
            })
        }
    }

    pub fn update(&self, pixels: &[u8], left: i32, top: i32) -> Result<(), String> {
        let expected = (self.width * self.height * 4) as usize;
        if pixels.len() < expected {
            return Err(format!("像素缓冲过小: {} < {}", pixels.len(), expected));
        }
        unsafe {
            std::ptr::copy_nonoverlapping(pixels.as_ptr(), self.bits, expected);
            let dst = POINT { x: left, y: top };
            let size = SIZE {
                cx: self.width,
                cy: self.height,
            };
            let src = POINT { x: 0, y: 0 };
            let blend = BLENDFUNCTION {
                BlendOp: AC_SRC_OVER as u8,
                BlendFlags: 0,
                SourceConstantAlpha: 255,
                AlphaFormat: AC_SRC_ALPHA as u8,
            };
            UpdateLayeredWindow(
                self.hwnd,
                self.screen_dc,
                Some(&dst),
                Some(&size),
                self.mem_dc,
                Some(&src),
                COLORREF(0),
                Some(&blend),
                ULW_ALPHA,
            )
            .map_err(|e| format!("UpdateLayeredWindow 失败: {e}"))
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem_dc, self.old_bitmap);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.mem_dc);
            let _ = ReleaseDC(HWND::default(), self.screen_dc);
        }
    }
}

pub fn single_instance(name: &str) -> bool {
    use windows::Win32::Foundation::ERROR_ALREADY_EXISTS;
    use windows::Win32::System::Threading::CreateMutexW;
    let name = wide(name);
    unsafe {
        let handle = CreateMutexW(None, false, PCWSTR(name.as_ptr()));
        match handle {
            Ok(handle) => {
                if windows::Win32::Foundation::GetLastError() == ERROR_ALREADY_EXISTS {
                    let _ = windows::Win32::Foundation::CloseHandle(handle);
                    false
                } else {
                    // Intentionally leaked so the mutex lives for the whole process.
                    true
                }
            }
            Err(error) => {
                log::warn(&format!("CreateMutexW failed: {error}"));
                true
            }
        }
    }
}
