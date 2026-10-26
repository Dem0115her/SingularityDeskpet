use std::sync::Once;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, RegisterClassW, SetWindowPos, ShowWindow,
    HMENU, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE, SW_SHOWNOACTIVATE, WNDCLASSW,
    WNDCLASS_STYLES, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

use crate::desktop_icons::{self, DesktopIcon, PatchPixels, SpritePixels};
use crate::log;
use crate::win::{self, LayeredSurface};

const PATCH_CLASS: &str = "SingularityIconPatchV1";
const HUNT_SPEED: f32 = 430.0;
const ABSORB_SECONDS: f32 = 1.05;
const TRAVEL_SECONDS: f32 = 0.92;
const EJECT_SECONDS: f32 = 1.15;
const EJECT_FLIGHT_SECONDS: f32 = 0.72;

static PATCH_CLASS_REGISTER: Once = Once::new();

pub struct FunContext {
    pub dt: f32,
    pub center: (f32, f32),
    pub pet_radius: f32,
    pub pet_hwnd: HWND,
    pub work: RECT,
    pub enabled: bool,
    pub ripples_allowed: bool,
    pub can_start: bool,
    pub interval_seconds: f32,
}

pub struct FunOutput {
    pub center: Option<(f32, f32)>,
    pub absorb: f32,
    pub ripple_age: f32,
    pub ripple_strength: f32,
    pub white: f32,
    pub notice: Option<&'static str>,
}

impl FunOutput {
    fn idle() -> Self {
        Self {
            center: None,
            absorb: 0.0,
            ripple_age: -1.0,
            ripple_strength: 0.0,
            white: 0.0,
            notice: None,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FunState {
    Idle,
    Hunting,
    Absorbing,
    Travel,
    Ejecting,
}

pub struct FunController {
    state: FunState,
    state_time: f32,
    idle_seconds: f32,
    interval_seconds: f32,
    ripple_timer: f32,
    rng: u32,
    target: Option<DesktopIcon>,
    patch_pixels: Option<PatchPixels>,
    patch: Option<PatchWindow>,
    sprite: Option<SpriteWindow>,
    hunt_origin: (f32, f32),
    travel_origin: (f32, f32),
    travel_target: (f32, f32),
    travel_control: (f32, f32),
    ripple_age: f32,
    ripple_strength: f32,
    absorb: f32,
    white: f32,
    demo_pending: bool,
    icon_moved: bool,
    eject_at: Option<(f32, f32)>,
    phase_sway: f32,
    notice: Option<&'static str>,
    notice_timer: f32,
    initialized: bool,
}

impl FunController {
    pub fn new() -> Self {
        Self {
            state: FunState::Idle,
            state_time: 0.0,
            idle_seconds: 90.0,
            interval_seconds: 90.0,
            ripple_timer: 5.0,
            rng: 0x6d2b_79f5,
            target: None,
            patch_pixels: None,
            patch: None,
            sprite: None,
            hunt_origin: (0.0, 0.0),
            travel_origin: (0.0, 0.0),
            travel_target: (0.0, 0.0),
            travel_control: (0.0, 0.0),
            ripple_age: -1.0,
            ripple_strength: 0.0,
            absorb: 0.0,
            white: 0.0,
            demo_pending: false,
            icon_moved: false,
            eject_at: None,
            phase_sway: 0.0,
            notice: None,
            notice_timer: 0.0,
            initialized: false,
        }
    }

    pub fn request_demo(&mut self) {
        if self.state != FunState::Idle {
            self.abort();
        }
        self.demo_pending = true;
        log::info("fun demo requested");
    }

    pub fn is_active(&self) -> bool {
        self.state != FunState::Idle
    }

    pub fn set_interval_seconds(&mut self, seconds: f32) {
        self.interval_seconds = seconds.clamp(30.0, 300.0);
        if self.state == FunState::Idle && !self.demo_pending {
            self.idle_seconds = self.interval_for_next();
        }
    }

    pub fn abort(&mut self) {
        let was_active = self.state != FunState::Idle || self.demo_pending;
        self.close_patch();
        self.close_sprite();
        self.target = None;
        self.patch_pixels = None;
        self.state = FunState::Idle;
        self.state_time = 0.0;
        self.absorb = 0.0;
        self.white = 0.0;
        self.icon_moved = false;
        self.eject_at = None;
        self.ripple_age = -1.0;
        self.ripple_strength = 0.0;
        if was_active {
            self.idle_seconds = self.interval_for_next();
        }
        self.demo_pending = false;
    }

    pub fn tick(&mut self, ctx: FunContext) -> FunOutput {
        let mut output = FunOutput::idle();
        self.interval_seconds = ctx.interval_seconds.clamp(30.0, 300.0);
        self.state_time += ctx.dt;
        if self.notice_timer > 0.0 {
            self.notice_timer -= ctx.dt;
            if self.notice_timer <= 0.0 {
                self.notice = None;
            }
        }
        self.update_ripple(ctx.dt);

        if self.state == FunState::Idle && ctx.ripples_allowed {
            self.ripple_timer -= ctx.dt;
            if self.ripple_timer <= 0.0 {
                self.emit_ripple(0.65);
                self.ripple_timer = 10.0 + self.rand01() * 14.0;
            }
        }

        if !ctx.enabled && !self.demo_pending && self.state == FunState::Idle {
            self.absorb = 0.0;
            self.white = 0.0;
            self.copy_fx(&mut output);
            return output;
        }

        if self.state == FunState::Idle {
            if ctx.enabled && !self.initialized {
                self.idle_seconds = self.interval_for_next();
                self.initialized = true;
            }
            self.idle_seconds -= ctx.dt;
            let should_start =
                ctx.can_start && (self.demo_pending || (ctx.enabled && self.idle_seconds <= 0.0));
            if should_start {
                if self.start_hunt(&ctx) {
                    self.demo_pending = false;
                } else {
                    self.demo_pending = false;
                    self.idle_seconds = self.interval_for_next();
                }
            }
            self.copy_fx(&mut output);
            return output;
        }

        if !ctx.can_start && self.state != FunState::Ejecting {
            self.abort();
            self.copy_fx(&mut output);
            return output;
        }

        match self.state {
            FunState::Hunting => {
                let Some(target) = self.target.as_ref() else {
                    self.abort();
                    return output;
                };
                let (tx, ty) = target.center();
                let dx = tx as f32 - ctx.center.0;
                let dy = ty as f32 - ctx.center.1;
                let dist = (dx * dx + dy * dy).sqrt();
                let max_step = HUNT_SPEED * ctx.dt;
                let step = dist.min(max_step);
                let nx = if dist > 0.001 { dx / dist } else { 0.0 };
                let ny = if dist > 0.001 { dy / dist } else { 0.0 };
                let sway = (self.state_time * 2.1 + self.phase_sway).sin() * 0.14;
                let (s, c) = sway.sin_cos();
                let mx = nx * c - ny * s;
                let my = nx * s + ny * c;
                let mut cx = ctx.center.0 + mx * step;
                let mut cy = ctx.center.1 + my * step;
                clamp_pet(&mut cx, &mut cy, ctx.pet_radius, ctx.work);
                output.center = Some((cx, cy));
                self.absorb = (1.0 - dist / (ctx.pet_radius * 3.2)).clamp(0.0, 1.0) * 0.22;
                self.phase_sway += ctx.dt * 1.7;
                let reach = reachable_distance(target, ctx.pet_radius, ctx.work);
                let absorb_distance = (reach + ctx.pet_radius * 1.55).min(ctx.pet_radius * 2.4);
                let rect_distance = rect_distance(cx, cy, &target.screen);
                if rect_distance <= absorb_distance {
                    self.state = FunState::Absorbing;
                    self.state_time = 0.0;
                    self.hunt_origin = (cx, cy);
                    log::info(&format!(
                        "fun: absorbing icon '{}' at ({tx},{ty}) from ({cx:.0},{cy:.0}) rect_d={rect_distance:.1} limit={absorb_distance:.1}",
                        target.name
                    ));
                }
            }
            FunState::Absorbing => {
                let t = (self.state_time / ABSORB_SECONDS).clamp(0.0, 1.0);
                self.absorb = smoothstep(0.0, 0.72, t);
                if t > 0.06 && self.patch.is_none() {
                    if let Some(pixels) = self.patch_pixels.as_ref() {
                        match PatchWindow::new(pixels, ctx.pet_hwnd) {
                            Ok(patch) => self.patch = Some(patch),
                            Err(error) => {
                                log::warn(&format!("fun patch failed: {error}"));
                                self.notice = Some("图标遮罩创建失败");
                                self.notice_timer = 4.0;
                                self.abort();
                                self.copy_fx(&mut output);
                                return output;
                            }
                        }
                    }
                    let sprite = self
                        .patch_pixels
                        .as_ref()
                        .and_then(|pixels| pixels.sprite.as_ref())
                        .map(|sprite| SpritePixels {
                            width: sprite.width,
                            height: sprite.height,
                            pixels: sprite.pixels.clone(),
                        });
                    if self.sprite.is_none() {
                        if let Some(sprite) = sprite {
                            match SpriteWindow::new(&sprite, ctx.pet_hwnd) {
                                Ok(window) => self.sprite = Some(window),
                                Err(error) => {
                                    log::warn(&format!("fun sprite failed: {error}"));
                                }
                            }
                        }
                    }
                }
                if t > 0.06 {
                    let u = ((t - 0.06) / 0.55).clamp(0.0, 1.0);
                    let e = smoothstep(0.0, 1.0, u);
                    if let Some(sprite) = self.sprite.as_mut() {
                        let start = self
                            .target
                            .as_ref()
                            .map(|target| {
                                let (x, y) = target.center();
                                (x as f32, y as f32)
                            })
                            .unwrap_or(self.hunt_origin);
                        let end = self.hunt_origin;
                        let center_x = start.0 + (end.0 - start.0) * e;
                        let center_y = start.1 + (end.1 - start.1) * e;
                        sprite.update(center_x, center_y, 1.0 - 0.94 * e, 1.0 - e);
                    }
                    if u >= 1.0 {
                        self.close_sprite();
                    }
                }
                if t > 0.58 {
                    self.emit_ripple(0.7);
                }
                if t >= 1.0 {
                    self.close_sprite();
                    self.plan_travel(&ctx);
                    self.state = FunState::Travel;
                    self.state_time = 0.0;
                    self.emit_ripple(1.0);
                    log::info(&format!(
                        "fun: travel to ({:.0},{:.0})",
                        self.travel_target.0, self.travel_target.1
                    ));
                }
            }
            FunState::Travel => {
                let t = (self.state_time / TRAVEL_SECONDS).clamp(0.0, 1.0);
                let e = smoothstep(0.0, 1.0, t);
                let x = quadratic(
                    self.travel_origin.0,
                    self.travel_control.0,
                    self.travel_target.0,
                    e,
                );
                let y = quadratic(
                    self.travel_origin.1,
                    self.travel_control.1,
                    self.travel_target.1,
                    e,
                );
                output.center = Some((x, y));
                self.absorb = (1.0 - t * 1.35).max(0.0);
                if t >= 1.0 {
                    self.state = FunState::Ejecting;
                    self.state_time = 0.0;
                    self.eject_at =
                        self.choose_eject_position(ctx.work, ctx.pet_radius, self.travel_target);
                    self.emit_ripple(1.0);
                }
            }
            FunState::Ejecting => {
                let t = (self.state_time / EJECT_SECONDS).clamp(0.0, 1.0);
                self.white = (t * std::f32::consts::PI).sin().max(0.0);
                if self.sprite.is_none() {
                    let sprite = self
                        .patch_pixels
                        .as_ref()
                        .and_then(|pixels| pixels.sprite.as_ref())
                        .map(|sprite| SpritePixels {
                            width: sprite.width,
                            height: sprite.height,
                            pixels: sprite.pixels.clone(),
                        });
                    if let Some(sprite) = sprite {
                        match SpriteWindow::new(&sprite, ctx.pet_hwnd) {
                            Ok(window) => self.sprite = Some(window),
                            Err(error) => {
                                log::warn(&format!("fun eject sprite failed: {error}"));
                            }
                        }
                    }
                }
                let flight = (self.state_time / EJECT_FLIGHT_SECONDS).clamp(0.0, 1.0);
                let e = smoothstep(0.0, 1.0, flight);
                if let (Some(sprite), Some((x, y)), Some(target)) =
                    (self.sprite.as_mut(), self.eject_at, self.target.as_ref())
                {
                    let width = (target.screen.right - target.screen.left) as f32;
                    let height = (target.screen.bottom - target.screen.top) as f32;
                    let destination_x = x + width * 0.5;
                    let destination_y = y + height * 0.5;
                    let start = self.travel_target;
                    let center_x = start.0 + (destination_x - start.0) * e;
                    let center_y = start.1 + (destination_y - start.1) * e;
                    sprite.update(center_x, center_y, 0.06 + 0.94 * e, e);
                }
                if self.state_time > EJECT_FLIGHT_SECONDS + 0.01 && !self.icon_moved {
                    if let (Some(target), Some((x, y))) = (self.target.as_ref(), self.eject_at) {
                        if !desktop_icons::move_icon(target, x as i32, y as i32) {
                            log::warn("fun: icon move failed; restoring original layout");
                        }
                    }
                    self.icon_moved = true;
                    self.close_patch();
                    self.close_sprite();
                }
                if t > 0.85 {
                    self.emit_ripple(0.55);
                }
                if t >= 1.0 {
                    self.close_patch();
                    self.close_sprite();
                    self.white = 0.0;
                    self.absorb = 0.0;
                    self.target = None;
                    self.patch_pixels = None;
                    self.icon_moved = false;
                    self.eject_at = None;
                    self.state = FunState::Idle;
                    self.state_time = 0.0;
                    self.idle_seconds = self.interval_for_next();
                    log::info("fun: interaction complete");
                }
            }
            FunState::Idle => {}
        }
        self.copy_fx(&mut output);
        output
    }

    fn start_hunt(&mut self, ctx: &FunContext) -> bool {
        let icons = desktop_icons::list_icons(ctx.work);
        if icons.is_empty() {
            log::info("fun: no visible desktop icon available");
            self.notice = Some("没有可见的桌面图标");
            self.notice_timer = 4.0;
            return false;
        }
        let mut candidates: Vec<(f32, DesktopIcon)> = icons
            .into_iter()
            .filter_map(|icon| {
                let (x, y) = icon.center();
                let dx = x as f32 - ctx.center.0;
                let dy = y as f32 - ctx.center.1;
                let distance = (dx * dx + dy * dy).sqrt();
                if distance < ctx.pet_radius * 2.6 {
                    return None;
                }
                if !desktop_icons::is_desktop_point(x, y) {
                    return None;
                }
                Some((distance, icon))
            })
            .collect();
        if candidates.is_empty() {
            self.notice = Some("图标都被窗口遮住了");
            self.notice_timer = 4.0;
            return false;
        }
        candidates.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap_or(std::cmp::Ordering::Equal));
        let pick = self.rand01().mul_add(3.0, 0.0) as usize;
        let pick = pick.min(candidates.len() - 1);
        let icon = candidates.remove(pick).1;
        let Some(pixels) = desktop_icons::capture_patch(&icon, ctx.work, 20) else {
            log::warn("fun: could not capture icon patch");
            self.notice = Some("无法捕获图标背景");
            self.notice_timer = 4.0;
            return false;
        };
        log::info(&format!(
            "fun: hunting icon '{}' at {:?}",
            icon.name, icon.screen
        ));
        self.target = Some(icon);
        self.patch_pixels = Some(pixels);
        self.state = FunState::Hunting;
        self.state_time = 0.0;
        self.hunt_origin = ctx.center;
        self.phase_sway = self.rand01() * std::f32::consts::TAU;
        self.absorb = 0.0;
        true
    }

    fn plan_travel(&mut self, ctx: &FunContext) {
        let origin = self.hunt_origin;
        let work = ctx.work;
        let margin = ctx.pet_radius + 24.0;
        let min_x = work.left as f32 + margin;
        let max_x = work.right as f32 - margin;
        let min_y = work.top as f32 + margin;
        let max_y = work.bottom as f32 - margin;
        let mut best = (origin.0, origin.1);
        for _ in 0..24 {
            let x = min_x + self.rand01() * (max_x - min_x).max(1.0);
            let y = min_y + self.rand01() * (max_y - min_y).max(1.0);
            let dx = x - origin.0;
            let dy = y - origin.1;
            let distance = (dx * dx + dy * dy).sqrt();
            if distance > ctx.pet_radius * 3.6 {
                best = (x, y);
                break;
            }
        }
        self.travel_origin = origin;
        self.travel_target = best;
        let mx = (origin.0 + best.0) * 0.5;
        let my = (origin.1 + best.1) * 0.5;
        let dx = best.0 - origin.0;
        let dy = best.1 - origin.1;
        let dist = (dx * dx + dy * dy).sqrt().max(1.0);
        let bend = ctx.pet_radius * (1.2 + self.rand01() * 1.4);
        let side = if self.rand01() > 0.5 { 1.0 } else { -1.0 };
        self.travel_control = (mx - dy / dist * bend * side, my + dx / dist * bend * side);
        self.absorb = 1.0;
    }

    fn choose_eject_position(
        &mut self,
        work: RECT,
        pet_radius: f32,
        pet: (f32, f32),
    ) -> Option<(f32, f32)> {
        let target = self.target.as_ref()?;
        let width = (target.screen.right - target.screen.left) as f32;
        let height = (target.screen.bottom - target.screen.top) as f32;
        let margin = 14.0;
        let min_radius = eject_min_radius(pet_radius, width, height);
        for _ in 0..40 {
            let angle = self.rand01() * std::f32::consts::TAU;
            let radius = min_radius + self.rand01() * pet_radius * 0.9;
            let cx = pet.0 + angle.cos() * radius;
            let cy = pet.1 + angle.sin() * radius;
            let left = cx - width * 0.5;
            let top = cy - height * 0.5;
            if left < work.left as f32 + margin
                || top < work.top as f32 + margin
                || left + width > work.right as f32 - margin
                || top + height > work.bottom as f32 - margin
            {
                continue;
            }
            let probe_x = (left + width * 0.5) as i32;
            let probe_y = (top + height * 0.5) as i32;
            if desktop_icons::is_desktop_point(probe_x, probe_y) {
                return Some((left, top));
            }
        }
        None
    }

    fn emit_ripple(&mut self, strength: f32) {
        self.ripple_age = 0.0;
        self.ripple_strength = strength;
    }

    fn update_ripple(&mut self, dt: f32) {
        if self.ripple_age < 0.0 {
            return;
        }
        self.ripple_age += dt;
        self.ripple_strength *= (-dt * 1.25).exp();
        if self.ripple_age > 2.6 || self.ripple_strength < 0.01 {
            self.ripple_age = -1.0;
            self.ripple_strength = 0.0;
        }
    }

    fn copy_fx(&self, output: &mut FunOutput) {
        output.absorb = self.absorb;
        output.ripple_age = self.ripple_age;
        output.ripple_strength = self.ripple_strength;
        output.white = self.white;
        output.notice = if self.notice_timer > 0.0 {
            self.notice
        } else {
            None
        };
    }

    fn close_patch(&mut self) {
        if let Some(patch) = self.patch.take() {
            patch.destroy();
        }
    }

    fn close_sprite(&mut self) {
        if let Some(sprite) = self.sprite.take() {
            sprite.destroy();
        }
    }

    fn interval_for_next(&mut self) -> f32 {
        let spread = 0.82 + self.rand01() * 0.36;
        self.interval_seconds * spread
    }

    fn rand01(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x & 0x00ff_ffff) as f32 / 16_777_215.0
    }
}

impl Drop for FunController {
    fn drop(&mut self) {
        self.close_patch();
        self.close_sprite();
    }
}

struct PatchWindow {
    hwnd: HWND,
    _surface: LayeredSurface,
}

impl PatchWindow {
    fn new(pixels: &PatchPixels, pet: HWND) -> Result<Self, String> {
        let instance = unsafe {
            GetModuleHandleW(None).map_err(|error| format!("GetModuleHandleW: {error}"))?
        };
        register_patch_class(instance.into());
        let class = win::wide(PATCH_CLASS);
        let title = win::wide("Singularity patch");
        let width = pixels.width as i32;
        let height = pixels.height as i32;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOACTIVATE,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                pixels.rect.left,
                pixels.rect.top,
                width,
                height,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )
            .map_err(|error| format!("CreateWindowExW(patch): {error}"))?
        };
        let surface = LayeredSurface::new(hwnd, width, height)?;
        surface.update(&pixels.pixels, pixels.rect.left, pixels.rect.top)?;
        unsafe {
            let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                hwnd,
                pet,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        log::info(&format!(
            "fun: patch shown at ({}, {}) {}x{}",
            pixels.rect.left, pixels.rect.top, width, height
        ));
        Ok(Self {
            hwnd,
            _surface: surface,
        })
    }

    fn destroy(self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

struct SpriteWindow {
    hwnd: HWND,
    surface: LayeredSurface,
    source: SpritePixels,
    pixels: Vec<u8>,
    width: u32,
    height: u32,
    pet: HWND,
    visible: bool,
}

impl SpriteWindow {
    fn new(sprite: &SpritePixels, pet: HWND) -> Result<Self, String> {
        let instance = unsafe {
            GetModuleHandleW(None).map_err(|error| format!("GetModuleHandleW: {error}"))?
        };
        register_patch_class(instance.into());
        let class = win::wide(PATCH_CLASS);
        let title = win::wide("Singularity icon sprite");
        let width = sprite.width.max(1) as i32;
        let height = sprite.height.max(1) as i32;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_TOPMOST
                    | WS_EX_NOACTIVATE
                    | WS_EX_TRANSPARENT,
                PCWSTR(class.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                width,
                height,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )
            .map_err(|error| format!("CreateWindowExW(sprite): {error}"))?
        };
        let surface = LayeredSurface::new(hwnd, width, height)?;
        let pixels = vec![0u8; (sprite.width * sprite.height * 4) as usize];
        Ok(Self {
            hwnd,
            surface,
            source: SpritePixels {
                width: sprite.width,
                height: sprite.height,
                pixels: sprite.pixels.clone(),
            },
            pixels,
            width: sprite.width,
            height: sprite.height,
            pet,
            visible: false,
        })
    }

    fn update(&mut self, center_x: f32, center_y: f32, scale: f32, alpha: f32) {
        let width = self.width as usize;
        let height = self.height as usize;
        let source_width = self.source.width as usize;
        let source_height = self.source.height as usize;
        let scale = scale.clamp(0.01, 2.0);
        let alpha = alpha.clamp(0.0, 1.0);
        self.pixels.fill(0);
        let visible_w = (source_width as f32 * scale).round().max(1.0) as usize;
        let visible_h = (source_height as f32 * scale).round().max(1.0) as usize;
        let x0 = width.saturating_sub(visible_w) / 2;
        let y0 = height.saturating_sub(visible_h) / 2;
        for y in 0..visible_h {
            for x in 0..visible_w {
                let sx = (x as f32 / scale).floor() as usize;
                let sy = (y as f32 / scale).floor() as usize;
                if sx >= source_width || sy >= source_height {
                    continue;
                }
                let source = (sy * source_width + sx) * 4;
                let target = ((y0 + y) * width + (x0 + x)) * 4;
                self.pixels[target] = (self.source.pixels[source] as f32 * alpha) as u8;
                self.pixels[target + 1] = (self.source.pixels[source + 1] as f32 * alpha) as u8;
                self.pixels[target + 2] = (self.source.pixels[source + 2] as f32 * alpha) as u8;
                self.pixels[target + 3] = (self.source.pixels[source + 3] as f32 * alpha) as u8;
            }
        }
        let x = (center_x - self.width as f32 * 0.5).round() as i32;
        let y = (center_y - self.height as f32 * 0.5).round() as i32;
        if let Err(error) = self.surface.update(&self.pixels, x, y) {
            log::warn(&format!("fun sprite update failed: {error}"));
        }
        if !self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    self.hwnd,
                    self.pet,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            self.visible = true;
        }
    }

    fn destroy(self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn eject_min_radius(pet_radius: f32, width: f32, height: f32) -> f32 {
    pet_radius * 3.05 + width.max(height) * 0.55 + 10.0
}

fn register_patch_class(instance: HINSTANCE) {
    PATCH_CLASS_REGISTER.call_once(|| unsafe {
        let class = win::wide(PATCH_CLASS);
        let wc = WNDCLASSW {
            style: WNDCLASS_STYLES(0),
            lpfnWndProc: Some(patch_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance,
            hIcon: Default::default(),
            hCursor: Default::default(),
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(class.as_ptr()),
        };
        let _ = RegisterClassW(&wc);
    });
}

unsafe extern "system" fn patch_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

fn rect_distance(px: f32, py: f32, rect: &RECT) -> f32 {
    let nearest_x = px.clamp(rect.left as f32, rect.right as f32);
    let nearest_y = py.clamp(rect.top as f32, rect.bottom as f32);
    let dx = px - nearest_x;
    let dy = py - nearest_y;
    (dx * dx + dy * dy).sqrt()
}

fn reachable_distance(icon: &DesktopIcon, radius: f32, work: RECT) -> f32 {
    let half = radius * 3.0 + 4.0;
    let min_x = work.left as f32 + half;
    let max_x = work.right as f32 - half;
    let min_y = work.top as f32 + half;
    let max_y = work.bottom as f32 - half;
    let (tx, ty) = icon.center();
    let cx = (tx as f32).clamp(min_x, max_x);
    let cy = (ty as f32).clamp(min_y, max_y);
    rect_distance(cx, cy, &icon.screen)
}

fn clamp_pet(cx: &mut f32, cy: &mut f32, radius: f32, work: RECT) {
    let half = radius * 3.0 + 4.0;
    let min_x = work.left as f32 + half;
    let max_x = work.right as f32 - half;
    let min_y = work.top as f32 + half;
    let max_y = work.bottom as f32 - half;
    if max_x > min_x {
        *cx = cx.clamp(min_x, max_x);
    }
    if max_y > min_y {
        *cy = cy.clamp(min_y, max_y);
    }
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(0.0001)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn quadratic(a: f32, b: f32, c: f32, t: f32) -> f32 {
    let u = 1.0 - t;
    u * u * a + 2.0 * u * t * b + t * t * c
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows::Win32::Foundation::{HWND, RECT};

    fn context(center: (f32, f32), enabled: bool, can_start: bool) -> FunContext {
        FunContext {
            dt: 0.016,
            center,
            pet_radius: 52.0,
            pet_hwnd: HWND::default(),
            work: RECT {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1032,
            },
            enabled,
            ripples_allowed: true,
            can_start,
            interval_seconds: 90.0,
        }
    }

    #[test]
    fn idle_abort_keeps_auto_timer() {
        let mut controller = FunController::new();
        controller.idle_seconds = 12.0;
        controller.abort();
        assert!((controller.idle_seconds - 12.0).abs() < 0.01);
    }

    #[test]
    fn interval_setting_reschedules_when_idle() {
        let mut controller = FunController::new();
        controller.set_interval_seconds(30.0);
        assert!(controller.idle_seconds >= 30.0 * 0.82 - 0.01);
        assert!(controller.idle_seconds <= 30.0 * 1.18 + 0.01);
    }

    #[test]
    fn eject_radius_clears_pet_window() {
        let pet_radius = 52.0;
        let icon_size = 56.0;
        let min_radius = eject_min_radius(pet_radius, icon_size, icon_size);
        assert!(min_radius - icon_size * 0.5 > pet_radius * 3.0);
    }

    #[test]
    fn hunting_reaches_icon_near_top_edge() {
        let mut controller = FunController::new();
        controller.state = FunState::Hunting;
        controller.target = Some(DesktopIcon {
            index: 0,
            name: "Steam".to_owned(),
            screen: RECT {
                left: 1064,
                top: 105,
                right: 1140,
                bottom: 177,
            },
        });
        let mut center = (963.0, 417.0);
        for _ in 0..200 {
            let output = controller.tick(context(center, true, true));
            if let Some((x, y)) = output.center {
                center = (x, y);
            }
            if matches!(controller.state, FunState::Absorbing) {
                break;
            }
        }
        assert!(
            matches!(controller.state, FunState::Absorbing),
            "hunt got stuck at {center:?}"
        );
        assert!(
            controller.hunt_origin.1 >= 160.0,
            "pet should start travel from the clamped position"
        );
    }

    #[test]
    fn completes_full_cycle_without_moving_real_icon() {
        let mut controller = FunController::new();
        controller.state = FunState::Hunting;
        controller.target = Some(DesktopIcon {
            index: 0,
            name: "Fake".to_owned(),
            screen: RECT {
                left: 1064,
                top: 105,
                right: 1140,
                bottom: 177,
            },
        });
        let mut center = (963.0, 417.0);
        let mut saw_absorb = false;
        let mut saw_travel = false;
        let mut saw_eject = false;
        for _ in 0..600 {
            let output = controller.tick(context(center, true, true));
            if let Some((x, y)) = output.center {
                center = (x, y);
            }
            match controller.state {
                FunState::Absorbing => saw_absorb = true,
                FunState::Travel => saw_travel = true,
                FunState::Ejecting => {
                    saw_eject = true;
                    controller.target = None;
                }
                FunState::Idle if saw_eject => break,
                _ => {}
            }
        }
        assert!(saw_absorb && saw_travel && saw_eject);
        assert!(matches!(controller.state, FunState::Idle));
    }

    #[test]
    fn idle_ripple_works_when_fun_disabled() {
        let mut controller = FunController::new();
        controller.ripple_timer = 0.0;
        let output = controller.tick(context((900.0, 500.0), false, false));
        assert!(output.ripple_age >= 0.0);
        assert!(output.ripple_strength > 0.5);
    }
}
