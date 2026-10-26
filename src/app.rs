use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::HBRUSH;
use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::SystemInformation::GetLocalTime;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    keybd_event, GetAsyncKeyState, GetDoubleClickTime, RegisterHotKey, ReleaseCapture, SetCapture,
    SetFocus, UnregisterHotKey, KEYBD_EVENT_FLAGS, KEYEVENTF_KEYUP, MOD_ALT, MOD_CONTROL,
    VK_ESCAPE, VK_LBUTTON, VK_MEDIA_NEXT_TRACK, VK_MEDIA_PLAY_PAUSE, VK_MEDIA_PREV_TRACK,
    VK_VOLUME_MUTE,
};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_INFO, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIIF_INFO, NIM_ADD,
    NIM_DELETE, NIM_MODIFY, NIN_BALLOONUSERCLICK, NOTIFYICONDATAW, NOTIFYICONDATAW_0,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::fun::{FunContext, FunController};
use crate::log;
use crate::panel::{
    PanelActions, SettingsPanel, PANEL_GAP_DIP, PANEL_HEIGHT_DIP, PANEL_MARGIN_DIP, PANEL_WIDTH_DIP,
};
use crate::region::{RegionResponse, RegionSelector};
use crate::render::{FrameParams, Renderer, WINDOW_SCALE};
use crate::settings::{self, Note, Reminder, Settings, Todo};
use crate::snapshot;
use crate::stats::{
    StatsAction, StatsOverlay, HEIGHT_DIP as STATS_HEIGHT_DIP, WIDTH_DIP as STATS_WIDTH_DIP,
};
use crate::sysmon::SystemMonitor;
use crate::win::{self, LayeredSurface};

const WINDOW_CLASS: &str = "SingularityPetWindowV1";
const PANEL_CLASS: &str = "SingularitySettingsPanelV1";
const STATS_CLASS: &str = "SingularityStatsWindowV1";
const REGION_CLASS: &str = "SingularityRegionSelectorV1";
const WINDOW_TITLE: &str = "奇点 · 黑洞桌宠";
const WM_TRAY: u32 = WM_APP + 1;
const HOTKEY_ID: i32 = 0x5151;
const TRAY_ID: u32 = 1;

pub fn run() -> Result<(), String> {
    if !win::single_instance("Global\\SingularityBlackHolePet") {
        log::warn("another instance is already running");
        return Ok(());
    }

    let mut app = Box::new(App::create()?);
    let hwnd = app.hwnd;
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, &mut *app as *mut App as isize);
    }
    app.after_created();

    let mut next_frame = Instant::now();
    loop {
        let mut msg = MSG::default();
        while unsafe { PeekMessageW(&mut msg, HWND::default(), 0, 0, PM_REMOVE) }.as_bool() {
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
            if app.quit {
                break;
            }
        }
        if app.quit {
            break;
        }

        let now = Instant::now();
        if now >= next_frame {
            app.tick();
            if app.quit {
                break;
            }
            next_frame = now + app.frame_interval();
        }

        let wait = next_frame.saturating_duration_since(Instant::now());
        let wait_ms = wait.as_millis().min(200) as u32;
        unsafe {
            MsgWaitForMultipleObjectsEx(None, wait_ms, QS_ALLINPUT, MWMO_INPUTAVAILABLE);
        }
    }

    app.shutdown();
    unsafe {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        let _ = DestroyWindow(hwnd);
    }
    Ok(())
}

struct App {
    hwnd: HWND,
    settings: Settings,
    renderer: Renderer,
    surface: Option<LayeredSurface>,
    tray_icon: Option<HICON>,
    tray_added: bool,
    hand_cursor: HCURSOR,
    width: i32,
    height: i32,
    dpi: f32,
    center_x: f32,
    center_y: f32,
    clock: f64,
    visual_time: f32,
    fade: f32,
    exiting: bool,
    quit: bool,
    shown: bool,
    user_hidden: bool,
    fullscreen_hidden: bool,
    click_through: bool,
    dragging: bool,
    drag_offset_x: f32,
    drag_offset_y: f32,
    drag_start: POINT,
    drag_started: Instant,
    desktop_lens: bool,
    last_click: Option<(Instant, POINT)>,
    hover_x: f32,
    hover_y: f32,
    hover_weight: f32,
    pulse: f32,
    pulse_age: f32,
    wander_target: Option<(f32, f32)>,
    wander_pause_until: f64,
    wander_phase: f32,
    rng: u32,
    last_fullscreen_check: Instant,
    last_save: Instant,
    last_center_saved: (i32, i32),
    last_frame: Instant,
    last_render_error: Option<Instant>,
    dump_frame: Option<PathBuf>,
    frame_count: u32,
    dump_done: bool,
    panel: Option<SettingsPanel>,
    region_select: Option<RegionSelector>,
    panel_guard_until: f64,
    open_panel_reminders: bool,
    last_left_button: bool,
    last_radius_resize: Instant,
    intro_pending: bool,
    fun: FunController,
    fun_absorb: f32,
    fun_ripple_age: f32,
    fun_ripple_strength: f32,
    fun_white: f32,
    last_fun_notice: Option<&'static str>,
    stats_linger_until: f64,
    sysmon: SystemMonitor,
    stats: Option<StatsOverlay>,
}

impl App {
    fn create() -> Result<Self, String> {
        let settings = settings::load();
        let instance =
            unsafe { GetModuleHandleW(None).map_err(|e| format!("GetModuleHandleW failed: {e}"))? };
        let class_name = win::wide(WINDOW_CLASS);
        let title = win::wide(WINDOW_TITLE);
        let cursor = unsafe { LoadCursorW(HINSTANCE::default(), IDC_ARROW).unwrap_or_default() };
        let wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(class_name.as_ptr()),
        };
        let atom = unsafe { RegisterClassW(&wc) };
        if atom == 0 {
            let error = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(format!("RegisterClassW failed: {error:?}"));
        }

        let panel_class_name = win::wide(PANEL_CLASS);
        let panel_wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(panel_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(panel_class_name.as_ptr()),
        };
        let panel_atom = unsafe { RegisterClassW(&panel_wc) };
        if panel_atom == 0 {
            let error = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(format!("RegisterClassW(panel) failed: {error:?}"));
        }

        let region_class_name = win::wide(REGION_CLASS);
        let region_wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(region_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(region_class_name.as_ptr()),
        };
        let region_atom = unsafe { RegisterClassW(&region_wc) };
        if region_atom == 0 {
            let error = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(format!("RegisterClassW(region) failed: {error:?}"));
        }

        let stats_class_name = win::wide(STATS_CLASS);
        let stats_wc = WNDCLASSW {
            style: CS_DBLCLKS,
            lpfnWndProc: Some(stats_wndproc),
            cbClsExtra: 0,
            cbWndExtra: 0,
            hInstance: instance.into(),
            hIcon: HICON::default(),
            hCursor: cursor,
            hbrBackground: HBRUSH::default(),
            lpszMenuName: PCWSTR::null(),
            lpszClassName: PCWSTR(stats_class_name.as_ptr()),
        };
        let stats_atom = unsafe { RegisterClassW(&stats_wc) };
        if stats_atom == 0 {
            let error = unsafe { windows::Win32::Foundation::GetLastError() };
            return Err(format!("RegisterClassW(stats) failed: {error:?}"));
        }

        let initial_side = (settings.radius_dip * WINDOW_SCALE).round() as i32;
        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_LAYERED
                    | WS_EX_TOOLWINDOW
                    | WS_EX_NOACTIVATE
                    | WS_EX_TOPMOST
                    | WS_EX_TRANSPARENT,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                initial_side,
                initial_side,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )
            .map_err(|e| format!("CreateWindowExW failed: {e}"))?
        };

        if let (Some(x), Some(y)) = (settings.x, settings.y) {
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    HWND::default(),
                    x - initial_side / 2,
                    y - initial_side / 2,
                    0,
                    0,
                    SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
        }

        win::exclude_from_capture(hwnd);

        let dpi = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
        let dpi = if dpi <= 0.0 { 1.0 } else { dpi };
        let width = (settings.radius_dip * dpi * WINDOW_SCALE).round() as i32;
        let height = width;
        let work = win::monitor_work_area(hwnd);
        let mut center_x = settings
            .x
            .map(|v| v as f32)
            .unwrap_or_else(|| work.left as f32 + (work.right - work.left) as f32 * 0.65);
        let mut center_y = settings
            .y
            .map(|v| v as f32)
            .unwrap_or_else(|| work.top as f32 + (work.bottom - work.top) as f32 * 0.56);
        clamp_center(&mut center_x, &mut center_y, width, height, work);
        let mut settings = settings;
        settings.x = Some(center_x.round() as i32);
        settings.y = Some(center_y.round() as i32);

        let renderer = Renderer::new(width as u32, height as u32)?;
        let surface = LayeredSurface::new(hwnd, width, height)?;
        let hand_cursor =
            unsafe { LoadCursorW(HINSTANCE::default(), IDC_HAND).unwrap_or_default() };

        log::info(&format!(
            "pet created hwnd={hwnd:?} dpi={dpi:.2} side={width} center=({center_x:.0},{center_y:.0}) work={:?}",
            work
        ));
        let intro_pending = !settings.intro_seen;
        let saved_center = (settings.x.unwrap_or(0), settings.y.unwrap_or(0));
        let dump_frame = std::env::var_os("SINGULARITY_DUMP_FRAME").map(PathBuf::from);
        if let Some(path) = &dump_frame {
            log::info(&format!("frame dump enabled: {}", path.display()));
        }
        let desktop_lens = std::env::var_os("SINGULARITY_ALLOW_CAPTURE").is_none();
        if !desktop_lens {
            log::warn("SINGULARITY_ALLOW_CAPTURE=1: desktop lens disabled to avoid self-capture");
        }

        Ok(Self {
            hwnd,
            settings,
            renderer,
            surface: Some(surface),
            tray_icon: None,
            tray_added: false,
            hand_cursor,
            width,
            height,
            dpi,
            center_x,
            center_y,
            clock: 0.0,
            visual_time: 0.0,
            fade: 0.0,
            exiting: false,
            quit: false,
            shown: false,
            user_hidden: false,
            fullscreen_hidden: false,
            click_through: true,
            dragging: false,
            drag_offset_x: 0.0,
            drag_offset_y: 0.0,
            drag_start: POINT::default(),
            drag_started: Instant::now(),
            desktop_lens,
            last_click: None,
            hover_x: 0.0,
            hover_y: 0.0,
            hover_weight: 0.0,
            pulse: 0.0,
            pulse_age: 0.0,
            wander_target: None,
            wander_pause_until: 0.0,
            wander_phase: 1.7,
            rng: 0x9e37_79b9,
            last_fullscreen_check: Instant::now(),
            last_save: Instant::now(),
            last_center_saved: saved_center,
            last_frame: Instant::now(),
            last_render_error: None,
            dump_frame,
            frame_count: 0,
            dump_done: false,
            panel: None,
            region_select: None,
            panel_guard_until: 0.0,
            open_panel_reminders: false,
            last_left_button: false,
            last_radius_resize: Instant::now(),
            intro_pending,
            fun: FunController::new(),
            fun_absorb: 0.0,
            fun_ripple_age: -1.0,
            fun_ripple_strength: 0.0,
            fun_white: 0.0,
            last_fun_notice: None,
            stats_linger_until: 0.0,
            sysmon: SystemMonitor::new(),
            stats: None,
        })
    }

    fn after_created(&mut self) {
        unsafe {
            let _ = RegisterHotKey(self.hwnd, HOTKEY_ID, MOD_CONTROL | MOD_ALT, 'B' as u32);
            let _ = SetWindowPos(
                self.hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        self.add_tray_icon();
        win::set_click_through(self.hwnd, true);
        let _ = self.ensure_panel();
        let _ = self.ensure_stats();
        let settings = self.settings.clone();
        if let Some(panel) = self.panel.as_mut() {
            panel.prewarm(&settings, self.clock);
        }
        if let Some(stats) = self.stats.as_mut() {
            stats.prewarm(&settings, self.clock, self.hwnd);
        }
    }

    fn add_tray_icon(&mut self) {
        let icon = match win::load_app_icon() {
            Some(icon) => icon,
            None => {
                log::warn("tray icon unavailable");
                return;
            }
        };
        self.tray_icon = Some(icon);
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP | NIF_SHOWTIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        write_wide(&mut data.szTip, "奇点 · 黑洞桌宠\n点击打开设置");
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &data) };
        if ok.as_bool() {
            self.tray_added = true;
            log::info("tray icon added");
            if self.intro_pending {
                let mut info = data;
                info.uFlags = NIF_INFO;
                info.Anonymous = NOTIFYICONDATAW_0 { uTimeout: 7000 };
                info.dwInfoFlags = NIIF_INFO;
                write_wide(&mut info.szInfoTitle, "奇点已就位");
                write_wide(
                    &mut info.szInfo,
                    "左键单击核心打开设置 · 拖动移动 · Ctrl+Alt+B 显隐",
                );
                let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &info) };
                self.intro_pending = false;
                self.settings.intro_seen = true;
                self.save_settings();
            }
        } else {
            log::warn("Shell_NotifyIconW(NIM_ADD) failed");
        }
    }

    fn remove_tray_icon(&mut self) {
        if self.tray_added {
            let data = NOTIFYICONDATAW {
                cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
                hWnd: self.hwnd,
                uID: TRAY_ID,
                ..Default::default()
            };
            unsafe {
                let _ = Shell_NotifyIconW(NIM_DELETE, &data);
            }
            self.tray_added = false;
        }
        if let Some(icon) = self.tray_icon.take() {
            win::destroy_icon(icon);
        }
    }

    fn frame_interval(&self) -> Duration {
        let active = self.dragging
            || self.pulse > 0.01
            || self.hover_weight > 0.01
            || !self.shown
            || self.fade < 0.999
            || self.exiting
            || self.fun.is_active()
            || self.fun_ripple_strength > 0.01
            || self.stats.as_ref().is_some_and(|stats| stats.is_active())
            || self.panel.as_ref().is_some_and(|panel| panel.is_active());
        if self.user_hidden || self.fullscreen_hidden {
            Duration::from_millis(200)
        } else if active {
            Duration::from_millis(16)
        } else {
            Duration::from_millis(33)
        }
    }

    fn visible(&self) -> bool {
        !self.user_hidden && !self.fullscreen_hidden
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now
            .saturating_duration_since(self.last_frame)
            .as_secs_f32()
            .clamp(0.0, 0.10);
        self.last_frame = now;
        self.clock += dt as f64;
        self.check_reminders();
        if self
            .settings
            .focus_until_unix
            .is_some_and(|until| until <= unix_now())
        {
            self.settings.focus_until_unix = None;
            self.settings.focus_reminder_id = None;
            self.save_settings();
        }
        if self.exiting {
            self.fade = (self.fade - dt / 0.38).max(0.0);
            if self.fade <= 0.001 {
                self.quit = true;
                return;
            }
        } else if self.fade < 1.0 {
            self.fade = (self.fade + dt / 0.85).min(1.0);
        }

        if self.settings.fullscreen_hide
            && now.duration_since(self.last_fullscreen_check) >= Duration::from_millis(900)
        {
            self.last_fullscreen_check = now;
            let fullscreen = win::is_fullscreen_foreground(self.hwnd);
            if fullscreen != self.fullscreen_hidden {
                self.fullscreen_hidden = fullscreen;
                if fullscreen {
                    self.destroy_region_select();
                    self.close_panel();
                    self.fun.abort();
                    self.hide_stats();
                }
                self.apply_visibility();
            }
        }

        if !self.visible() {
            if self.dragging {
                self.dragging = false;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            self.fun.abort();
            return;
        }

        self.update_dpi_if_needed();
        if self.quit {
            return;
        }

        let work = self.wander_area();
        let cursor = win::cursor_pos().unwrap_or_default();
        let dx = cursor.x as f32 - self.center_x;
        let dy = cursor.y as f32 - self.center_y;
        let radius = self.width as f32 / WINDOW_SCALE;
        let hit_radius = radius * 1.30;
        let cursor_dist = (dx * dx + dy * dy).sqrt();
        let over = cursor_dist <= hit_radius;
        let interactive = over || self.dragging;
        if interactive == self.click_through {
            self.click_through = !interactive;
            win::set_click_through(self.hwnd, !interactive);
        }

        let panel_open = self.panel_is_open();
        let stats_contains = self
            .stats
            .as_ref()
            .is_some_and(|stats| stats.is_open() && stats.contains_screen(cursor.x, cursor.y));
        let stats_hovering = cursor_dist <= radius * 2.4;
        if stats_hovering || stats_contains {
            self.stats_linger_until = self.clock + 1.3;
        }
        let want_stats = self.visible()
            && !panel_open
            && !self.dragging
            && self.region_select.is_none()
            && !self.fun.is_active()
            && (stats_hovering || stats_contains || self.clock < self.stats_linger_until);
        if want_stats && self.ensure_stats() {
            let (stats_x, stats_y) = self.stats_position(radius);
            if let Some(stats) = self.stats.as_mut() {
                stats.set_layout(stats_x, stats_y, self.center_x, self.center_y, radius);
                stats.set_open(true, self.clock);
            }
        } else if self.clock >= self.stats_linger_until {
            self.hide_stats();
        }
        let stats_open = self.stats.as_ref().is_some_and(|stats| stats.is_open());
        let can_start_fun = !panel_open
            && !stats_open
            && !self.dragging
            && self.region_select.is_none()
            && !self.settings.paused;
        let fun_output = self.fun.tick(FunContext {
            dt,
            center: (self.center_x, self.center_y),
            pet_radius: radius,
            pet_hwnd: self.hwnd,
            work,
            enabled: self.settings.fun_enabled,
            ripples_allowed: self.visible() && !self.settings.paused,
            can_start: can_start_fun,
            interval_seconds: self.settings.fun_interval_seconds as f32,
        });
        self.fun_absorb = fun_output.absorb;
        self.fun_ripple_age = fun_output.ripple_age;
        self.fun_ripple_strength = fun_output.ripple_strength;
        self.fun_white = fun_output.white;
        if fun_output.notice.is_some() {
            if fun_output.notice != self.last_fun_notice {
                self.notify_tray("趣味互动", fun_output.notice.unwrap_or_default());
            }
            self.last_fun_notice = fun_output.notice;
        } else {
            self.last_fun_notice = None;
        }
        let fun_override = fun_output.center.is_some() || self.fun.is_active();
        if let Some((x, y)) = fun_output.center {
            self.center_x = x;
            self.center_y = y;
        }

        if self.dragging {
            let px = cursor.x as f32 + self.drag_offset_x;
            let py = cursor.y as f32 + self.drag_offset_y;
            self.center_x = px;
            self.center_y = py;
            clamp_center(
                &mut self.center_x,
                &mut self.center_y,
                self.width,
                self.height,
                work,
            );
        } else if fun_override {
            self.wander_target = None;
        } else if !self.settings.paused {
            let near = cursor_dist < radius * 2.4;
            if self.settings.wander && !near && !panel_open && !stats_open {
                self.update_wander(dt, work);
            } else {
                self.wander_target = None;
            }
        }

        if !self.settings.paused {
            self.visual_time += dt;
        }

        if self.pulse > 0.001 {
            self.pulse_age += dt;
            self.pulse = (-self.pulse_age * 2.35).exp();
            if self.pulse < 0.002 {
                self.pulse = 0.0;
            }
        }

        let proximity = (1.0 - (cursor_dist / (radius * 2.6)).clamp(0.0, 1.0)).powi(2);
        let target_x = (dx / (radius * 1.9)).clamp(-1.0, 1.0) * proximity;
        let target_y = (dy / (radius * 1.9)).clamp(-1.0, 1.0) * proximity;
        let smooth = 1.0 - (-dt * 7.0).exp();
        self.hover_x += (target_x - self.hover_x) * smooth;
        self.hover_y += (target_y - self.hover_y) * smooth;
        self.hover_weight += (proximity - self.hover_weight) * smooth;

        self.clamp_position(work);
        self.render(now);
        self.render_panel();
        self.render_stats();
        if self.panel_is_open()
            || self.fun.is_active()
            || self.stats.as_ref().is_some_and(|stats| stats.is_active())
        {
            win::bring_to_top(self.hwnd);
        }
        self.poll_panel_close();
        self.poll_region_select();
        if self.settings.wander && !self.settings.paused {
            let moved = (self.center_x.round() as i32 - self.last_center_saved.0).abs() > 2
                || (self.center_y.round() as i32 - self.last_center_saved.1).abs() > 2;
            if moved && now.duration_since(self.last_save) >= Duration::from_secs(20) {
                self.save_settings();
            }
        }
    }

    fn update_dpi_if_needed(&mut self) {
        let dpi = unsafe { GetDpiForWindow(self.hwnd) } as f32 / 96.0;
        let dpi = if dpi <= 0.0 { 1.0 } else { dpi };
        let side = (self.settings.radius_dip * dpi * WINDOW_SCALE).round() as i32;
        if side != self.width || (dpi - self.dpi).abs() > 0.001 {
            if (dpi - self.dpi).abs() > 0.001 {
                self.destroy_region_select();
                self.destroy_panel();
                self.stats = None;
            }
            self.dpi = dpi;
            if let Err(error) = self.resize_to(side) {
                log::error(&format!("resize failed: {error}"));
                self.quit = true;
            }
        }
    }

    fn resize_to(&mut self, side: i32) -> Result<(), String> {
        let side = side.clamp(140, 1400);
        if side == self.width && self.surface.is_some() {
            return Ok(());
        }
        self.fun.abort();
        self.width = side;
        self.height = side;
        self.renderer.resize(side as u32, side as u32);
        self.surface = Some(LayeredSurface::new(self.hwnd, side, side)?);
        let work = self.wander_area();
        clamp_center(
            &mut self.center_x,
            &mut self.center_y,
            self.width,
            self.height,
            work,
        );
        log::info(&format!(
            "layout side={} dpi={:.2} center=({:.0},{:.0})",
            side, self.dpi, self.center_x, self.center_y
        ));
        Ok(())
    }

    fn update_wander(&mut self, dt: f32, work: RECT) {
        let half = self.width as f32 * 0.5;
        let min_x = work.left as f32 + half + 4.0;
        let max_x = work.right as f32 - half - 4.0;
        let min_y = work.top as f32 + half + 4.0;
        let max_y = work.bottom as f32 - half - 4.0;
        if max_x <= min_x || max_y <= min_y {
            return;
        }

        if self.wander_target.is_none() {
            if self.clock < self.wander_pause_until {
                return;
            }
            let mut target = None;
            for _ in 0..8 {
                let x = min_x + self.rand01() * (max_x - min_x);
                let y = min_y + self.rand01() * (max_y - min_y);
                let dx = x - self.center_x;
                let dy = y - self.center_y;
                if dx * dx + dy * dy > (self.width as f32 * 0.6).powi(2) {
                    target = Some((x, y));
                    break;
                }
            }
            self.wander_target = target.or(Some(((min_x + max_x) * 0.5, (min_y + max_y) * 0.5)));
        }

        let (tx, ty) = self.wander_target.unwrap();
        let dx = tx - self.center_x;
        let dy = ty - self.center_y;
        let dist = (dx * dx + dy * dy).sqrt();
        if dist < 4.0 {
            self.wander_target = None;
            self.wander_pause_until = self.clock + 1.2 + self.rand01() as f64 * 3.2;
            return;
        }

        let speed = (self.settings.wander_speed * self.dpi).max(0.0);
        let approach = (dist / 140.0).clamp(0.12, 1.0);
        let sway = (self.clock as f32 * 0.62 + self.wander_phase).sin()
            * 0.16
            * (dist / 220.0).clamp(0.0, 1.0);
        let (s, c) = sway.sin_cos();
        let nx = dx / dist;
        let ny = dy / dist;
        let mx = nx * c - ny * s;
        let my = nx * s + ny * c;
        let step = speed * approach * dt;
        self.center_x += mx * step;
        self.center_y += my * step;
        self.center_x = self.center_x.clamp(min_x, max_x);
        self.center_y = self.center_y.clamp(min_y, max_y);
    }

    fn wander_area(&self) -> RECT {
        let work = win::monitor_work_area(self.hwnd);
        if let Some([x, y, w, h]) = self.settings.custom_area {
            if w >= self.width + 16 && h >= self.width + 16 {
                let rect = RECT {
                    left: x.max(work.left),
                    top: y.max(work.top),
                    right: (x + w).min(work.right),
                    bottom: (y + h).min(work.bottom),
                };
                if rect.right - rect.left >= self.width + 16
                    && rect.bottom - rect.top >= self.width + 16
                {
                    return rect;
                }
            }
        }
        work
    }

    fn clamp_position(&mut self, work: RECT) {
        clamp_center(
            &mut self.center_x,
            &mut self.center_y,
            self.width,
            self.height,
            work,
        );
    }

    fn render(&mut self, now: Instant) {
        let radius = self.width as f32 / WINDOW_SCALE;
        let focus_active = self
            .settings
            .focus_until_unix
            .is_some_and(|until| until > unix_now());
        let focus_breath = if focus_active {
            0.10 + 0.05 * ((self.visual_time * 1.4).sin() * 0.5 + 0.5)
        } else {
            0.0
        };
        let params = FrameParams {
            time: self.visual_time,
            hole_radius: radius,
            pulse: self.pulse,
            pulse_age: self.pulse_age,
            fade: self.fade,
            hover_x: self.hover_x,
            hover_y: self.hover_y,
            motion_amount: self.hover_weight,
            lens_strength: self.settings.lens_strength,
            accent: self.settings.accent_linear(),
            fun: [
                self.fun_absorb,
                self.fun_ripple_age,
                self.fun_ripple_strength,
                self.fun_white,
            ],
            focus: focus_breath,
            debug: 0,
            ..FrameParams::default()
        };
        let left = (self.center_x - self.width as f32 * 0.5).round() as i32;
        let top = (self.center_y - self.height as f32 * 0.5).round() as i32;
        if self.desktop_lens {
            let _ = self.renderer.capture_backdrop(left, top);
        } else {
            self.renderer.disable_backdrop();
        }
        let pixels = match self.renderer.render(&params) {
            Ok(pixels) => pixels,
            Err(error) => {
                log::error(&format!("render failed: {error}"));
                return;
            }
        };
        self.frame_count = self.frame_count.wrapping_add(1);
        if !self.dump_done && self.frame_count >= 90 {
            if let Some(path) = self.dump_frame.clone() {
                snapshot::dump_frame(&path, pixels, self.width as u32, self.height as u32);
                self.dump_done = true;
            }
        }
        let result = self
            .surface
            .as_ref()
            .ok_or_else(|| "layered surface missing".to_string())
            .and_then(|surface| surface.update(pixels, left, top));
        match result {
            Ok(()) => {
                if !self.shown {
                    self.shown = true;
                    unsafe {
                        let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                        let _ = SetWindowPos(
                            self.hwnd,
                            HWND_TOPMOST,
                            0,
                            0,
                            0,
                            0,
                            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                        );
                    }
                    log::info("pet shown");
                }
            }
            Err(error) => {
                let should_log = self
                    .last_render_error
                    .map(|t| now.duration_since(t) >= Duration::from_secs(5))
                    .unwrap_or(true);
                if should_log {
                    log::error(&format!("present failed: {error}"));
                    self.last_render_error = Some(now);
                }
            }
        }
    }

    fn pulse_click(&mut self) {
        self.pulse = 1.0;
        self.pulse_age = 0.0;
    }

    fn handle_click_release(&mut self, cursor: POINT) {
        let elapsed = self.drag_started.elapsed();
        let dist = {
            let dx = cursor.x - self.drag_start.x;
            let dy = cursor.y - self.drag_start.y;
            ((dx * dx + dy * dy) as f32).sqrt()
        };
        if dist <= 4.0 && elapsed <= Duration::from_millis(700) {
            let double = self
                .last_click
                .map(|(t, p)| {
                    t.elapsed() <= double_click_time()
                        && ((p.x - cursor.x).abs() <= 4 && (p.y - cursor.y).abs() <= 4)
                })
                .unwrap_or(false);
            self.last_click = Some((Instant::now(), cursor));
            self.pulse_click();
            if double {
                self.settings.wander = !self.settings.wander;
                self.wander_target = None;
                self.wander_pause_until = self.clock + 1.0;
                self.save_settings();
                log::info(&format!("wander={}", self.settings.wander));
            } else if self.panel_is_open() {
                self.close_panel();
            } else {
                self.open_panel();
            }
        } else {
            if self.panel_is_open() {
                self.close_panel();
            }
            self.save_settings();
        }
    }

    fn handle_message(&mut self, msg: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        match msg {
            WM_NCHITTEST => {
                if self.dragging || self.cursor_in_hit_radius() {
                    Some(LRESULT(HTCLIENT as isize))
                } else {
                    Some(LRESULT(HTTRANSPARENT as isize))
                }
            }
            WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_SETCURSOR => {
                if (self.cursor_in_hit_radius() || self.dragging) && !self.hand_cursor.0.is_null() {
                    unsafe {
                        SetCursor(self.hand_cursor);
                    }
                    return Some(LRESULT(1));
                }
                None
            }
            WM_LBUTTONDOWN => {
                let cursor = win::cursor_pos().unwrap_or_default();
                if self.cursor_in_hit_radius() {
                    self.fun.abort();
                    self.hide_stats();
                    self.dragging = true;
                    self.drag_start = cursor;
                    self.drag_started = Instant::now();
                    self.drag_offset_x = self.center_x - cursor.x as f32;
                    self.drag_offset_y = self.center_y - cursor.y as f32;
                    self.wander_target = None;
                    unsafe {
                        SetCapture(self.hwnd);
                    }
                    Some(LRESULT(0))
                } else {
                    Some(LRESULT(0))
                }
            }
            WM_MOUSEMOVE => {
                if self.dragging {
                    let cursor = win::cursor_pos().unwrap_or_default();
                    self.center_x = cursor.x as f32 + self.drag_offset_x;
                    self.center_y = cursor.y as f32 + self.drag_offset_y;
                    let work = self.wander_area();
                    clamp_center(
                        &mut self.center_x,
                        &mut self.center_y,
                        self.width,
                        self.height,
                        work,
                    );
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if self.dragging {
                    self.dragging = false;
                    unsafe {
                        let _ = ReleaseCapture();
                    }
                    let cursor = win::cursor_pos().unwrap_or_default();
                    self.handle_click_release(cursor);
                }
                Some(LRESULT(0))
            }
            WM_RBUTTONUP | WM_CONTEXTMENU => {
                if !self.panel_is_open() {
                    self.open_panel();
                }
                Some(LRESULT(0))
            }
            WM_HOTKEY => {
                if wparam.0 as i32 == HOTKEY_ID {
                    self.toggle_hidden();
                }
                Some(LRESULT(0))
            }
            WM_TRAY => {
                self.handle_tray(wparam, lparam);
                Some(LRESULT(0))
            }
            WM_DPICHANGED => {
                self.fun.abort();
                let rect = unsafe { &*(lparam.0 as *const RECT) };
                self.center_x = ((rect.left + rect.right) as f32) * 0.5;
                self.center_y = ((rect.top + rect.bottom) as f32) * 0.5;
                let dpi = unsafe { GetDpiForWindow(self.hwnd) } as f32 / 96.0;
                self.dpi = if dpi <= 0.0 { 1.0 } else { dpi };
                let side = (self.settings.radius_dip * self.dpi * WINDOW_SCALE).round() as i32;
                if let Err(error) = self.resize_to(side) {
                    log::error(&format!("dpi resize failed: {error}"));
                }
                Some(LRESULT(0))
            }
            WM_DISPLAYCHANGE => {
                self.fun.abort();
                let work = self.wander_area();
                self.clamp_position(work);
                Some(LRESULT(0))
            }
            WM_QUERYENDSESSION | WM_ENDSESSION => {
                self.save_settings();
                self.quit = true;
                Some(LRESULT(1))
            }
            WM_CLOSE => {
                self.begin_exit();
                Some(LRESULT(0))
            }
            WM_DESTROY => {
                self.quit = true;
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn handle_tray(&mut self, wparam: WPARAM, lparam: LPARAM) {
        let _ = wparam;
        let event = (lparam.0 as u32) & 0xFFFF;
        match event {
            NIN_BALLOONUSERCLICK => {
                self.open_panel_reminders = true;
                if !self.visible() && self.user_hidden {
                    self.user_hidden = false;
                    self.apply_visibility();
                }
                if self.panel_is_open() {
                    if let Some(panel) = self.panel.as_mut() {
                        panel.show_reminders_page();
                    }
                    self.open_panel_reminders = false;
                } else {
                    self.open_panel();
                }
            }
            WM_RBUTTONUP | WM_CONTEXTMENU | WM_LBUTTONUP | WM_LBUTTONDBLCLK => {
                if !self.visible() {
                    if self.user_hidden {
                        self.user_hidden = false;
                        self.apply_visibility();
                    }
                } else if self.panel_is_open() {
                    self.close_panel();
                } else {
                    self.open_panel();
                }
            }
            _ => {}
        }
    }

    fn toggle_hidden(&mut self) {
        self.user_hidden = !self.user_hidden;
        if self.user_hidden {
            self.destroy_region_select();
            self.close_panel();
            self.fun.abort();
            self.hide_stats();
        }
        self.apply_visibility();
        log::info(&format!("user_hidden={}", self.user_hidden));
    }

    fn apply_visibility(&mut self) {
        if self.visible() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                let _ = SetWindowPos(
                    self.hwnd,
                    HWND_TOPMOST,
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
            self.last_frame = Instant::now();
        } else {
            if self.dragging {
                self.dragging = false;
                unsafe {
                    let _ = ReleaseCapture();
                }
            }
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    fn cursor_in_hit_radius(&self) -> bool {
        let cursor = win::cursor_pos().unwrap_or_default();
        let dx = cursor.x as f32 - self.center_x;
        let dy = cursor.y as f32 - self.center_y;
        let hit = self.width as f32 / WINDOW_SCALE * 1.30;
        dx * dx + dy * dy <= hit * hit
    }

    fn panel_is_open(&self) -> bool {
        self.panel.as_ref().is_some_and(|panel| panel.is_open())
    }

    fn panel_position(&self) -> (i32, i32) {
        let panel_w = (PANEL_WIDTH_DIP * self.dpi).round() as i32;
        let panel_h = (PANEL_HEIGHT_DIP * self.dpi).round() as i32;
        let gap = (PANEL_GAP_DIP * self.dpi).round() as i32;
        let margin = (PANEL_MARGIN_DIP * self.dpi).round() as i32;
        let work = win::monitor_work_area(self.hwnd);
        let pet_left = (self.center_x - self.width as f32 * 0.5).round() as i32;
        let pet_right = pet_left + self.width;
        let right_x = pet_right + gap;
        let left_x = pet_left - gap - panel_w;

        let x = if right_x + panel_w <= work.right - margin {
            right_x
        } else if left_x >= work.left + margin {
            left_x
        } else {
            (work.right - margin - panel_w).max(work.left + margin)
        };
        let min_y = work.top + margin;
        let max_y = (work.bottom - margin - panel_h).max(min_y);
        let y = ((self.center_y.round() as i32) - panel_h / 2).clamp(min_y, max_y);
        (x, y)
    }

    fn ensure_stats(&mut self) -> bool {
        if self.stats.is_some() {
            return true;
        }
        let width = (STATS_WIDTH_DIP * self.dpi).round().max(64.0) as i32;
        let height = (STATS_HEIGHT_DIP * self.dpi).round().max(64.0) as i32;
        let instance = match unsafe { GetModuleHandleW(None) } {
            Ok(instance) => instance,
            Err(error) => {
                log::error(&format!("stats window instance failed: {error}"));
                return false;
            }
        };
        let class = win::wide(STATS_CLASS);
        let title = win::wide("Singularity stats");
        let hwnd = match unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
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
        } {
            Ok(hwnd) => hwnd,
            Err(error) => {
                log::error(&format!("stats window create failed: {error}"));
                return false;
            }
        };
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut App as isize);
        }
        match StatsOverlay::new(
            hwnd,
            self.dpi,
            self.renderer.device(),
            self.renderer.queue(),
        ) {
            Ok(stats) => {
                self.stats = Some(stats);
                true
            }
            Err(error) => {
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                log::error(&format!("stats overlay create failed: {error}"));
                false
            }
        }
    }

    fn stats_position(&self, radius: f32) -> (i32, i32) {
        let width = (STATS_WIDTH_DIP * self.dpi).round() as i32;
        let height = (STATS_HEIGHT_DIP * self.dpi).round() as i32;
        let margin = (12.0 * self.dpi).round() as i32;
        let gap = (radius * 3.3 + 24.0 * self.dpi).round() as i32;
        let work = win::monitor_work_area(self.hwnd);
        let center_x = self.center_x.round() as i32;
        let center_y = self.center_y.round() as i32;
        let min_x = work.left + margin;
        let max_x = (work.right - margin - width).max(min_x);
        let min_y = work.top + margin;
        let max_y = (work.bottom - margin - height).max(min_y);
        let right_x = center_x + gap;
        let left_x = center_x - gap - width;
        let (mut x, mut y) = if right_x + width <= work.right - margin {
            (right_x, center_y - height / 2)
        } else if left_x >= work.left + margin {
            (left_x, center_y - height / 2)
        } else {
            let below_y = center_y + gap;
            let above_y = center_y - gap - height;
            if below_y + height <= work.bottom - margin {
                (center_x - width / 2, below_y)
            } else if above_y >= work.top + margin {
                (center_x - width / 2, above_y)
            } else {
                (left_x, center_y - height / 2)
            }
        };
        x = x.clamp(min_x, max_x);
        y = y.clamp(min_y, max_y);
        (x, y)
    }

    fn hide_stats(&mut self) {
        self.stats_linger_until = 0.0;
        if let Some(stats) = self.stats.as_mut() {
            stats.set_open(false, self.clock);
            stats.pointer_gone();
        }
    }

    fn render_stats(&mut self) {
        let action = {
            let Some(stats) = self.stats.as_mut() else {
                return;
            };
            if !stats.is_active() {
                return;
            }
            let snapshot = self.sysmon.snapshot();
            stats.render(&self.settings, snapshot, self.clock, self.hwnd)
        };
        match action {
            Some(StatsAction::Media(key)) => self.send_media_key(&key),
            Some(StatsAction::Close) => self.hide_stats(),
            Some(StatsAction::ToggleTodo { id, done }) => {
                if let Some(todo) = self.settings.todos.iter_mut().find(|todo| todo.id == id) {
                    todo.done = done;
                    self.save_settings();
                }
            }
            None => {}
        }
    }

    fn ensure_panel(&mut self) -> bool {
        if self.panel.is_some() {
            return true;
        }
        let panel_w = (PANEL_WIDTH_DIP * self.dpi).round() as i32;
        let panel_h = (PANEL_HEIGHT_DIP * self.dpi).round() as i32;
        let instance = match unsafe { GetModuleHandleW(None) } {
            Ok(instance) => instance,
            Err(error) => {
                log::error(&format!("panel GetModuleHandleW failed: {error}"));
                return false;
            }
        };
        let class_name = win::wide(PANEL_CLASS);
        let title = win::wide("奇点 · 设置");
        let hwnd = match unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_TOPMOST,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                0,
                0,
                panel_w,
                panel_h,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )
        } {
            Ok(hwnd) => hwnd,
            Err(error) => {
                log::error(&format!("panel CreateWindowExW failed: {error}"));
                return false;
            }
        };
        win::exclude_from_capture(hwnd);
        match SettingsPanel::new(
            hwnd,
            self.dpi,
            self.renderer.device(),
            self.renderer.queue(),
        ) {
            Ok(panel) => {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut App as isize);
                }
                self.panel = Some(panel);
                true
            }
            Err(error) => {
                log::error(&format!("panel init failed: {error}"));
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                false
            }
        }
    }

    fn destroy_panel(&mut self) {
        if let Some(panel) = self.panel.take() {
            let hwnd = panel.hwnd();
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
            }
        }
    }

    fn open_panel(&mut self) {
        if self.panel_is_open() || !self.ensure_panel() {
            return;
        }
        self.fun.abort();
        self.hide_stats();
        self.pulse_click();
        let (x, y) = self.panel_position();
        let clock = self.clock;
        let settings = self.settings.clone();
        if let Some(panel) = self.panel.as_mut() {
            panel.sync_settings(&settings);
            panel.set_layout(
                x,
                y,
                self.center_x,
                self.center_y,
                self.width as f32 / WINDOW_SCALE,
            );
            panel.set_open(true, clock);
            if self.open_panel_reminders {
                panel.show_reminders_page();
                self.open_panel_reminders = false;
            }
        }
        self.panel_guard_until = clock + 0.30;
        self.last_left_button =
            (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000) != 0;
        self.wander_target = None;
        log::info("settings panel opened");
    }

    fn close_panel(&mut self) {
        if !self.visible() {
            self.destroy_panel();
            return;
        }
        if let Some(panel) = self.panel.as_mut() {
            panel.set_open(false, self.clock);
        }
        self.wander_pause_until = self.clock + 0.8;
        log::info("settings panel closing");
        self.save_settings();
    }

    fn render_panel(&mut self) {
        if self.panel.is_none() {
            return;
        }
        let (x, y) = self.panel_position();
        let clock = self.clock;
        let actions = match self.panel.as_mut() {
            Some(panel) => {
                panel.set_layout(
                    x,
                    y,
                    self.center_x,
                    self.center_y,
                    self.width as f32 / WINDOW_SCALE,
                );
                panel.render(&self.settings, clock)
            }
            None => None,
        };
        if let Some(actions) = actions {
            self.apply_panel_actions(actions);
        }
    }

    fn poll_panel_close(&mut self) {
        if self.region_select.is_some() {
            return;
        }
        let down = (unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) } as u16 & 0x8000) != 0;
        if !self.panel_is_open() {
            self.last_left_button = down;
            return;
        }
        if self.clock >= self.panel_guard_until {
            if down && !self.last_left_button {
                let cursor = win::cursor_pos().unwrap_or_default();
                let on_panel = self
                    .panel
                    .as_ref()
                    .is_some_and(|panel| panel.contains_screen(cursor.x, cursor.y));
                if !on_panel && !self.cursor_in_hit_radius() {
                    self.close_panel();
                } else if !on_panel && !down {
                    if let Some(panel) = self.panel.as_mut() {
                        panel.pointer_gone();
                    }
                }
            }
            let escape = (unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } as u16 & 0x8000) != 0;
            if escape {
                self.close_panel();
            }
        }
        self.last_left_button = down;
    }

    fn start_region_select(&mut self) -> bool {
        if self.region_select.is_some() {
            return true;
        }
        let work = win::monitor_work_area(self.hwnd);
        let width = (work.right - work.left).max(120);
        let height = (work.bottom - work.top).max(120);
        let instance = match unsafe { GetModuleHandleW(None) } {
            Ok(instance) => instance,
            Err(error) => {
                log::error(&format!("region GetModuleHandleW failed: {error}"));
                return false;
            }
        };
        let class_name = win::wide(REGION_CLASS);
        let title = win::wide("选择黑洞漫游范围");
        let hwnd = match unsafe {
            CreateWindowExW(
                WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST,
                PCWSTR(class_name.as_ptr()),
                PCWSTR(title.as_ptr()),
                WS_POPUP,
                work.left,
                work.top,
                width,
                height,
                HWND::default(),
                HMENU::default(),
                instance,
                None,
            )
        } {
            Ok(hwnd) => hwnd,
            Err(error) => {
                log::error(&format!("region CreateWindowExW failed: {error}"));
                return false;
            }
        };
        win::exclude_from_capture(hwnd);
        match RegionSelector::new(hwnd, work, &self.settings.accent, self.width + 40) {
            Ok(selector) => {
                unsafe {
                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, self as *mut App as isize);
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                self.region_select = Some(selector);
                log::info("region selection started");
                true
            }
            Err(error) => {
                log::error(&format!("region init failed: {error}"));
                unsafe {
                    let _ = DestroyWindow(hwnd);
                }
                false
            }
        }
    }

    fn destroy_region_select(&mut self) {
        if let Some(selector) = self.region_select.take() {
            let hwnd = selector.hwnd();
            unsafe {
                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                let _ = DestroyWindow(hwnd);
            }
            log::info("region selection closed");
        }
    }

    fn handle_region_message(
        &mut self,
        _hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        let response = self
            .region_select
            .as_mut()
            .and_then(|selector| selector.handle_message(msg, wparam, lparam));
        match response {
            Some(RegionResponse::Handled(result)) => Some(result),
            Some(RegionResponse::Selected(rect)) => {
                self.apply_custom_area(rect);
                self.destroy_region_select();
                Some(LRESULT(0))
            }
            Some(RegionResponse::Cancelled) => {
                self.destroy_region_select();
                Some(LRESULT(0))
            }
            None => None,
        }
    }

    fn apply_custom_area(&mut self, rect: RECT) {
        let work = win::monitor_work_area(self.hwnd);
        let area = RECT {
            left: rect.left.max(work.left),
            top: rect.top.max(work.top),
            right: rect.right.min(work.right),
            bottom: rect.bottom.min(work.bottom),
        };
        let width = area.right - area.left;
        let height = area.bottom - area.top;
        if width < self.width + 16 || height < self.width + 16 {
            log::warn("region selection too small; ignored");
            return;
        }
        self.settings.custom_area = Some([area.left, area.top, width, height]);
        let wander_area = self.wander_area();
        self.clamp_position(wander_area);
        self.save_settings();
        log::info(&format!(
            "custom wander area set: {},{} {}x{}",
            area.left, area.top, width, height
        ));
    }

    fn set_work_area(&mut self) {
        self.settings.custom_area = None;
        let work = self.wander_area();
        self.clamp_position(work);
        self.save_settings();
        log::info("wander area reset to work area");
    }

    fn poll_region_select(&mut self) {
        if self.region_select.is_none() {
            return;
        }
        let escape = (unsafe { GetAsyncKeyState(VK_ESCAPE.0 as i32) } as u16 & 0x8000) != 0;
        if escape {
            self.destroy_region_select();
        }
    }

    fn check_reminders(&mut self) {
        if self.settings.reminders.is_empty() {
            return;
        }
        let now = unix_now();
        let mut due = Vec::new();
        let mut repeating = Vec::new();
        for mut reminder in std::mem::take(&mut self.settings.reminders) {
            if reminder.due_unix <= now {
                if reminder.enabled {
                    due.push(reminder.label.clone());
                    if reminder.repeat_daily {
                        while reminder.due_unix <= now {
                            reminder.due_unix = reminder.due_unix.saturating_add(86_400);
                        }
                    } else {
                        continue;
                    }
                } else if reminder.repeat_daily {
                    while reminder.due_unix <= now {
                        reminder.due_unix = reminder.due_unix.saturating_add(86_400);
                    }
                }
            }
            repeating.push(reminder);
        }
        self.settings.reminders = repeating;
        if due.is_empty() {
            return;
        }
        self.save_settings();
        self.notify_tray("奇点提醒", &due.join(" · "));
        if self.settings.reminder_sound {
            unsafe {
                let _ = MessageBeep(MB_ICONASTERISK);
            }
        }
        self.pulse_click();
        log::info(&format!("reminder due: {}", due.join(" / ")));
    }

    fn notify_tray(&self, title: &str, info: &str) {
        if !self.tray_added {
            log::warn("tray icon unavailable; cannot show notification");
            return;
        }
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_ID,
            uFlags: NIF_INFO,
            ..Default::default()
        };
        data.Anonymous = NOTIFYICONDATAW_0 { uTimeout: 6000 };
        data.dwInfoFlags = NIIF_INFO;
        write_wide(&mut data.szInfoTitle, title);
        write_wide(&mut data.szInfo, info);
        let _ = unsafe { Shell_NotifyIconW(NIM_MODIFY, &data) };
    }

    fn start_focus(&mut self, minutes: u32) {
        let minutes = minutes.clamp(1, 180);
        if self.settings.focus_reminder_id.is_some() || self.settings.focus_until_unix.is_some() {
            self.stop_focus();
        }
        let id = self
            .settings
            .reminders
            .iter()
            .map(|reminder| reminder.id)
            .max()
            .unwrap_or(0)
            .wrapping_add(1);
        let due = unix_now() + minutes as u64 * 60;
        self.settings.reminders.push(Reminder {
            id,
            due_unix: due,
            label: "专注结束 · 休息一下".to_owned(),
            repeat_daily: false,
            enabled: true,
        });
        self.settings.focus_until_unix = Some(due);
        self.settings.focus_minutes = minutes;
        self.settings.focus_reminder_id = Some(id);
        self.save_settings();
        self.pulse_click();
        log::info(&format!("focus started: {minutes} minutes (id={id})"));
    }

    fn stop_focus(&mut self) {
        if let Some(id) = self.settings.focus_reminder_id.take() {
            self.settings.reminders.retain(|reminder| reminder.id != id);
        }
        self.settings.focus_until_unix = None;
        self.save_settings();
        log::info("focus stopped");
    }

    fn send_media_key(&self, action: &str) {
        let key = match action {
            "previous" => VK_MEDIA_PREV_TRACK,
            "play_pause" => VK_MEDIA_PLAY_PAUSE,
            "next" => VK_MEDIA_NEXT_TRACK,
            "mute" => VK_VOLUME_MUTE,
            _ => return,
        };
        log::info(&format!("media key: {action}"));
        unsafe {
            keybd_event(key.0 as u8, 0, KEYBD_EVENT_FLAGS(0), 0);
            keybd_event(key.0 as u8, 0, KEYEVENTF_KEYUP, 0);
        }
    }

    fn apply_panel_actions(&mut self, actions: PanelActions) {
        let now = Instant::now();
        if let Some(value) = actions.radius_dip {
            self.settings.radius_dip = value.clamp(28.0, 92.0);
        }
        if actions.radius_dip.is_some() || actions.commit {
            let side = (self.settings.radius_dip * self.dpi * WINDOW_SCALE).round() as i32;
            let due = actions.commit
                || now.duration_since(self.last_radius_resize) >= Duration::from_millis(70);
            if side != self.width && due {
                if let Err(error) = self.resize_to(side) {
                    log::error(&format!("panel radius resize failed: {error}"));
                }
                self.last_radius_resize = now;
            }
        }
        if let Some(value) = actions.lens_strength {
            self.settings.lens_strength = value.clamp(0.0, 2.0);
        }
        if let Some(value) = actions.accent {
            if settings::parse_accent_hex(&value).is_some() {
                self.settings.accent = value;
            }
        }
        if let Some(value) = actions.wander {
            self.settings.wander = value;
            if !value {
                self.wander_target = None;
            }
        }
        if let Some(value) = actions.wander_speed {
            self.settings.wander_speed = value.clamp(0.0, 120.0);
        }
        if let Some(value) = actions.paused {
            self.settings.paused = value;
        }
        if let Some(value) = actions.fullscreen_hide {
            self.settings.fullscreen_hide = value;
            self.apply_visibility();
        }
        if let Some(value) = actions.fun_interval_seconds {
            self.settings.fun_interval_seconds = value.clamp(30, 300);
            self.fun
                .set_interval_seconds(self.settings.fun_interval_seconds as f32);
        }
        if let Some(value) = actions.fun_enabled {
            let was_enabled = self.settings.fun_enabled;
            self.settings.fun_enabled = value;
            if value && !was_enabled {
                self.fun
                    .set_interval_seconds(self.settings.fun_interval_seconds as f32);
            }
            if !value {
                self.fun.abort();
            }
        }
        if actions.fun_demo {
            self.close_panel();
            self.fun.request_demo();
        }
        if actions.set_work_area {
            self.set_work_area();
        }
        if actions.select_region {
            self.start_region_select();
        }
        if let Some(minutes) = actions.add_reminder {
            let minutes = minutes.clamp(1, 720);
            let id = self
                .settings
                .reminders
                .iter()
                .map(|reminder| reminder.id)
                .max()
                .unwrap_or(0)
                .wrapping_add(1);
            let due_unix = unix_now() + minutes as u64 * 60;
            self.settings.reminders.push(Reminder {
                id,
                due_unix,
                label: format!("{minutes} 分钟提醒"),
                repeat_daily: false,
                enabled: true,
            });
            self.save_settings();
            log::info(&format!("reminder added: {minutes} minutes (id={id})"));
        }
        if let Some((hour, minute, daily)) = actions.add_alarm {
            let hour = hour.min(23);
            let minute = minute.min(59);
            let now_local = local_seconds_of_day();
            let target = hour * 3600 + minute * 60;
            let mut delta = target as i64 - now_local as i64;
            if delta <= 0 {
                delta += 86_400;
            }
            let id = self
                .settings
                .reminders
                .iter()
                .map(|reminder| reminder.id)
                .max()
                .unwrap_or(0)
                .wrapping_add(1);
            self.settings.reminders.push(Reminder {
                id,
                due_unix: unix_now() + delta as u64,
                label: format!("{hour:02}:{minute:02} 闹钟"),
                repeat_daily: daily,
                enabled: true,
            });
            self.save_settings();
            log::info(&format!(
                "alarm added: {hour:02}:{minute:02} daily={daily} (id={id})"
            ));
        }
        if let Some((id, enabled)) = actions.set_reminder_enabled {
            if let Some(reminder) = self
                .settings
                .reminders
                .iter_mut()
                .find(|reminder| reminder.id == id)
            {
                reminder.enabled = enabled;
                self.save_settings();
                log::info(&format!("reminder enabled={enabled}: id={id}"));
            }
        }
        if let Some(sound) = actions.set_reminder_sound {
            self.settings.reminder_sound = sound;
            self.save_settings();
        }
        if let Some(id) = actions.remove_reminder {
            self.settings.reminders.retain(|reminder| reminder.id != id);
            self.save_settings();
            log::info(&format!("reminder removed: id={id}"));
        }
        if let Some(minutes) = actions.start_focus {
            self.start_focus(minutes);
        }
        if actions.stop_focus {
            self.stop_focus();
        }
        if let Some(text) = actions.add_note {
            let text = text.trim().to_owned();
            if !text.is_empty() {
                let id = self
                    .settings
                    .notes
                    .iter()
                    .map(|note| note.id)
                    .max()
                    .unwrap_or(0)
                    .wrapping_add(1);
                self.settings.notes.push(Note {
                    id,
                    text,
                    created_unix: unix_now(),
                });
                while self.settings.notes.len() > 100 {
                    self.settings.notes.remove(0);
                }
                self.save_settings();
            }
        }
        if let Some(id) = actions.remove_note {
            self.settings.notes.retain(|note| note.id != id);
            self.save_settings();
        }
        if let Some(title) = actions.add_todo {
            let title = title.trim().to_owned();
            if !title.is_empty() {
                let id = self
                    .settings
                    .todos
                    .iter()
                    .map(|todo| todo.id)
                    .max()
                    .unwrap_or(0)
                    .wrapping_add(1);
                self.settings.todos.push(Todo {
                    id,
                    title,
                    done: false,
                });
                self.save_settings();
            }
        }
        if let Some((id, done)) = actions.toggle_todo {
            if let Some(todo) = self.settings.todos.iter_mut().find(|todo| todo.id == id) {
                todo.done = done;
                self.save_settings();
            }
        }
        if let Some(id) = actions.remove_todo {
            self.settings.todos.retain(|todo| todo.id != id);
            self.save_settings();
        }
        if actions.reset_position {
            let work = win::monitor_work_area(self.hwnd);
            self.center_x = work.left as f32 + (work.right - work.left) as f32 * 0.65;
            self.center_y = work.top as f32 + (work.bottom - work.top) as f32 * 0.56;
            let area = self.wander_area();
            self.clamp_position(area);
            self.wander_target = None;
            self.save_settings();
            log::info("pet position reset");
        }
        if actions.hide {
            self.toggle_hidden();
        }
        if actions.exit {
            self.save_settings();
            self.destroy_region_select();
            self.destroy_panel();
            self.begin_exit();
        }
        if actions.reset {
            self.reset_settings();
        }
        if actions.commit && !actions.close {
            self.save_settings();
        }
        if actions.close {
            self.close_panel();
        }
    }

    fn reset_settings(&mut self) {
        let x = self.settings.x;
        let y = self.settings.y;
        let intro_seen = self.settings.intro_seen;
        let reminders = std::mem::take(&mut self.settings.reminders);
        let todos = std::mem::take(&mut self.settings.todos);
        let notes = std::mem::take(&mut self.settings.notes);
        let focus_until = self.settings.focus_until_unix;
        let focus_minutes = self.settings.focus_minutes;
        let focus_reminder = self.settings.focus_reminder_id;
        self.settings = Settings::default();
        self.settings.x = x;
        self.settings.y = y;
        self.settings.intro_seen = intro_seen;
        self.settings.reminders = reminders;
        self.settings.todos = todos;
        self.settings.notes = notes;
        self.settings.focus_until_unix = focus_until;
        self.settings.focus_minutes = focus_minutes;
        self.settings.focus_reminder_id = focus_reminder;
        self.fun.abort();
        let side = (self.settings.radius_dip * self.dpi * WINDOW_SCALE).round() as i32;
        if let Err(error) = self.resize_to(side) {
            log::error(&format!("reset resize failed: {error}"));
        }
        self.wander_target = None;
        self.last_radius_resize = Instant::now();
        self.save_settings();
    }

    fn handle_stats_message(
        &mut self,
        msg: u32,
        _wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        let x = (lparam.0 & 0xFFFF) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        match msg {
            WM_MOUSEACTIVATE => Some(LRESULT(MA_NOACTIVATE as isize)),
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_MOUSEMOVE => {
                if let Some(stats) = self.stats.as_mut() {
                    stats.pointer_moved(x, y);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                if let Some(stats) = self.stats.as_mut() {
                    stats.pointer_button(x, y, true);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                if let Some(stats) = self.stats.as_mut() {
                    stats.pointer_button(x, y, false);
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn handle_panel_message(
        &mut self,
        hwnd: HWND,
        msg: u32,
        wparam: WPARAM,
        lparam: LPARAM,
    ) -> Option<LRESULT> {
        let x = (lparam.0 & 0xFFFF) as i16 as f32;
        let y = ((lparam.0 >> 16) & 0xFFFF) as i16 as f32;
        match msg {
            WM_CHAR | WM_IME_CHAR => {
                if let Some(character) = char::from_u32(wparam.0 as u32) {
                    if !character.is_control() {
                        if let Some(panel) = self.panel.as_mut() {
                            panel.text_input(&character.to_string());
                        }
                    }
                }
                Some(LRESULT(0))
            }
            WM_KEYDOWN | WM_KEYUP => {
                if let Some(panel) = self.panel.as_mut() {
                    panel.key_input(wparam.0 as u16, msg == WM_KEYDOWN);
                }
                Some(LRESULT(0))
            }
            WM_ERASEBKGND => Some(LRESULT(1)),
            WM_NCHITTEST => Some(LRESULT(HTCLIENT as isize)),
            WM_MOUSEMOVE => {
                if let Some(panel) = self.panel.as_mut() {
                    panel.pointer_moved(x, y);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONDOWN => {
                unsafe {
                    let _ = SetForegroundWindow(hwnd);
                    let _ = SetFocus(hwnd);
                    SetCapture(hwnd);
                }
                if let Some(panel) = self.panel.as_mut() {
                    panel.pointer_button(x, y, true);
                }
                Some(LRESULT(0))
            }
            WM_LBUTTONUP => {
                unsafe {
                    let _ = ReleaseCapture();
                }
                if let Some(panel) = self.panel.as_mut() {
                    panel.pointer_button(x, y, false);
                }
                Some(LRESULT(0))
            }
            WM_MOUSEWHEEL => {
                let delta = ((wparam.0 >> 16) & 0xFFFF) as i16 as f32;
                if let Some(panel) = self.panel.as_mut() {
                    panel.mouse_wheel(delta);
                }
                Some(LRESULT(0))
            }
            _ => None,
        }
    }

    fn begin_exit(&mut self) {
        if self.exiting {
            return;
        }
        self.exiting = true;
        self.fun.abort();
        self.hide_stats();
        self.save_settings();
        if !self.visible() {
            self.quit = true;
        }
        log::info("exit requested");
    }

    fn save_settings(&mut self) {
        self.settings.x = Some(self.center_x.round() as i32);
        self.settings.y = Some(self.center_y.round() as i32);
        self.last_center_saved = (self.settings.x.unwrap_or(0), self.settings.y.unwrap_or(0));
        self.last_save = Instant::now();
        settings::save(&self.settings);
    }

    fn rand01(&mut self) -> f32 {
        let mut x = self.rng;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.rng = x;
        (x & 0x00ff_ffff) as f32 / 16_777_215.0
    }

    fn shutdown(&mut self) {
        self.save_settings();
        self.fun.abort();
        self.destroy_region_select();
        self.destroy_panel();
        self.stats = None;
        self.remove_tray_icon();
        unsafe {
            let _ = UnregisterHotKey(self.hwnd, HOTKEY_ID);
        }
        self.surface = None;
        log::info("shutdown complete");
    }
}

fn clamp_center(cx: &mut f32, cy: &mut f32, width: i32, height: i32, work: RECT) {
    let half_w = width as f32 * 0.5 + 2.0;
    let half_h = height as f32 * 0.5 + 2.0;
    let min_x = work.left as f32 + half_w;
    let max_x = work.right as f32 - half_w;
    let min_y = work.top as f32 + half_h;
    let max_y = work.bottom as f32 - half_h;
    *cx = if max_x <= min_x {
        (work.left + work.right) as f32 * 0.5
    } else {
        (*cx).clamp(min_x, max_x)
    };
    *cy = if max_y <= min_y {
        (work.top + work.bottom) as f32 * 0.5
    } else {
        (*cy).clamp(min_y, max_y)
    };
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn local_seconds_of_day() -> u32 {
    let time = unsafe { GetLocalTime() };
    time.wHour as u32 * 3600 + time.wMinute as u32 * 60 + time.wSecond as u32
}

fn write_wide(dst: &mut [u16], text: &str) {
    let encoded: Vec<u16> = text.encode_utf16().collect();
    let count = encoded.len().min(dst.len().saturating_sub(1));
    dst[..count].copy_from_slice(&encoded[..count]);
    if count < dst.len() {
        dst[count] = 0;
    }
}

fn double_click_time() -> Duration {
    let ms = unsafe { GetDoubleClickTime() }.max(200);
    Duration::from_millis(ms as u64)
}

unsafe extern "system" fn region_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app = &mut *ptr;
        if let Some(result) = app.handle_region_message(hwnd, msg, wparam, lparam) {
            return result;
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn panel_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app = &mut *ptr;
        if let Some(result) = app.handle_panel_message(hwnd, msg, wparam, lparam) {
            return result;
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn stats_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app = &mut *ptr;
        if let Some(result) = app.handle_stats_message(msg, wparam, lparam) {
            return result;
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}

unsafe extern "system" fn wndproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if msg == WM_NCDESTROY {
        SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }
    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA) as *mut App;
    if !ptr.is_null() {
        let app = &mut *ptr;
        if let Some(result) = app.handle_message(msg, wparam, lparam) {
            return result;
        }
    }
    DefWindowProcW(hwnd, msg, wparam, lparam)
}
