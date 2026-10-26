use std::sync::Arc;

use egui::{Align, Color32, Layout, Margin, Pos2, RichText, Rounding, Stroke, Vec2};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    DestroyWindow, SetWindowPos, ShowWindow, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SW_HIDE,
    SW_SHOWNOACTIVATE,
};

use crate::log;
use crate::settings::Settings;
use crate::sysmon::StatsSnapshot;
use crate::ui::{self, EguiCanvas};
use crate::win::{self, LayeredSurface};

pub const WIDTH_DIP: f32 = 304.0;
pub const HEIGHT_DIP: f32 = 364.0;
const OPEN_SECONDS: f32 = 0.95;
const CLOSE_SECONDS: f32 = 0.42;

const BG: Color32 = Color32::from_rgba_premultiplied(12, 16, 23, 250);
const CPU_COLOR: Color32 = Color32::from_rgb(255, 178, 112);
const GPU_COLOR: Color32 = Color32::from_rgb(236, 150, 208);
const RAM_COLOR: Color32 = Color32::from_rgb(126, 206, 255);
const BAT_COLOR: Color32 = Color32::from_rgb(130, 230, 170);
const BAT_LOW: Color32 = Color32::from_rgb(255, 128, 138);

pub enum StatsAction {
    Media(String),
    Close,
    ToggleTodo { id: u32, done: bool },
}

pub struct StatsOverlay {
    hwnd: HWND,
    surface: LayeredSurface,
    canvas: EguiCanvas,
    device: Arc<wgpu::Device>,
    queue: Arc<wgpu::Queue>,
    present_pixels: Vec<u8>,
    width: u32,
    height: u32,
    dpi: f32,
    open: bool,
    open_amount: f32,
    visible: bool,
    final_x: i32,
    final_y: i32,
    hole_x: f32,
    hole_y: f32,
    pet_radius: f32,
    last_now: f64,
    prewarming: bool,
    events: Vec<egui::Event>,
    pointer: Option<Pos2>,
}

impl StatsOverlay {
    pub fn new(
        hwnd: HWND,
        dpi: f32,
        device: Arc<wgpu::Device>,
        queue: Arc<wgpu::Queue>,
    ) -> Result<Self, String> {
        let width = (WIDTH_DIP * dpi).round().max(64.0) as u32;
        let height = (HEIGHT_DIP * dpi).round().max(64.0) as u32;
        win::exclude_from_capture(hwnd);
        let surface = LayeredSurface::new(hwnd, width as i32, height as i32)?;
        let canvas = EguiCanvas::new(&device, width, height, dpi);
        log::info(&format!(
            "stats overlay created {width}x{height} @ {dpi:.2}x"
        ));
        Ok(Self {
            hwnd,
            surface,
            canvas,
            device,
            queue,
            present_pixels: vec![0; (width * height * 4) as usize],
            width,
            height,
            dpi,
            open: false,
            open_amount: 0.0,
            visible: false,
            final_x: 0,
            final_y: 0,
            hole_x: 0.0,
            hole_y: 0.0,
            pet_radius: 52.0,
            last_now: 0.0,
            prewarming: false,
            events: Vec::new(),
            pointer: None,
        })
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    pub fn is_active(&self) -> bool {
        self.open || self.open_amount > 0.001
    }

    pub fn set_open(&mut self, open: bool, now: f64) {
        if self.open == open {
            return;
        }
        self.open = open;
        if open {
            self.open_amount = 0.0;
            self.last_now = now;
        }
    }

    pub fn set_layout(
        &mut self,
        final_x: i32,
        final_y: i32,
        hole_x: f32,
        hole_y: f32,
        pet_radius: f32,
    ) {
        self.final_x = final_x;
        self.final_y = final_y;
        self.hole_x = hole_x;
        self.hole_y = hole_y;
        self.pet_radius = pet_radius;
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
        let start_x = self.hole_x - self.width as f32 * 0.5;
        let start_y = self.hole_y - self.height as f32 * 0.5;
        let x = start_x + (self.final_x as f32 - start_x) * t;
        let y = start_y + (self.final_y as f32 - start_y) * t;
        (x.round() as i32, y.round() as i32)
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

    pub fn prewarm(&mut self, settings: &Settings, now: f64, pet_hwnd: HWND) {
        self.prewarming = true;
        self.open = true;
        self.open_amount = 1.0;
        self.last_now = now;
        self.visible = false;
        let _ = self.render(settings, StatsSnapshot::default(), now, pet_hwnd);
        self.prewarming = false;
        self.open = false;
        self.open_amount = 0.0;
        self.last_now = 0.0;
        self.visible = false;
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
        }
    }

    pub fn render(
        &mut self,
        settings: &Settings,
        stats: StatsSnapshot,
        now: f64,
        pet_hwnd: HWND,
    ) -> Option<StatsAction> {
        if !self.is_active() {
            return None;
        }
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
            return None;
        }

        let accent = ui::hex_color(&settings.accent);
        self.canvas.ctx().style_mut(|style| {
            style.visuals.selection.bg_fill = accent;
            style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, accent);
            style.visuals.widgets.active.bg_fill = accent;
            style.visuals.text_cursor.stroke = Stroke::new(1.5_f32, accent);
        });
        let raw = self
            .canvas
            .raw_input(now, dt, std::mem::take(&mut self.events));
        let progress = ((self.open_amount * 1.35) - 0.35).clamp(0.0, 1.0);
        let mut action = None;
        let canvas = &mut self.canvas;
        canvas.render(&self.device, &self.queue, raw, |ctx| {
            build_ui(ctx, accent, stats, progress, now, settings, &mut action);
        });

        let scale = if self.open_amount > 0.8 {
            1.0
        } else {
            0.28 + 0.72 * ui::ease_out(self.open_amount)
        };
        let alpha = if self.open_amount > 0.8 {
            1.0
        } else {
            ui::ease_out(self.open_amount)
        };
        self.compose_present(scale, alpha, settings.lens_strength);
        let (x, y) = self.current_position();
        if let Err(error) = self.surface.update(&self.present_pixels, x, y) {
            log::error(&format!("stats present failed: {error}"));
        }
        if !self.visible && !self.prewarming {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            }
            self.visible = true;
        }
        unsafe {
            let _ = SetWindowPos(
                self.hwnd,
                pet_hwnd,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        action
    }

    fn compose_present(&mut self, scale: f32, alpha: f32, lens_strength: f32) {
        let width = self.width as usize;
        let height = self.height as usize;
        let cx = width as f32 * 0.5;
        let cy = height as f32 * 0.5;
        let inv = 1.0 / scale.max(0.01);
        let (win_x, win_y) = self.current_position();
        self.present_pixels.fill(0);
        let raw = self.canvas.pixels();
        for y in 0..height {
            for x in 0..width {
                let screen_x = win_x as f32 + x as f32 + 0.5;
                let screen_y = win_y as f32 + y as f32 + 0.5;
                let dx = screen_x - self.hole_x;
                let dy = screen_y - self.hole_y;
                let len = (dx * dx + dy * dy).sqrt();
                let (mut src_screen_x, mut src_screen_y) = (screen_x, screen_y);
                if lens_strength > 0.001 && len > 0.001 {
                    let d = len / self.pet_radius.max(1.0);
                    if d < 3.05 {
                        let dir_x = dx / len;
                        let dir_y = dy / len;
                        let falloff = 1.0 - ui::smoothstep(0.8, 3.05, d);
                        let bend = lens_strength * self.pet_radius * 0.42 / d.max(0.35) * falloff;
                        let swirl = lens_strength
                            * self.pet_radius
                            * 0.10
                            * (1.0 - ui::smoothstep(0.8, 2.6, d))
                            / d.max(0.7);
                        src_screen_x += dir_x * bend - dir_y * swirl;
                        src_screen_y += dir_y * bend + dir_x * swirl;
                    }
                }
                let local_x = cx + (src_screen_x - win_x as f32 - cx) * inv;
                let local_y = cy + (src_screen_y - win_y as f32 - cy) * inv;
                let Some(pixel) = sample_bilinear(raw, width, height, local_x, local_y) else {
                    continue;
                };
                let o = (y * width + x) * 4;
                self.present_pixels[o] = (pixel[0] as f32 * alpha) as u8;
                self.present_pixels[o + 1] = (pixel[1] as f32 * alpha) as u8;
                self.present_pixels[o + 2] = (pixel[2] as f32 * alpha) as u8;
                self.present_pixels[o + 3] = (pixel[3] as f32 * alpha) as u8;
            }
        }
    }
}

impl Drop for StatsOverlay {
    fn drop(&mut self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_HIDE);
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

fn build_ui(
    ctx: &egui::Context,
    accent: Color32,
    stats: StatsSnapshot,
    progress: f32,
    now: f64,
    settings: &Settings,
    action: &mut Option<StatsAction>,
) {
    egui::Area::new(egui::Id::new("singularity-stats-card"))
        .fixed_pos(Pos2::ZERO)
        .show(ctx, |ui| {
            egui::Frame::none()
                .fill(BG)
                .stroke(Stroke::new(1.0_f32, Color32::from_white_alpha(12)))
                .rounding(Rounding::same(16.0))
                .inner_margin(Margin::symmetric(14.0, 12.0))
                .shadow(egui::epaint::Shadow {
                    offset: Vec2::new(0.0, 6.0),
                    blur: 20.0,
                    spread: 0.0,
                    color: Color32::from_black_alpha(140),
                })
                .show(ui, |ui| {
                    ui.set_width(WIDTH_DIP - 28.0);
                    ui.spacing_mut().item_spacing.y = 4.0;
                    ui::top_accent(ui, accent);
                    ui.horizontal(|ui| {
                        ui::status_dot(ui, accent, now);
                        ui.label(
                            RichText::new("系统状态")
                                .size(13.0)
                                .strong()
                                .color(ui::TEXT),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui::close_button(ui, ui::MUTED, 18.0)
                                .on_hover_text("关闭")
                                .clicked()
                            {
                                *action = Some(StatsAction::Close);
                            }
                            ui.add_space(2.0);
                            ui.label(RichText::new("1s").size(8.8).color(ui::DIM));
                            ui.add_space(4.0);
                            ui.label(RichText::new("实时").size(8.8).color(ui::MUTED));
                            ui::status_dot(ui, ui::OK, now);
                        });
                    });
                    ui.add_space(1.0);
                    ui::hairline(ui);
                    ui.add_space(2.0);
                    ui::stat_bar(ui, "CPU", stats.cpu, CPU_COLOR, progress, None);
                    ui::stat_bar(
                        ui,
                        "GPU",
                        stats.gpu.unwrap_or(0.0),
                        GPU_COLOR,
                        progress,
                        if stats.gpu.is_some() {
                            None
                        } else {
                            Some("—")
                        },
                    );
                    ui::stat_bar(ui, "内存", stats.ram, RAM_COLOR, progress, None);
                    let battery = stats.battery;
                    let color = battery
                        .map(|value| if value < 20.0 { BAT_LOW } else { BAT_COLOR })
                        .unwrap_or(ui::MUTED);
                    ui::stat_bar(
                        ui,
                        if battery.is_some() {
                            "电池"
                        } else {
                            "电源"
                        },
                        battery.unwrap_or(0.0),
                        color,
                        progress,
                        if battery.is_some() {
                            None
                        } else {
                            Some(if stats.charging { "AC" } else { "—" })
                        },
                    );
                    if stats.charging && battery.is_some() {
                        ui.horizontal(|ui| {
                            ui.add_space(3.0);
                            ui::status_dot(ui, BAT_COLOR, now);
                            ui.label(RichText::new("正在充电").size(9.2).color(BAT_COLOR));
                        });
                    }
                    ui.add_space(4.0);
                    ui::hairline(ui);
                    ui.add_space(3.0);
                    todo_section(ui, settings, accent, action);
                    ui.add_space(4.0);
                    ui::hairline(ui);
                    ui.add_space(3.0);
                    let button_width = (ui.available_width() - 24.0) / 4.0;
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 8.0;
                        for (icon, action_name, tip) in [
                            (ui::MediaIcon::Previous, "previous", "上一首"),
                            (ui::MediaIcon::Play, "play_pause", "播放 / 暂停"),
                            (ui::MediaIcon::Next, "next", "下一首"),
                            (ui::MediaIcon::Mute, "mute", "静音"),
                        ] {
                            if ui::media_button(ui, icon, accent, button_width)
                                .on_hover_text(tip)
                                .clicked()
                            {
                                *action = Some(StatsAction::Media(action_name.to_owned()));
                            }
                        }
                    });
                });
        });
}

fn todo_section(
    ui: &mut egui::Ui,
    settings: &Settings,
    accent: Color32,
    action: &mut Option<StatsAction>,
) {
    let pending_count = settings.todos.iter().filter(|todo| !todo.done).count();
    ui.horizontal(|ui| {
        ui.label(RichText::new("待办").size(9.0).color(ui::MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if pending_count > 0 {
                ui.label(
                    RichText::new(format!("{pending_count} 项"))
                        .size(8.5)
                        .color(ui::DIM),
                );
            }
        });
    });
    if pending_count == 0 {
        let text = if settings.todos.is_empty() {
            "暂无待办"
        } else {
            "全部完成"
        };
        ui.label(RichText::new(text).size(10.0).color(ui::DIM));
        return;
    }
    for todo in settings.todos.iter().filter(|todo| !todo.done).take(3) {
        ui.horizontal(|ui| {
            if ui::mini_check(ui, false, accent, true).clicked() {
                *action = Some(StatsAction::ToggleTodo {
                    id: todo.id,
                    done: true,
                });
            }
            ui.add(
                egui::Label::new(RichText::new(&todo.title).size(10.5).color(ui::TEXT)).truncate(),
            );
        });
    }
}

fn sample_bilinear(pixels: &[u8], width: usize, height: usize, x: f32, y: f32) -> Option<[u8; 4]> {
    if x < 0.0 || y < 0.0 || x > (width - 1) as f32 || y > (height - 1) as f32 {
        return None;
    }
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let p00 = &pixels[(y0 * width + x0) * 4..(y0 * width + x0) * 4 + 4];
    let p10 = &pixels[(y0 * width + x1) * 4..(y0 * width + x1) * 4 + 4];
    let p01 = &pixels[(y1 * width + x0) * 4..(y1 * width + x0) * 4 + 4];
    let p11 = &pixels[(y1 * width + x1) * 4..(y1 * width + x1) * 4 + 4];
    let mut result = [0u8; 4];
    for channel in 0..4 {
        let top = p00[channel] as f32 * (1.0 - tx) + p10[channel] as f32 * tx;
        let bottom = p01[channel] as f32 * (1.0 - tx) + p11[channel] as f32 * tx;
        result[channel] = (top * (1.0 - ty) + bottom * ty).round().clamp(0.0, 255.0) as u8;
    }
    Some(result)
}
