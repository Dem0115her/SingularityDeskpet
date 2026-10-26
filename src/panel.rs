use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use egui::{
    Align, Align2, Color32, FontId, Layout, Margin, Pos2, RichText, Rounding, Sense, Stroke, Vec2,
};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, VK_CONTROL, VK_MENU, VK_SHIFT};
use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_HIDE, SW_SHOWNOACTIVATE};

use crate::log;
use crate::settings::{self, Settings};
use crate::ui::{self, EguiCanvas, MUTED, TEXT};
use crate::win::LayeredSurface;

pub const PANEL_WIDTH_DIP: f32 = 360.0;
pub const PANEL_HEIGHT_DIP: f32 = 500.0;
pub const PANEL_MARGIN_DIP: f32 = 12.0;
pub const PANEL_GAP_DIP: f32 = 14.0;

const CARD_MARGIN: f32 = 12.0;
const CARD_INNER: f32 = 16.0;
const CARD_ROUNDING: f32 = 20.0;
const CONTENT_WIDTH: f32 = PANEL_WIDTH_DIP - CARD_MARGIN * 2.0 - CARD_INNER * 2.0;
const SCROLL_HEIGHT: f32 = 300.0;
const OPEN_SECONDS: f32 = 0.52;
const CLOSE_SECONDS: f32 = 0.26;

const BG: Color32 = Color32::from_rgba_premultiplied(14, 18, 27, 246);

#[derive(Default)]
pub struct PanelActions {
    pub close: bool,
    pub radius_dip: Option<f32>,
    pub lens_strength: Option<f32>,
    pub accent: Option<String>,
    pub wander: Option<bool>,
    pub wander_speed: Option<f32>,
    pub paused: Option<bool>,
    pub fullscreen_hide: Option<bool>,
    pub set_work_area: bool,
    pub select_region: bool,
    pub fun_enabled: Option<bool>,
    pub fun_interval_seconds: Option<u32>,
    pub fun_demo: bool,
    pub add_reminder: Option<u32>,
    pub add_alarm: Option<(u32, u32, bool)>,
    pub set_reminder_enabled: Option<(u32, bool)>,
    pub set_reminder_sound: Option<bool>,
    pub remove_reminder: Option<u32>,
    pub start_focus: Option<u32>,
    pub stop_focus: bool,
    pub add_todo: Option<String>,
    pub toggle_todo: Option<(u32, bool)>,
    pub remove_todo: Option<u32>,
    pub add_note: Option<String>,
    pub remove_note: Option<u32>,
    pub reset_position: bool,
    pub hide: bool,
    pub exit: bool,
    pub reset: bool,
    pub commit: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SettingsPage {
    Appearance,
    Behavior,
    Reminders,
    Todos,
}

impl SettingsPage {
    fn index(self) -> usize {
        match self {
            Self::Appearance => 0,
            Self::Behavior => 1,
            Self::Reminders => 2,
            Self::Todos => 3,
        }
    }

    fn from_index(index: usize) -> Self {
        match index {
            1 => Self::Behavior,
            2 => Self::Reminders,
            3 => Self::Todos,
            _ => Self::Appearance,
        }
    }
}

pub struct SettingsPanel {
    hwnd: HWND,
    surface: LayeredSurface,
    canvas: EguiCanvas,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    raw_pixels: Vec<u8>,
    present_pixels: Vec<u8>,
    width: u32,
    height: u32,
    dpi: f32,
    open: bool,
    open_amount: f32,
    visible: bool,
    final_x: i32,
    final_y: i32,
    spit_x: f32,
    spit_y: f32,
    lens_radius: f32,
    last_now: f64,
    events: Vec<egui::Event>,
    pointer: Option<Pos2>,
    reset_armed_until: Option<f64>,
    page: SettingsPage,
    custom_minutes: u32,
    alarm_hour: u32,
    alarm_minute: u32,
    alarm_daily: bool,
    todo_draft: String,
    note_draft: String,
    accent_draft: String,
    cache_valid: bool,
    perf: bool,
    prewarming: bool,
}

struct PanelUi<'a> {
    settings: &'a Settings,
    now: f64,
    accent: Color32,
    reset_armed: &'a mut Option<f64>,
    page: &'a mut SettingsPage,
    custom_minutes: &'a mut u32,
    alarm_hour: &'a mut u32,
    alarm_minute: &'a mut u32,
    alarm_daily: &'a mut bool,
    todo_draft: &'a mut String,
    note_draft: &'a mut String,
    accent_draft: &'a mut String,
}

impl SettingsPanel {
    pub fn new(
        hwnd: HWND,
        dpi: f32,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    ) -> Result<Self, String> {
        let width = (PANEL_WIDTH_DIP * dpi).round().max(64.0) as u32;
        let height = (PANEL_HEIGHT_DIP * dpi).round().max(64.0) as u32;
        let surface = LayeredSurface::new(hwnd, width as i32, height as i32)?;
        let canvas = EguiCanvas::new(&device, width, height, dpi);
        log::info(&format!(
            "settings panel created {width}x{height} @ {dpi:.2}x"
        ));
        Ok(Self {
            hwnd,
            surface,
            canvas,
            device,
            queue,
            raw_pixels: vec![0; (width * height * 4) as usize],
            present_pixels: vec![0; (width * height * 4) as usize],
            width,
            height,
            dpi,
            open: false,
            open_amount: 0.0,
            visible: false,
            final_x: 0,
            final_y: 0,
            spit_x: 0.0,
            spit_y: 0.0,
            lens_radius: 52.0,
            last_now: 0.0,
            events: Vec::new(),
            pointer: None,
            reset_armed_until: None,
            page: SettingsPage::Appearance,
            custom_minutes: 25,
            alarm_hour: 9,
            alarm_minute: 0,
            alarm_daily: true,
            todo_draft: String::new(),
            note_draft: String::new(),
            accent_draft: "#FFE8D2".to_owned(),
            cache_valid: false,
            perf: std::env::var_os("SINGULARITY_PANEL_PERF").is_some(),
            prewarming: false,
        })
    }

    pub fn hwnd(&self) -> HWND {
        self.hwnd
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_active(&self) -> bool {
        self.open || self.open_amount > 0.001
    }

    pub fn sync_settings(&mut self, settings: &Settings) {
        self.accent_draft = settings.accent.clone();
    }

    pub fn set_open(&mut self, open: bool, now: f64) {
        if open == self.open {
            return;
        }
        self.open = open;
        self.cache_valid = false;
        if open {
            self.open_amount = 0.0;
            self.events.clear();
            self.last_now = now;
            self.page = SettingsPage::Appearance;
        } else {
            self.events.clear();
            self.reset_armed_until = None;
        }
    }

    pub fn show_reminders_page(&mut self) {
        self.page = SettingsPage::Reminders;
    }

    pub fn text_input(&mut self, text: &str) {
        if !text.is_empty() {
            self.events.push(egui::Event::Text(text.to_owned()));
        }
    }

    pub fn key_input(&mut self, vk: u16, pressed: bool) {
        let key = match vk {
            0x08 => egui::Key::Backspace,
            0x09 => egui::Key::Tab,
            0x0D => egui::Key::Enter,
            0x1B => egui::Key::Escape,
            0x23 => egui::Key::End,
            0x24 => egui::Key::Home,
            0x25 => egui::Key::ArrowLeft,
            0x26 => egui::Key::ArrowUp,
            0x27 => egui::Key::ArrowRight,
            0x28 => egui::Key::ArrowDown,
            0x2E => egui::Key::Delete,
            _ => return,
        };
        let down = |key: u16| unsafe { GetKeyState(key as i32) as u16 & 0x8000 != 0 };
        let ctrl = down(VK_CONTROL.0);
        let modifiers = egui::Modifiers {
            alt: down(VK_MENU.0),
            ctrl,
            shift: down(VK_SHIFT.0),
            mac_cmd: false,
            command: ctrl,
        };
        self.events.push(egui::Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
    }

    pub fn set_layout(
        &mut self,
        final_x: i32,
        final_y: i32,
        spit_x: f32,
        spit_y: f32,
        lens_radius: f32,
    ) {
        self.final_x = final_x;
        self.final_y = final_y;
        self.spit_x = spit_x;
        self.spit_y = spit_y;
        self.lens_radius = lens_radius.max(1.0);
    }

    pub fn pointer_moved(&mut self, client_x: f32, client_y: f32) {
        let pos = Pos2::new(client_x / self.dpi, client_y / self.dpi);
        if self.pointer != Some(pos) {
            self.pointer = Some(pos);
            self.events.push(egui::Event::PointerMoved(pos));
        }
    }

    pub fn pointer_gone(&mut self) {
        if self.pointer.is_some() {
            self.pointer = None;
            self.events.push(egui::Event::PointerGone);
        }
    }

    pub fn pointer_button(&mut self, client_x: f32, client_y: f32, pressed: bool) {
        let pos = Pos2::new(client_x / self.dpi, client_y / self.dpi);
        self.pointer = Some(pos);
        self.events.push(egui::Event::PointerMoved(pos));
        self.events.push(egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        });
    }

    pub fn mouse_wheel(&mut self, delta_y: f32) {
        self.events.push(egui::Event::MouseWheel {
            unit: egui::MouseWheelUnit::Point,
            delta: Vec2::new(0.0, delta_y / self.dpi),
            modifiers: egui::Modifiers::default(),
        });
    }

    pub fn contains_screen(&self, x: i32, y: i32) -> bool {
        if !self.visible {
            return false;
        }
        let (left, top) = self.current_position();
        x >= left && x < left + self.width as i32 && y >= top && y < top + self.height as i32
    }

    fn current_position(&self) -> (i32, i32) {
        let t = ui::ease_out(self.open_amount);
        let start_x = self.spit_x - self.width as f32 * 0.5;
        let start_y = self.spit_y - self.height as f32 * 0.5;
        let x = start_x + (self.final_x as f32 - start_x) * t;
        let y = start_y + (self.final_y as f32 - start_y) * t;
        (x.round() as i32, y.round() as i32)
    }

    pub fn prewarm(&mut self, settings: &Settings, now: f64) {
        self.prewarming = true;
        self.open = true;
        self.open_amount = 1.0;
        self.last_now = now;
        self.cache_valid = false;
        self.visible = false;
        let _ = self.render(settings, now);
        self.prewarming = false;
        self.open = false;
        self.open_amount = 0.0;
        self.last_now = 0.0;
        self.visible = false;
        self.cache_valid = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn render(&mut self, settings: &Settings, now: f64) -> Option<PanelActions> {
        if !self.is_active() {
            return None;
        }
        let frame_start = Instant::now();
        let target = if self.open { 1.0 } else { 0.0 };
        let dt = if self.last_now > 0.0 {
            (now - self.last_now).clamp(0.0, 0.25) as f32
        } else {
            1.0 / 60.0
        };
        self.last_now = now;
        let speed = if target > self.open_amount {
            1.0 / OPEN_SECONDS
        } else {
            1.0 / CLOSE_SECONDS
        };
        if self.open_amount < target {
            self.open_amount = (self.open_amount + dt * speed).min(target);
        } else if self.open_amount > target {
            self.open_amount = (self.open_amount - dt * speed).max(target);
        }

        if !self.open && self.open_amount <= 0.001 {
            if self.visible {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_HIDE);
                }
                self.visible = false;
            }
            self.cache_valid = false;
            return None;
        }

        let animating = self.open_amount < 0.999 || !self.open;
        if animating && self.cache_valid {
            self.events.clear();
            let scale = if self.open_amount > 0.8 {
                1.0
            } else {
                0.12 + 0.88 * ui::ease_out(self.open_amount)
            };
            let alpha = if self.open_amount > 0.8 {
                1.0
            } else {
                ui::ease_out(self.open_amount)
            };
            self.compose_present(scale, alpha, settings.lens_strength, self.open_amount);
            let (x, y) = self.current_position();
            if let Err(error) = self.surface.update(&self.present_pixels, x, y) {
                log::error(&format!("panel present failed: {error}"));
            }
            if !self.visible && !self.prewarming {
                unsafe {
                    let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
                }
                self.visible = true;
            }
            if self.perf {
                log::info(&format!(
                    "panel cached frame {:.2}ms open={:.2} scale={:.2}",
                    frame_start.elapsed().as_secs_f32() * 1000.0,
                    self.open_amount,
                    scale
                ));
            }
            return Some(PanelActions::default());
        }

        let accent = ui::hex_color(&settings.accent);
        self.canvas.ctx().style_mut(|style| {
            style.visuals.selection.bg_fill = accent;
            style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
            style.visuals.widgets.active.bg_fill = accent;
            style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, accent);
            style.visuals.text_cursor.stroke = Stroke::new(1.5_f32, accent);
        });
        let mut actions = PanelActions::default();
        let raw = self
            .canvas
            .raw_input(now, dt, std::mem::take(&mut self.events));
        let mut ui_state = PanelUi {
            settings,
            now,
            accent,
            reset_armed: &mut self.reset_armed_until,
            page: &mut self.page,
            custom_minutes: &mut self.custom_minutes,
            alarm_hour: &mut self.alarm_hour,
            alarm_minute: &mut self.alarm_minute,
            alarm_daily: &mut self.alarm_daily,
            todo_draft: &mut self.todo_draft,
            note_draft: &mut self.note_draft,
            accent_draft: &mut self.accent_draft,
        };
        let canvas = &mut self.canvas;
        let pixels = canvas.render(&self.device, &self.queue, raw, |ctx| {
            build_ui(ctx, &mut ui_state, &mut actions);
        });
        self.raw_pixels.copy_from_slice(pixels);
        self.cache_valid = true;

        let scale = if self.open_amount > 0.8 {
            1.0
        } else {
            0.12 + 0.88 * ui::ease_out(self.open_amount)
        };
        let alpha = if self.open_amount > 0.8 {
            1.0
        } else {
            ui::ease_out(self.open_amount)
        };
        self.compose_present(scale, alpha, settings.lens_strength, self.open_amount);
        let (x, y) = self.current_position();
        if let Err(error) = self.surface.update(&self.present_pixels, x, y) {
            log::error(&format!("panel present failed: {error}"));
        }
        if !self.visible && !self.prewarming {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.visible = true;
        }
        if !self.open && self.open_amount <= 0.001 {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
            self.cache_valid = false;
        }
        if self.perf {
            log::info(&format!(
                "panel render frame {:.2}ms open={:.2} scale={:.2}",
                frame_start.elapsed().as_secs_f32() * 1000.0,
                self.open_amount,
                scale
            ));
        }
        Some(actions)
    }

    fn compose_present(&mut self, scale: f32, alpha: f32, lens_strength: f32, open_amount: f32) {
        if scale >= 0.999 && alpha >= 0.999 {
            self.present_pixels.copy_from_slice(&self.raw_pixels);
            return;
        }
        let width = self.width as usize;
        let height = self.height as usize;
        let cx = width as f32 * 0.5;
        let cy = height as f32 * 0.5;
        let inv = 1.0 / scale.max(0.01);
        let (win_x, win_y) = self.current_position();
        let warp_amount = (1.0 - ui::smoothstep(0.20, 0.82, open_amount)).clamp(0.0, 1.0);
        self.present_pixels.fill(0);
        let x0 = (cx * (1.0 - scale)).floor().max(0.0) as usize;
        let x1 = (cx + (width as f32 - cx) * scale).ceil().min(width as f32) as usize;
        let y0 = (cy * (1.0 - scale)).floor().max(0.0) as usize;
        let y1 = (cy + (height as f32 - cy) * scale)
            .ceil()
            .min(height as f32) as usize;
        for y in y0..y1 {
            for x in x0..x1 {
                let mut screen_x = win_x as f32 + x as f32 + 0.5;
                let mut screen_y = win_y as f32 + y as f32 + 0.5;
                if lens_strength > 0.001 && warp_amount > 0.001 {
                    let dx = screen_x - self.spit_x;
                    let dy = screen_y - self.spit_y;
                    let len = (dx * dx + dy * dy).sqrt();
                    if len > 0.001 {
                        let d = len / self.lens_radius.max(1.0);
                        if d < 3.05 {
                            let dir_x = dx / len;
                            let dir_y = dy / len;
                            let falloff = 1.0 - ui::smoothstep(0.8, 3.05, d);
                            let bend = lens_strength * self.lens_radius * 0.42 / d.max(0.35)
                                * falloff
                                * warp_amount;
                            let swirl = lens_strength
                                * self.lens_radius
                                * 0.10
                                * (1.0 - ui::smoothstep(0.8, 2.6, d))
                                / d.max(0.7)
                                * warp_amount;
                            screen_x += dir_x * bend - dir_y * swirl;
                            screen_y += dir_y * bend + dir_x * swirl;
                        }
                    }
                }
                let sx = (cx + (screen_x - win_x as f32 - cx) * inv).round() as isize;
                let sy = (cy + (screen_y - win_y as f32 - cy) * inv).round() as isize;
                if sx < 0 || sy < 0 || sx >= width as isize || sy >= height as isize {
                    continue;
                }
                let source = (sy as usize * width + sx as usize) * 4;
                let o = (y * width + x) * 4;
                self.present_pixels[o] = (self.raw_pixels[source] as f32 * alpha) as u8;
                self.present_pixels[o + 1] = (self.raw_pixels[source + 1] as f32 * alpha) as u8;
                self.present_pixels[o + 2] = (self.raw_pixels[source + 2] as f32 * alpha) as u8;
                self.present_pixels[o + 3] = (self.raw_pixels[source + 3] as f32 * alpha) as u8;
            }
        }
    }
}

fn build_ui(ctx: &egui::Context, state: &mut PanelUi<'_>, actions: &mut PanelActions) {
    egui::Area::new(egui::Id::new("singularity-settings-card"))
        .fixed_pos(Pos2::new(CARD_MARGIN, CARD_MARGIN))
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(BG)
                .stroke(Stroke::new(1.0_f32, ui::STROKE))
                .rounding(Rounding::same(CARD_ROUNDING))
                .inner_margin(Margin::same(CARD_INNER))
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 7.0),
                    blur: 18.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(140),
                })
                .show(ui, |ui| {
                    ui.set_width(CONTENT_WIDTH);
                    ui.set_min_height(PANEL_HEIGHT_DIP - CARD_MARGIN * 2.0 - CARD_INNER * 2.0);
                    ui::top_accent(ui, state.accent);
                    header(ui, state.now, state.accent, actions);
                    ui.add_space(9.0);
                    let selected = state.page.index();
                    if let Some(index) = ui::tab_bar(
                        ui,
                        &["外观", "行为", "提醒", "待办"],
                        selected,
                        state.accent,
                    ) {
                        *state.page = SettingsPage::from_index(index);
                    }
                    ui.add_space(11.0);
                    ui::hairline(ui);
                    ui.add_space(7.0);
                    egui::ScrollArea::vertical()
                        .scroll_bar_visibility(
                            egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded,
                        )
                        .auto_shrink([false, false])
                        .max_height(SCROLL_HEIGHT)
                        .show(ui, |ui| match *state.page {
                            SettingsPage::Appearance => appearance(ui, state, actions),
                            SettingsPage::Behavior => behavior(ui, state, actions),
                            SettingsPage::Reminders => reminders_page(ui, state, actions),
                            SettingsPage::Todos => todos_page(ui, state, actions),
                        });
                    ui.add_space(5.0);
                    footer(ui, state, actions);
                });
        });
}

fn header(ui: &mut egui::Ui, now: f64, accent: Color32, actions: &mut PanelActions) {
    ui.horizontal(|ui| {
        ui::orb(ui, 40.0, accent, now);
        ui.add_space(5.0);
        ui.vertical(|ui| {
            ui.add_space(2.0);
            ui.label(RichText::new("奇点").size(19.0).strong().color(ui::TEXT));
            ui.label(
                RichText::new("SINGULARITY · 黑洞桌宠")
                    .size(8.8)
                    .color(ui::DIM),
            );
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui::close_button(ui, ui::MUTED, 26.0).clicked() {
                actions.close = true;
            }
            let (badge, _) = ui.allocate_exact_size(Vec2::new(46.0, 20.0), Sense::hover());
            let painter = ui.painter();
            painter.rect_filled(badge, Rounding::same(6.0), Color32::from_white_alpha(6));
            painter.text(
                badge.center(),
                Align2::CENTER_CENTER,
                format!("v{}", env!("CARGO_PKG_VERSION")),
                FontId::proportional(8.3),
                ui::DIM,
            );
        });
    });
}

fn appearance(ui: &mut egui::Ui, state: &mut PanelUi<'_>, actions: &mut PanelActions) {
    let accent = state.accent;
    let settings = state.settings;
    ui::section_header(ui, "黑洞", accent);
    let mut radius = settings.radius_dip;
    ui::value_row(ui, "半径", &format!("{radius:.0} DIP"), accent);
    let response = ui::accent_slider(ui, &mut radius, 28.0, 92.0, accent, true);
    if response.changed() {
        actions.radius_dip = Some(radius);
    }
    if response.drag_stopped() || response.clicked() {
        actions.commit = true;
    }

    let mut lens = settings.lens_strength;
    ui::value_row(ui, "桌面透镜", &format!("{lens:.2}×"), accent);
    let response = ui::accent_slider(ui, &mut lens, 0.0, 2.0, accent, true);
    if response.changed() {
        actions.lens_strength = Some(lens);
    }
    if response.drag_stopped() || response.clicked() {
        actions.commit = true;
    }
    ui::hint(ui, "0 为平直桌面，数值越高空间弯曲越明显");

    ui::section_header(ui, "吸积光色", accent);
    accent_presets(ui, settings, accent, actions);
    ui.add_space(5.0);
    ui::value_row(ui, "自定义颜色", &settings.accent, accent);
    ui.horizontal(|ui| {
        let response = ui.add_sized(
            [122.0, 28.0],
            egui::TextEdit::singleline(state.accent_draft)
                .hint_text("#RRGGBB")
                .font(FontId::proportional(11.0))
                .margin(Margin::symmetric(8.0, 6.0))
                .vertical_align(Align::Center)
                .text_color(TEXT)
                .desired_width(122.0),
        );
        let apply = ui::pill_button(ui, "应用", accent, false, 52.0);
        let enter = response.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        if (apply.clicked() || enter) && settings::parse_accent_hex(state.accent_draft).is_some() {
            actions.accent = Some(state.accent_draft.trim().to_owned());
            actions.commit = true;
        }
    });
    ui::hint(ui, "输入 #RRGGBB；非法颜色不会保存");
}

fn accent_presets(
    ui: &mut egui::Ui,
    settings: &Settings,
    accent: Color32,
    actions: &mut PanelActions,
) {
    const PRESETS: [(&str, &str); 8] = [
        ("#FFE8D2", "暖白"),
        ("#FFB36B", "暖金"),
        ("#FF8A3A", "琥珀"),
        ("#FFD166", "日冕"),
        ("#FF6B8A", "玫瑰"),
        ("#B388FF", "紫罗兰"),
        ("#66E0FF", "冰蓝"),
        ("#6CFFB0", "翡翠"),
    ];
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 5.0;
        for (hex, name) in PRESETS {
            let color = ui::hex_color(hex);
            let selected = settings.accent.eq_ignore_ascii_case(hex);
            let (rect, response) = ui.allocate_exact_size(Vec2::splat(25.0), Sense::click());
            let center = rect.center();
            let clicked = response.clicked();
            if selected {
                ui.painter()
                    .circle_stroke(center, 11.5, Stroke::new(1.4_f32, Color32::WHITE));
            } else if response.hovered() {
                ui.painter().circle_stroke(
                    center,
                    11.5,
                    Stroke::new(1.0_f32, ui::alpha(accent, 140)),
                );
            }
            ui.painter().circle_filled(center, 8.0, color);
            if selected {
                ui.painter()
                    .circle_filled(center, 2.0, Color32::from_white_alpha(190));
            }
            response.on_hover_text(name);
            if clicked {
                actions.accent = Some(hex.to_owned());
                actions.commit = true;
            }
        }
    });
}

fn behavior(ui: &mut egui::Ui, state: &mut PanelUi<'_>, actions: &mut PanelActions) {
    let accent = state.accent;
    let settings = state.settings;
    ui::section_header(ui, "漫游", accent);
    let mut wander = settings.wander;
    if ui::toggle_row(ui, "自由漫游", &mut wander, accent) {
        actions.wander = Some(wander);
        actions.commit = true;
    }
    let mut speed = settings.wander_speed;
    ui::value_row(
        ui,
        "漫游速度",
        &format!("{speed:.0}"),
        if wander { accent } else { ui::DIM },
    );
    let response = ui::accent_slider(ui, &mut speed, 0.0, 120.0, accent, wander);
    if response.changed() {
        actions.wander_speed = Some(speed);
    }
    if response.drag_stopped() || response.clicked() {
        actions.commit = true;
    }
    let range = match settings.custom_area {
        Some([_, _, width, height]) => format!("自定义 {width}×{height}"),
        None => "整个工作区".to_owned(),
    };
    ui::value_row(ui, "漫游范围", &range, MUTED);
    ui.horizontal(|ui| {
        if ui::pill_button(ui, "整个工作区", accent, false, 143.0).clicked() {
            actions.set_work_area = true;
        }
        if ui::pill_button(ui, "框选区域", accent, false, 143.0).clicked() {
            actions.select_region = true;
        }
    });

    ui::section_header(ui, "窗口", accent);
    let mut paused = settings.paused;
    if ui::toggle_row(ui, "暂停动画", &mut paused, accent) {
        actions.paused = Some(paused);
        actions.commit = true;
    }
    let mut fullscreen = settings.fullscreen_hide;
    if ui::toggle_row(ui, "全屏时自动隐藏", &mut fullscreen, accent) {
        actions.fullscreen_hide = Some(fullscreen);
        actions.commit = true;
    }
    ui::hint(ui, "双击核心也可以切换漫游，拖动后从新位置继续。");

    ui::section_header(ui, "桌面互动", accent);
    let mut fun_enabled = settings.fun_enabled;
    if ui::toggle_row(ui, "自动吸收桌面图标", &mut fun_enabled, accent) {
        actions.fun_enabled = Some(fun_enabled);
        actions.commit = true;
    }
    let mut interval = settings.fun_interval_seconds as f32;
    ui::value_row(
        ui,
        "吸收间隔",
        &format!("约 {} 秒", settings.fun_interval_seconds),
        if fun_enabled { accent } else { ui::DIM },
    );
    let response = ui::accent_slider(ui, &mut interval, 30.0, 300.0, accent, fun_enabled);
    if response.changed() {
        actions.fun_interval_seconds = Some(interval.round().clamp(30.0, 300.0) as u32);
    }
    if response.drag_stopped() || response.clicked() {
        actions.commit = true;
    }
    ui::hint(ui, "30 秒为高频，300 秒为低频；实际间隔有 ±18% 抖动。");
    if ui::pill_button(ui, "演示吸收", accent, true, 84.0).clicked() {
        actions.fun_demo = true;
    }
    ui::hint(ui, "开启后按间隔自动吸收；演示按钮始终可用。");
    ui::hint(ui, "引力波圆环为默认效果，会自行扩散。");
}

fn reminders_page(ui: &mut egui::Ui, state: &mut PanelUi<'_>, actions: &mut PanelActions) {
    let accent = state.accent;
    let settings = state.settings;
    let now = unix_now();
    ui::section_header(ui, "专注", accent);
    if let Some(until) = settings.focus_until_unix.filter(|until| *until > now) {
        let remaining = until - now;
        let total = (settings.focus_minutes.max(1) as f32) * 60.0;
        let progress = 1.0 - (remaining as f32 / total.max(1.0)).clamp(0.0, 1.0);
        egui::Frame::none()
            .fill(Color32::from_white_alpha(4))
            .stroke(Stroke::new(1.0_f32, ui::alpha(accent, 45)))
            .rounding(Rounding::same(12.0))
            .inner_margin(Margin::symmetric(12.0, 10.0))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(RichText::new("专注进行中").size(9.5).color(ui::MUTED));
                        ui.label(
                            RichText::new(format!("{} 分钟", remaining.div_ceil(60)))
                                .size(21.0)
                                .strong()
                                .color(accent),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui::pill_button(ui, "结束专注", accent, false, 76.0).clicked() {
                            actions.stop_focus = true;
                        }
                    });
                });
                ui.add_space(6.0);
                ui::progress_bar(ui, progress, accent, 4.0);
            });
        ui::hint(ui, "黑洞会随专注缓慢呼吸，结束时用托盘气泡提醒。");
    } else {
        ui.horizontal(|ui| {
            for minutes in [25u32, 45, 60] {
                if ui::pill_button(ui, &format!("专注 {minutes} 分"), accent, false, 93.0).clicked()
                {
                    actions.start_focus = Some(minutes);
                }
            }
        });
        ui::hint(ui, "开始时黑洞进入缓慢呼吸，结束提醒会出现在提醒列表。");
    }

    ui::section_header(ui, "快速提醒", accent);
    ui::hint(ui, "到点后通过托盘气泡提醒，不打断当前应用。");
    let mut sound = settings.reminder_sound;
    if ui::toggle_row(ui, "提示音", &mut sound, accent) {
        actions.set_reminder_sound = Some(sound);
    }
    ui.horizontal(|ui| {
        for minutes in [5u32, 15, 30, 60] {
            if ui::pill_button(ui, &format!("+{minutes} 分"), accent, false, 68.0).clicked() {
                actions.add_reminder = Some(minutes);
            }
        }
    });
    ui.add_space(3.0);
    ui::value_row(
        ui,
        "自定义提醒",
        &format!("{} 分钟后", *state.custom_minutes),
        accent,
    );
    let mut minutes = *state.custom_minutes as f32;
    let response = ui::accent_slider(ui, &mut minutes, 1.0, 180.0, accent, true);
    if response.changed() {
        *state.custom_minutes = minutes.round().clamp(1.0, 180.0) as u32;
    }
    if ui::pill_button(ui, "添加提醒", accent, true, 96.0).clicked() {
        actions.add_reminder = Some(*state.custom_minutes);
        actions.commit = true;
    }

    ui::section_header(ui, "时钟闹钟", accent);
    ui::value_row(
        ui,
        "时间",
        &format!("{:02}:{:02}", *state.alarm_hour, *state.alarm_minute),
        accent,
    );
    ui.horizontal(|ui| {
        ui.label(RichText::new("时").size(10.5).color(MUTED));
        ui.add_sized(
            [48.0, 26.0],
            egui::DragValue::new(state.alarm_hour)
                .range(0..=23)
                .speed(0.25),
        );
        ui.add_space(2.0);
        ui.label(RichText::new("分").size(10.5).color(MUTED));
        ui.add_sized(
            [48.0, 26.0],
            egui::DragValue::new(state.alarm_minute)
                .range(0..=59)
                .speed(0.25),
        );
    });
    let mut daily = *state.alarm_daily;
    if ui::toggle_row(ui, "每天重复", &mut daily, accent) {
        *state.alarm_daily = daily;
    }
    if ui::pill_button(ui, "添加闹钟", accent, true, 96.0).clicked() {
        actions.add_alarm = Some((*state.alarm_hour, *state.alarm_minute, *state.alarm_daily));
    }

    ui::section_header(ui, "待触发", accent);
    if settings.reminders.is_empty() {
        ui::hint(ui, "暂无提醒");
        return;
    }
    let mut list: Vec<_> = settings.reminders.iter().collect();
    list.sort_by_key(|reminder| reminder.due_unix);
    for reminder in list {
        ui.horizontal(|ui| {
            let checked = ui::mini_check(ui, reminder.enabled, accent, true);
            if checked.clicked() {
                actions.set_reminder_enabled = Some((reminder.id, !reminder.enabled));
            }
            let label = if reminder.repeat_daily {
                format!("每天 {}", reminder.label)
            } else {
                reminder.label.clone()
            };
            let color = if reminder.enabled { TEXT } else { ui::DIM };
            ui.label(RichText::new(label).size(11.5).color(color));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui::close_button(ui, ui::MUTED, 22.0).clicked() {
                    actions.remove_reminder = Some(reminder.id);
                }
                let status = if reminder.enabled {
                    remaining_label(reminder.due_unix, now)
                } else {
                    "已关闭".to_owned()
                };
                ui.label(RichText::new(status).size(9.5).color(ui::MUTED));
            });
        });
    }
}

fn todos_page(ui: &mut egui::Ui, state: &mut PanelUi<'_>, actions: &mut PanelActions) {
    let accent = state.accent;
    let settings = state.settings;
    ui::section_header(ui, "便签", accent);
    let width = ui.available_width();
    let note_response = ui.add_sized(
        [width, 56.0],
        egui::TextEdit::multiline(state.note_draft)
            .hint_text("记一点东西…")
            .font(FontId::proportional(11.5))
            .margin(Margin::symmetric(9.0, 7.0))
            .desired_width(width)
            .desired_rows(2),
    );
    let submit_note = note_response.has_focus()
        && ui.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::Enter));
    ui.horizontal(|ui| {
        ui.add_space((ui.available_width() - 64.0).max(0.0));
        let clicked = ui::pill_button(ui, "记下", accent, true, 64.0).clicked();
        if (clicked || submit_note) && !state.note_draft.trim().is_empty() {
            actions.add_note = Some(state.note_draft.trim().to_owned());
            state.note_draft.clear();
        }
    });
    if settings.notes.is_empty() {
        ui::hint(ui, "暂无便签");
    } else {
        for note in &settings.notes {
            egui::Frame::none()
                .fill(Color32::from_white_alpha(4))
                .rounding(Rounding::same(9.0))
                .inner_margin(Margin::symmetric(9.0, 6.0))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::Label::new(RichText::new(&note.text).size(11.0).color(ui::TEXT))
                                .wrap(),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui::close_button(ui, ui::MUTED, 22.0).clicked() {
                                actions.remove_note = Some(note.id);
                            }
                        });
                    });
                });
            ui.add_space(3.0);
        }
    }

    ui::section_header(ui, "待办", accent);
    let mut submit_todo = false;
    ui.horizontal(|ui| {
        let input_width = (ui.available_width() - 83.0).max(120.0);
        let response = ui.add_sized(
            [input_width, 28.0],
            egui::TextEdit::singleline(state.todo_draft)
                .hint_text("添加一条待办…")
                .font(FontId::proportional(11.5))
                .margin(Margin::symmetric(8.0, 6.0))
                .vertical_align(Align::Center)
                .desired_width(input_width),
        );
        submit_todo = response.has_focus() && ui.input(|input| input.key_pressed(egui::Key::Enter));
        if ui::pill_button(ui, "添加", accent, true, 76.0).clicked() {
            submit_todo = true;
        }
    });
    if submit_todo && !state.todo_draft.trim().is_empty() {
        actions.add_todo = Some(state.todo_draft.trim().to_owned());
        state.todo_draft.clear();
    }

    if settings.todos.is_empty() {
        ui::hint(ui, "暂无待办");
    } else {
        for todo in &settings.todos {
            ui.horizontal(|ui| {
                let check = ui::mini_check(ui, todo.done, accent, true);
                if check.clicked() {
                    actions.toggle_todo = Some((todo.id, !todo.done));
                }
                let text = if todo.done {
                    RichText::new(format!("✓ {}", todo.title))
                        .size(11.5)
                        .color(ui::DIM)
                } else {
                    RichText::new(&todo.title).size(11.5).color(TEXT)
                };
                ui.label(text);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui::close_button(ui, ui::MUTED, 22.0).clicked() {
                        actions.remove_todo = Some(todo.id);
                    }
                });
            });
        }
    }
    ui::hint(ui, "待办保存在本地；完成项会保留，可手动删除。");
}

fn footer(ui: &mut egui::Ui, state: &mut PanelUi<'_>, actions: &mut PanelActions) {
    let now = state.now;
    let accent = state.accent;
    ui::hairline(ui);
    ui.add_space(2.0);
    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
        let armed = state.reset_armed.map(|until| now <= until).unwrap_or(false);
        let label = if armed {
            "再次点击确认"
        } else {
            "恢复默认"
        };
        if ui::pill_button(ui, label, accent, armed, 106.0).clicked() {
            if armed {
                actions.reset = true;
                actions.commit = true;
                *state.reset_armed = None;
            } else {
                *state.reset_armed = Some(now + 3.0);
            }
        }
        if ui::text_link(ui, "退出", ui::DANGER, accent).clicked() {
            actions.exit = true;
        }
        if ui::text_link(ui, "隐藏", ui::MUTED, accent).clicked() {
            actions.hide = true;
        }
        if ui::text_link(ui, "重置位置", ui::MUTED, accent).clicked() {
            actions.reset_position = true;
        }
    });
}

fn remaining_label(due_unix: u64, now: u64) -> String {
    let remain = due_unix.saturating_sub(now);
    if remain == 0 {
        "即将提醒".to_owned()
    } else if remain < 60 {
        "1 分钟内".to_owned()
    } else {
        format!("{} 分钟后", remain.div_ceil(60))
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
