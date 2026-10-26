use std::mem::size_of;

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{CloseHandle, BOOL, HANDLE, HWND, LPARAM, POINT, RECT};
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Memory::{
    VirtualAllocEx, VirtualFreeEx, MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE,
};
use windows::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_INFORMATION, PROCESS_VM_OPERATION, PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::Controls::{
    LVIF_TEXT, LVITEMW, LVM_GETITEMCOUNT, LVM_GETITEMRECT, LVM_GETITEMTEXTW, LVM_SETITEMPOSITION,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GetClassNameW, GetWindowRect,
    GetWindowThreadProcessId, SendMessageTimeoutW, SystemParametersInfoW, WindowFromPoint,
    SMTO_ABORTIFHUNG, SPI_GETWORKAREA, SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS,
};

use crate::capture::DesktopCapture;
use crate::log;

const LVM_SETITEMPOSITION32: u32 = 0x1031;

#[derive(Clone, Debug)]
pub struct DesktopIcon {
    pub index: i32,
    pub name: String,
    pub screen: RECT,
}

impl DesktopIcon {
    pub fn center(&self) -> (i32, i32) {
        (
            (self.screen.left + self.screen.right) / 2,
            (self.screen.top + self.screen.bottom) / 2,
        )
    }
}

pub struct PatchPixels {
    pub rect: RECT,
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    pub sprite: Option<SpritePixels>,
}

pub struct SpritePixels {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub fn find_listview() -> Option<HWND> {
    unsafe {
        let progman = FindWindowW(w!("Progman"), PCWSTR::null()).ok()?;
        if let Ok(def_view) = FindWindowExW(
            progman,
            HWND::default(),
            w!("SHELLDLL_DefView"),
            PCWSTR::null(),
        ) {
            if !def_view.0.is_null() {
                if let Ok(list) = FindWindowExW(
                    def_view,
                    HWND::default(),
                    w!("SysListView32"),
                    PCWSTR::null(),
                ) {
                    if !list.0.is_null() {
                        return Some(list);
                    }
                }
            }
        }

        let mut def_view = HWND::default();
        let _ = EnumWindows(
            Some(enum_def_view),
            LPARAM(&mut def_view as *mut HWND as isize),
        );
        if def_view.0.is_null() {
            return None;
        }
        FindWindowExW(
            def_view,
            HWND::default(),
            w!("SysListView32"),
            PCWSTR::null(),
        )
        .ok()
        .filter(|hwnd| !hwnd.0.is_null())
    }
}

unsafe extern "system" fn enum_def_view(hwnd: HWND, lparam: LPARAM) -> BOOL {
    let out = &mut *(lparam.0 as *mut HWND);
    if let Ok(candidate) = FindWindowExW(
        hwnd,
        HWND::default(),
        w!("SHELLDLL_DefView"),
        PCWSTR::null(),
    ) {
        if !candidate.0.is_null() {
            *out = candidate;
            return BOOL(0);
        }
    }
    BOOL(1)
}

pub fn list_icons(work: RECT) -> Vec<DesktopIcon> {
    unsafe {
        let Some(list_view) = find_listview() else {
            return Vec::new();
        };
        let mut list_rect = RECT::default();
        if GetWindowRect(list_view, &mut list_rect).is_err() {
            return Vec::new();
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(list_view, Some(&mut pid));
        if pid == 0 {
            return Vec::new();
        }
        let Ok(process) = OpenProcess(
            PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        ) else {
            log::warn("desktop icons: cannot open explorer process");
            return Vec::new();
        };
        let remote = VirtualAllocEx(
            process,
            None,
            4096,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            let _ = CloseHandle(process);
            return Vec::new();
        }

        let mut icons = Vec::new();
        let count = send_timeout(list_view, LVM_GETITEMCOUNT, 0, 0) as i32;
        if count <= 0 || count > 600 {
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
            let _ = CloseHandle(process);
            return icons;
        }

        let debug = std::env::var_os("SINGULARITY_ICON_DEBUG").is_some();
        for index in 0..count {
            let mut rect = RECT::default();
            if !write_remote(process, remote, &rect) {
                if debug {
                    println!("item {index}: write_remote failed");
                }
                continue;
            }
            send_timeout(list_view, LVM_GETITEMRECT, index as usize, remote as isize);
            if !read_remote(process, remote, &mut rect) {
                if debug {
                    println!("item {index}: read_remote failed");
                }
                continue;
            }
            let width = rect.right - rect.left;
            let height = rect.bottom - rect.top;
            if debug && index < 8 {
                println!("item {index}: {rect:?} ({width}x{height})");
            }
            if width < 8 || height < 8 {
                continue;
            }
            let screen = RECT {
                left: list_rect.left + rect.left,
                top: list_rect.top + rect.top,
                right: list_rect.left + rect.right,
                bottom: list_rect.top + rect.bottom,
            };
            if screen.left < work.left
                || screen.top < work.top
                || screen.right > work.right
                || screen.bottom > work.bottom
            {
                continue;
            }
            if !icon_visible(list_view, &screen) {
                if debug && index < 8 {
                    let point = POINT {
                        x: (screen.left + screen.right) / 2,
                        y: (screen.top + screen.bottom) / 2,
                    };
                    let hwnd = WindowFromPoint(point);
                    let mut class = [0u16; 256];
                    let len = GetClassNameW(hwnd, &mut class);
                    println!(
                        "item {index}: invisible at {point:?} hwnd={hwnd:?} class={} work={work:?}",
                        String::from_utf16_lossy(&class[..len.max(0) as usize])
                    );
                }
                continue;
            }
            let name = item_text(process, list_view, remote, index);
            icons.push(DesktopIcon {
                index,
                name,
                screen,
            });
        }

        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
        icons
    }
}

fn icon_visible(list_view: HWND, rect: &RECT) -> bool {
    let point = POINT {
        x: (rect.left + rect.right) / 2,
        y: (rect.top + rect.bottom) / 2,
    };
    unsafe {
        let hwnd = WindowFromPoint(point);
        if hwnd == list_view {
            return true;
        }
        if hwnd.0.is_null() {
            return false;
        }
        let mut class = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut class);
        let name = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        matches!(
            name.as_str(),
            "SysListView32" | "WorkerW" | "Progman" | "SHELLDLL_DefView"
        )
    }
}

pub fn debug_dump() {
    let mut work = RECT::default();
    unsafe {
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut RECT as *mut core::ffi::c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
    }
    let list_view = find_listview();
    println!("desktop work area: {work:?}");
    println!("list view: {list_view:?}");
    if let Some(list_view) = list_view {
        let count = send_timeout(list_view, LVM_GETITEMCOUNT, 0, 0);
        println!("raw item count: {count}");
        let mut class = [0u16; 256];
        let len = unsafe { GetClassNameW(list_view, &mut class) };
        println!(
            "list class: {}",
            String::from_utf16_lossy(&class[..len.max(0) as usize])
        );
        let mut rect = RECT::default();
        unsafe {
            let _ = GetWindowRect(list_view, &mut rect);
        }
        println!("list rect: {rect:?}");
    }
    let icons = list_icons(work);
    println!("visible icons: {}", icons.len());
    for icon in icons {
        println!(
            "#{:>3}  {:>5},{:<5} {:>3}x{:<3}  {}",
            icon.index,
            icon.screen.left,
            icon.screen.top,
            icon.screen.right - icon.screen.left,
            icon.screen.bottom - icon.screen.top,
            icon.name
        );
    }
}

pub fn is_desktop_point(x: i32, y: i32) -> bool {
    unsafe {
        let hwnd = WindowFromPoint(POINT { x, y });
        if hwnd.0.is_null() {
            return false;
        }
        let Some(list_view) = find_listview() else {
            return false;
        };
        if hwnd == list_view {
            return true;
        }
        let mut class = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut class);
        let name = String::from_utf16_lossy(&class[..len.max(0) as usize]);
        matches!(
            name.as_str(),
            "SysListView32" | "WorkerW" | "Progman" | "SHELLDLL_DefView"
        )
    }
}

pub fn move_icon(icon: &DesktopIcon, screen_x: i32, screen_y: i32) -> bool {
    unsafe {
        let Some(list_view) = find_listview() else {
            return false;
        };
        let mut list_rect = RECT::default();
        if GetWindowRect(list_view, &mut list_rect).is_err() {
            return false;
        }
        let x = (screen_x - list_rect.left).clamp(0, 16000);
        let y = (screen_y - list_rect.top).clamp(0, 16000);
        let packed = ((y as usize & 0xffff) << 16) | (x as usize & 0xffff);
        send_timeout(
            list_view,
            LVM_SETITEMPOSITION,
            icon.index as usize,
            packed as isize,
        );
        invalidate(list_view);

        if icon_moved(icon.index, screen_x, screen_y) {
            log::info(&format!(
                "desktop icon moved: {} -> ({screen_x},{screen_y})",
                icon.name
            ));
            return true;
        }

        let mut pid = 0u32;
        GetWindowThreadProcessId(list_view, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(process) = OpenProcess(
            PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        ) else {
            return false;
        };
        let remote = VirtualAllocEx(process, None, 32, MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if !remote.is_null() {
            let point = POINT { x, y };
            if write_remote(process, remote, &point) {
                send_timeout(
                    list_view,
                    LVM_SETITEMPOSITION32,
                    icon.index as usize,
                    remote as isize,
                );
                invalidate(list_view);
            }
            let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        }
        let _ = CloseHandle(process);
        let moved = icon_moved(icon.index, screen_x, screen_y);
        if moved {
            log::info(&format!(
                "desktop icon moved (32-bit): {} -> ({screen_x},{screen_y})",
                icon.name
            ));
        } else {
            log::warn(&format!(
                "desktop icon move failed: {} -> ({screen_x},{screen_y})",
                icon.name
            ));
        }
        moved
    }
}

fn icon_moved(index: i32, screen_x: i32, screen_y: i32) -> bool {
    unsafe {
        let Some(list_view) = find_listview() else {
            return false;
        };
        let mut list_rect = RECT::default();
        if GetWindowRect(list_view, &mut list_rect).is_err() {
            return false;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(list_view, Some(&mut pid));
        if pid == 0 {
            return false;
        }
        let Ok(process) = OpenProcess(
            PROCESS_VM_OPERATION | PROCESS_VM_READ | PROCESS_VM_WRITE | PROCESS_QUERY_INFORMATION,
            false,
            pid,
        ) else {
            return false;
        };
        let remote = VirtualAllocEx(
            process,
            None,
            4096,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote.is_null() {
            let _ = CloseHandle(process);
            return false;
        }
        let mut rect = RECT::default();
        let ok = write_remote(process, remote, &rect) && {
            send_timeout(list_view, LVM_GETITEMRECT, index as usize, remote as isize);
            read_remote(process, remote, &mut rect)
        };
        let _ = VirtualFreeEx(process, remote, 0, MEM_RELEASE);
        let _ = CloseHandle(process);
        ok && (list_rect.left + rect.left - screen_x).abs() <= 64
            && (list_rect.top + rect.top - screen_y).abs() <= 64
    }
}

pub fn capture_patch(icon: &DesktopIcon, work: RECT, halo: i32) -> Option<PatchPixels> {
    let rect = RECT {
        left: (icon.screen.left - halo).max(work.left),
        top: (icon.screen.top - halo).max(work.top),
        right: (icon.screen.right + halo).min(work.right),
        bottom: (icon.screen.bottom + halo).min(work.bottom),
    };
    if rect.right - rect.left < 8 || rect.bottom - rect.top < 8 {
        return None;
    }
    if rect.left > icon.screen.left
        || rect.top > icon.screen.top
        || rect.right < icon.screen.right
        || rect.bottom < icon.screen.bottom
    {
        return None;
    }
    let width = (rect.right - rect.left) as u32;
    let height = (rect.bottom - rect.top) as u32;
    let capture = DesktopCapture::new(width, height).ok()?;
    capture.capture(rect.left, rect.top).ok()?;
    let mut pixels = capture.pixels().to_vec();

    let inner = RECT {
        left: icon.screen.left - rect.left,
        top: icon.screen.top - rect.top,
        right: icon.screen.right - rect.left,
        bottom: icon.screen.bottom - rect.top,
    };
    let sprite = crop_sprite(capture.pixels(), width, height, inner);
    inpaint(&mut pixels, width, height, inner);
    Some(PatchPixels {
        rect,
        width,
        height,
        pixels,
        sprite,
    })
}

fn crop_sprite(source: &[u8], width: u32, height: u32, rect: RECT) -> Option<SpritePixels> {
    let x0 = rect.left.clamp(0, width as i32 - 1) as u32;
    let y0 = rect.top.clamp(0, height as i32 - 1) as u32;
    let x1 = rect.right.clamp(x0 as i32 + 1, width as i32) as u32;
    let y1 = rect.bottom.clamp(y0 as i32 + 1, height as i32) as u32;
    let sprite_width = x1 - x0;
    let sprite_height = y1 - y0;
    if sprite_width < 4 || sprite_height < 4 {
        return None;
    }
    let mut pixels = vec![0u8; (sprite_width * sprite_height * 4) as usize];
    for y in 0..sprite_height {
        for x in 0..sprite_width {
            let edge = x
                .min(y)
                .min(sprite_width - 1 - x)
                .min(sprite_height - 1 - y) as f32;
            let alpha = (edge / 5.0).clamp(0.0, 1.0);
            let source_index = (((y0 + y) * width + (x0 + x)) * 4) as usize;
            let target_index = ((y * sprite_width + x) * 4) as usize;
            pixels[target_index] = (source[source_index] as f32 * alpha) as u8;
            pixels[target_index + 1] = (source[source_index + 1] as f32 * alpha) as u8;
            pixels[target_index + 2] = (source[source_index + 2] as f32 * alpha) as u8;
            pixels[target_index + 3] = (255.0 * alpha) as u8;
        }
    }
    Some(SpritePixels {
        width: sprite_width,
        height: sprite_height,
        pixels,
    })
}

fn inpaint(pixels: &mut [u8], width: u32, height: u32, inner: RECT) {
    let x0 = inner.left.clamp(2, width as i32 - 2);
    let y0 = inner.top.clamp(2, height as i32 - 2);
    let x1 = inner.right.clamp(x0 + 1, width as i32 - 2);
    let y1 = inner.bottom.clamp(y0 + 1, height as i32 - 2);
    let w = x1 - x0;
    let h = y1 - y0;
    if w <= 0 || h <= 0 {
        return;
    }
    let source = pixels.to_vec();
    let sample = |x: i32, y: i32| -> [u8; 3] {
        let x = x.clamp(0, width as i32 - 1) as u32;
        let y = y.clamp(0, height as i32 - 1) as u32;
        let i = ((y * width + x) * 4) as usize;
        [source[i + 2], source[i + 1], source[i]]
    };
    let lerp = |a: [u8; 3], b: [u8; 3], t: f32| -> [u8; 3] {
        let t = t.clamp(0.0, 1.0);
        [
            (a[0] as f32 + (b[0] as f32 - a[0] as f32) * t) as u8,
            (a[1] as f32 + (b[1] as f32 - a[1] as f32) * t) as u8,
            (a[2] as f32 + (b[2] as f32 - a[2] as f32) * t) as u8,
        ]
    };
    let cx = (x0 + x1 - 1) as f32 / 2.0;
    let cy = (y0 + y1 - 1) as f32 / 2.0;
    let max_d = ((cx * cx + cy * cy).sqrt()).max(1.0);
    for y in y0..y1 {
        let ty = (y - y0) as f32 / (h - 1).max(1) as f32;
        for x in x0..x1 {
            let tx = (x - x0) as f32 / (w - 1).max(1) as f32;
            let left = sample(x0 - 2, y);
            let right = sample(x1 + 1, y);
            let top = sample(x, y0 - 2);
            let bottom = sample(x, y1 + 1);
            let horizontal = lerp(left, right, tx);
            let vertical = lerp(top, bottom, ty);
            let mixed = lerp(horizontal, vertical, 0.5);
            let dx = x as f32 - cx;
            let dy = y as f32 - cy;
            let d = (dx * dx + dy * dy).sqrt() / max_d;
            let shade = 1.0 - 0.09 * (1.0 - d.clamp(0.0, 1.0));
            let noise = (x
                .wrapping_mul(374761393)
                .wrapping_add(y.wrapping_mul(668265263))
                & 7)
                - 3;
            let i = ((y as u32 * width + x as u32) * 4) as usize;
            for (channel, value) in mixed.iter().enumerate() {
                let shaded = (*value as f32 * shade).round() as i32 + noise;
                pixels[i + 2 - channel] = shaded.clamp(0, 255) as u8;
            }
            pixels[i + 3] = 255;
        }
    }
    for _ in 0..24 {
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y as u32 * width + x as u32) * 4) as usize;
                let neighbors = [
                    ((y as u32 * width + (x - 1) as u32) * 4) as usize,
                    ((y as u32 * width + (x + 1) as u32) * 4) as usize,
                    (((y - 1) as u32 * width + x as u32) * 4) as usize,
                    (((y + 1) as u32 * width + x as u32) * 4) as usize,
                ];
                for channel in 0..3 {
                    let avg: u32 = neighbors
                        .iter()
                        .map(|n| pixels[n + channel] as u32)
                        .sum::<u32>()
                        / 4;
                    pixels[i + channel] =
                        ((pixels[i + channel] as u32 * 45 + avg * 55) / 100) as u8;
                }
                pixels[i + 3] = 255;
            }
        }
    }
}

fn item_text(
    process: HANDLE,
    list_view: HWND,
    remote_item: *mut core::ffi::c_void,
    index: i32,
) -> String {
    unsafe {
        let remote_text = VirtualAllocEx(
            process,
            None,
            2048,
            MEM_COMMIT | MEM_RESERVE,
            PAGE_READWRITE,
        );
        if remote_text.is_null() {
            return String::new();
        }
        let mut item = LVITEMW {
            mask: LVIF_TEXT,
            iItem: index,
            iSubItem: 0,
            pszText: PWSTR(remote_text as *mut u16),
            cchTextMax: 512,
            ..Default::default()
        };
        item.iItem = index;
        let ok = write_remote(process, remote_item, &item);
        let mut name = String::new();
        if ok {
            send_timeout(
                list_view,
                LVM_GETITEMTEXTW,
                index as usize,
                remote_item as isize,
            );
            let mut buffer = vec![0u8; 1024];
            if read_bytes(process, remote_text, &mut buffer) {
                let mut chars = Vec::new();
                for chunk in buffer.as_chunks::<2>().0 {
                    let value = u16::from_le_bytes(*chunk);
                    if value == 0 {
                        break;
                    }
                    chars.push(value);
                }
                name = String::from_utf16_lossy(&chars);
            }
        }
        let _ = VirtualFreeEx(process, remote_text, 0, MEM_RELEASE);
        name
    }
}

fn send_timeout(hwnd: HWND, msg: u32, wparam: usize, lparam: isize) -> usize {
    let mut result = 0usize;
    unsafe {
        let _ = SendMessageTimeoutW(
            hwnd,
            msg,
            windows::Win32::Foundation::WPARAM(wparam),
            LPARAM(lparam),
            SMTO_ABORTIFHUNG,
            900,
            Some(&mut result),
        );
    }
    result
}

fn write_remote<T: Copy>(process: HANDLE, remote: *mut core::ffi::c_void, value: &T) -> bool {
    unsafe {
        WriteProcessMemory(
            process,
            remote,
            value as *const T as *const core::ffi::c_void,
            size_of::<T>(),
            None,
        )
        .is_ok()
    }
}

fn read_remote<T: Copy>(process: HANDLE, remote: *mut core::ffi::c_void, value: &mut T) -> bool {
    unsafe {
        ReadProcessMemory(
            process,
            remote,
            value as *mut T as *mut core::ffi::c_void,
            size_of::<T>(),
            None,
        )
        .is_ok()
    }
}

fn read_bytes(process: HANDLE, remote: *mut core::ffi::c_void, buffer: &mut [u8]) -> bool {
    unsafe {
        ReadProcessMemory(
            process,
            remote,
            buffer.as_mut_ptr() as *mut core::ffi::c_void,
            buffer.len(),
            None,
        )
        .is_ok()
    }
}

fn invalidate(hwnd: HWND) {
    unsafe {
        let _ = windows::Win32::Graphics::Gdi::InvalidateRect(hwnd, None, true);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inpaint_small_patch_does_not_overflow() {
        let width = 32u32;
        let height = 32u32;
        let mut pixels = vec![128u8; (width * height * 4) as usize];
        for alpha in pixels.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        let inner = RECT {
            left: 8,
            top: 8,
            right: 24,
            bottom: 24,
        };
        inpaint(&mut pixels, width, height, inner);
        assert!(pixels
            .as_chunks::<4>()
            .0
            .iter()
            .all(|pixel| pixel[3] == 255));
    }

    #[test]
    fn icon_sprite_crop_is_masked() {
        let width = 32u32;
        let height = 32u32;
        let mut pixels = vec![200u8; (width * height * 4) as usize];
        for alpha in pixels.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        let sprite = crop_sprite(
            &pixels,
            width,
            height,
            RECT {
                left: 4,
                top: 4,
                right: 28,
                bottom: 28,
            },
        )
        .expect("sprite");
        assert_eq!(sprite.width, 24);
        assert_eq!(sprite.height, 24);
        let center = ((12 * 24) + 12) * 4;
        assert_eq!(sprite.pixels[center + 3], 255);
        assert_eq!(sprite.pixels[3], 0);
    }
}
