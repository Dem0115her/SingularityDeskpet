use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{
    HTCLIENT, MA_NOACTIVATE, WM_ERASEBKGND, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE,
    WM_MOUSEMOVE, WM_NCHITTEST, WM_RBUTTONUP,
};

use crate::settings::parse_accent_hex;
use crate::win::LayeredSurface;

pub enum RegionResponse {
    Handled(LRESULT),
    Selected(RECT),
    Cancelled,
}

pub struct RegionSelector {
    hwnd: HWND,
    surface: LayeredSurface,
    origin: RECT,
    width: i32,
    height: i32,
    pixels: Vec<u8>,
    start: Option<POINT>,
    current: Option<POINT>,
    border: [u8; 3],
    min_size: i32,
}

impl RegionSelector {
    pub fn new(hwnd: HWND, origin: RECT, accent: &str, min_size: i32) -> Result<Self, String> {
        let width = (origin.right - origin.left).max(64);
        let height = (origin.bottom - origin.top).max(64);
        let surface = LayeredSurface::new(hwnd, width, height)?;
        let linear = parse_accent_hex(accent).unwrap_or([1.0, 0.64, 0.40]);
        let border = [
            linear_to_srgb(linear[2]),
            linear_to_srgb(linear[1]),
            linear_to_srgb(linear[0]),
        ];
        let mut selector = Self {
            hwnd,
            surface,
            origin,
            width,
            height,
            pixels: vec![0; (width * height * 4) as usize],
            start: None,
            current: None,
            border,
            min_size: min_size.max(120),
        };
        selector.redraw();
        selector.present()?;
        Ok(selector)
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn handle_message(
        &mut self,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<RegionResponse> {
        let x = (lparam.0 & 0xFFFF) as i16 as i32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as i32;
        let point = POINT { x, y };
        match msg {
            WM_MOUSEACTIVATE => Some(RegionResponse::Handled(LRESULT(MA_NOACTIVATE as isize))),
            WM_ERASEBKGND => Some(RegionResponse::Handled(LRESULT(1))),
            WM_NCHITTEST => Some(RegionResponse::Handled(LRESULT(HTCLIENT as isize))),
            WM_LBUTTONDOWN => {
                unsafe {
                    SetCapture(self.hwnd);
                }
                self.start = Some(point);
                self.current = Some(point);
                self.redraw();
                let _ = self.present();
                Some(RegionResponse::Handled(LRESULT(0)))
            }
            WM_MOUSEMOVE => {
                if self.start.is_some() {
                    self.current = Some(point);
                    self.redraw();
                    let _ = self.present();
                }
                Some(RegionResponse::Handled(LRESULT(0)))
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                if let (Some(start), Some(current)) = (self.start, self.current) {
                    let rect = normalized_rect(start, current);
                    let width = rect.right - rect.left;
                    let height = rect.bottom - rect.top;
                    if width >= self.min_size && height >= self.min_size {
                        let screen = RECT {
                            left: self.origin.left + rect.left,
                            top: self.origin.top + rect.top,
                            right: self.origin.left + rect.right,
                            bottom: self.origin.top + rect.bottom,
                        };
                        return Some(RegionResponse::Selected(screen));
                    }
                }
                self.start = None;
                self.current = None;
                self.redraw();
                let _ = self.present();
                Some(RegionResponse::Handled(LRESULT(0)))
            }
            WM_RBUTTONUP => Some(RegionResponse::Cancelled),
            _ => None,
        }
    }

    fn present(&mut self) -> Result<(), String> {
        self.surface
            .update(&self.pixels, self.origin.left, self.origin.top)
    }

    fn redraw(&mut self) {
        let selection = match (self.start, self.current) {
            (Some(start), Some(current)) => Some(normalized_rect(start, current)),
            _ => None,
        };
        let border = self.border;
        for y in 0..self.height {
            for x in 0..self.width {
                let inside = selection
                    .map(|r| x >= r.left && x < r.right && y >= r.top && y < r.bottom)
                    .unwrap_or(false);
                let edge = inside
                    && selection
                        .map(|r| {
                            x - r.left < 2 || r.right - x <= 2 || y - r.top < 2 || r.bottom - y <= 2
                        })
                        .unwrap_or(false);
                let o = ((y * self.width + x) * 4) as usize;
                if edge {
                    self.pixels[o] = border[0];
                    self.pixels[o + 1] = border[1];
                    self.pixels[o + 2] = border[2];
                    self.pixels[o + 3] = 255;
                } else if inside {
                    self.pixels[o] = 0;
                    self.pixels[o + 1] = 0;
                    self.pixels[o + 2] = 0;
                    self.pixels[o + 3] = 0;
                } else {
                    self.pixels[o] = 0;
                    self.pixels[o + 1] = 0;
                    self.pixels[o + 2] = 0;
                    self.pixels[o + 3] = 112;
                }
            }
        }
    }
}

fn normalized_rect(a: POINT, b: POINT) -> RECT {
    RECT {
        left: a.x.min(b.x),
        top: a.y.min(b.y),
        right: a.x.max(b.x),
        bottom: a.y.max(b.y),
    }
}

fn linear_to_srgb(value: f32) -> u8 {
    let value = value.clamp(0.0, 1.0);
    let srgb = if value <= 0.0031308 {
        value * 12.92
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}
