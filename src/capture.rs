use std::ffi::c_void;
use std::mem::size_of;
use std::slice;

use windows::Win32::Foundation::{HANDLE, HWND};
use windows::Win32::Graphics::Gdi::{
    BitBlt, CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
    SelectObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP, HDC, HGDIOBJ,
    SRCCOPY,
};

pub struct DesktopCapture {
    mem_dc: HDC,
    bitmap: HBITMAP,
    old_bitmap: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl DesktopCapture {
    pub fn new(width: u32, height: u32) -> Result<Self, String> {
        let width = width.max(1);
        let height = height.max(1);
        unsafe {
            let screen_dc = GetDC(HWND::default());
            if screen_dc.0.is_null() {
                return Err("GetDC 失败".into());
            }
            let mem_dc = CreateCompatibleDC(screen_dc);
            if mem_dc.0.is_null() {
                let _ = ReleaseDC(HWND::default(), screen_dc);
                return Err("CreateCompatibleDC 失败".into());
            }

            let mut info = BITMAPINFO::default();
            info.bmiHeader.biSize = size_of::<BITMAPINFOHEADER>() as u32;
            info.bmiHeader.biWidth = width as i32;
            info.bmiHeader.biHeight = -(height as i32);
            info.bmiHeader.biPlanes = 1;
            info.bmiHeader.biBitCount = 32;
            info.bmiHeader.biCompression = BI_RGB.0;

            let mut bits: *mut c_void = std::ptr::null_mut();
            let bitmap = match CreateDIBSection(
                screen_dc,
                &info,
                DIB_RGB_COLORS,
                &mut bits,
                HANDLE::default(),
                0,
            ) {
                Ok(bitmap) => bitmap,
                Err(error) => {
                    let _ = DeleteDC(mem_dc);
                    let _ = ReleaseDC(HWND::default(), screen_dc);
                    return Err(format!("CreateDIBSection 失败: {error}"));
                }
            };
            if bits.is_null() {
                let _ = DeleteObject(bitmap);
                let _ = DeleteDC(mem_dc);
                let _ = ReleaseDC(HWND::default(), screen_dc);
                return Err("CreateDIBSection 未返回像素缓冲".into());
            }
            let old_bitmap = SelectObject(mem_dc, bitmap);
            let _ = ReleaseDC(HWND::default(), screen_dc);

            Ok(Self {
                mem_dc,
                bitmap,
                old_bitmap,
                bits: bits as *mut u8,
                width,
                height,
            })
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn pixels(&self) -> &[u8] {
        unsafe { slice::from_raw_parts(self.bits, (self.width * self.height * 4) as usize) }
    }

    pub fn capture(&self, screen_x: i32, screen_y: i32) -> Result<(), String> {
        unsafe {
            let screen_dc = GetDC(HWND::default());
            if screen_dc.0.is_null() {
                return Err("GetDC 失败".into());
            }
            let result = BitBlt(
                self.mem_dc,
                0,
                0,
                self.width as i32,
                self.height as i32,
                screen_dc,
                screen_x,
                screen_y,
                SRCCOPY,
            );
            let _ = ReleaseDC(HWND::default(), screen_dc);
            result.map_err(|error| format!("BitBlt 失败: {error}"))
        }
    }
}

impl Drop for DesktopCapture {
    fn drop(&mut self) {
        unsafe {
            let _ = SelectObject(self.mem_dc, self.old_bitmap);
            let _ = DeleteObject(self.bitmap);
            let _ = DeleteDC(self.mem_dc);
        }
    }
}
