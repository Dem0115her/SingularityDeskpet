use egui::{
    Align, Align2, Color32, FontData, FontDefinitions, FontFamily, FontId, Layout, Pos2, Rect,
    Response, RichText, Rounding, Sense, Stroke, TextStyle, Vec2,
};

pub const TEXT: Color32 = Color32::from_rgb(235, 240, 247);
pub const MUTED: Color32 = Color32::from_rgb(150, 161, 179);
pub const DIM: Color32 = Color32::from_rgb(105, 116, 135);
pub const ACCENT: Color32 = Color32::from_rgb(255, 176, 112);
pub const FIELD: Color32 = Color32::from_rgb(30, 36, 47);
pub const FIELD_HOVER: Color32 = Color32::from_rgb(43, 51, 65);
pub const STROKE: Color32 = Color32::from_rgba_premultiplied(14, 14, 14, 14);
pub const DANGER: Color32 = Color32::from_rgb(255, 128, 138);
pub const OK: Color32 = Color32::from_rgb(128, 232, 176);

pub fn alpha(color: Color32, value: u8) -> Color32 {
    let f = value as u32;
    Color32::from_rgba_premultiplied(
        (color.r() as u32 * f / 255) as u8,
        (color.g() as u32 * f / 255) as u8,
        (color.b() as u32 * f / 255) as u8,
        value,
    )
}

pub fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let lerp = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t) as u8;
    Color32::from_rgba_premultiplied(
        lerp(a.r(), b.r()),
        lerp(a.g(), b.g()),
        lerp(a.b(), b.b()),
        lerp(a.a(), b.a()),
    )
}

pub fn hex_color(value: &str) -> Color32 {
    let value = value.trim().trim_start_matches('#');
    if value.len() != 6 {
        return ACCENT;
    }
    match (
        u8::from_str_radix(&value[0..2], 16),
        u8::from_str_radix(&value[2..4], 16),
        u8::from_str_radix(&value[4..6], 16),
    ) {
        (Ok(r), Ok(g), Ok(b)) => Color32::from_rgb(r, g, b),
        _ => ACCENT,
    }
}

pub fn install_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates = [
        ("C:\\Windows\\Fonts\\msyh.ttc", "msyh"),
        ("C:\\Windows\\Fonts\\simhei.ttf", "simhei"),
    ];
    for (path, name) in candidates {
        if let Ok(bytes) = std::fs::read(path) {
            fonts
                .font_data
                .insert(name.to_owned(), FontData::from_owned(bytes));
            fonts
                .families
                .entry(FontFamily::Proportional)
                .or_default()
                .insert(0, name.to_owned());
            break;
        }
    }
    if !fonts.font_data.contains_key("msyh") && !fonts.font_data.contains_key("simhei") {
        crate::log::warn("Chinese UI font missing; falling back to egui fonts");
    }
    ctx.set_fonts(fonts);
}

pub fn install_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.animation_time = 0.14;
    style.visuals = egui::Visuals::dark();
    style.visuals.panel_fill = Color32::TRANSPARENT;
    style.visuals.window_fill = Color32::TRANSPARENT;
    style.visuals.extreme_bg_color = FIELD;
    style.visuals.faint_bg_color = FIELD;
    style.visuals.selection.bg_fill = ACCENT;
    style.visuals.selection.stroke = Stroke::new(1.0_f32, Color32::from_rgb(36, 26, 16));
    style.visuals.text_cursor.stroke = Stroke::new(1.5_f32, ACCENT);
    style.visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.noninteractive.bg_fill = Color32::TRANSPARENT;
    style.visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, STROKE);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT);
    style.visuals.widgets.inactive.bg_fill = FIELD;
    style.visuals.widgets.inactive.weak_bg_fill = FIELD;
    style.visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, STROKE);
    style.visuals.widgets.inactive.rounding = Rounding::same(8.0);
    style.visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    style.visuals.widgets.hovered.bg_fill = FIELD_HOVER;
    style.visuals.widgets.hovered.weak_bg_fill = FIELD_HOVER;
    style.visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.hovered.rounding = Rounding::same(8.0);
    style.visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::from_rgb(34, 24, 14));
    style.visuals.widgets.active.bg_fill = ACCENT;
    style.visuals.widgets.active.weak_bg_fill = ACCENT;
    style.visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT);
    style.visuals.widgets.active.rounding = Rounding::same(8.0);
    style.visuals.widgets.noninteractive.rounding = Rounding::same(8.0);
    style.visuals.slider_trailing_fill = true;
    style.spacing.item_spacing = Vec2::new(7.0, 6.0);
    style.spacing.button_padding = Vec2::new(9.0, 5.0);
    style.spacing.interact_size = Vec2::new(36.0, 24.0);
    style.spacing.slider_width = 100.0;
    style.spacing.slider_rail_height = 4.0;
    let mut scroll = egui::style::ScrollStyle::solid();
    scroll.bar_width = 3.0;
    scroll.bar_inner_margin = 6.0;
    scroll.bar_outer_margin = 0.0;
    scroll.handle_min_length = 24.0;
    style.spacing.scroll = scroll;
    style.text_styles.insert(
        TextStyle::Heading,
        FontId::new(20.0, FontFamily::Proportional),
    );
    style
        .text_styles
        .insert(TextStyle::Body, FontId::new(12.0, FontFamily::Proportional));
    style.text_styles.insert(
        TextStyle::Button,
        FontId::new(11.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        TextStyle::Small,
        FontId::new(10.0, FontFamily::Proportional),
    );
    ctx.set_style(style);
}

pub fn ease_out(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

pub fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0).max(0.0001)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub fn padded_row_bytes(width: u32) -> u32 {
    let unaligned = width * 4;
    (unaligned + 255) & !255
}

pub fn create_target(device: &wgpu::Device, label: &str, width: u32, height: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    })
}

pub fn create_readback(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
) -> wgpu::Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (padded_row_bytes(width) as u64) * height as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    })
}

pub struct EguiCanvas {
    ctx: egui::Context,
    renderer: egui_wgpu::Renderer,
    target: wgpu::Texture,
    target_view: wgpu::TextureView,
    readback: wgpu::Buffer,
    width: u32,
    height: u32,
    dpi: f32,
    readback_row: u32,
    pixels: Vec<u8>,
}

impl EguiCanvas {
    pub fn new(device: &wgpu::Device, width: u32, height: u32, dpi: f32) -> Self {
        let target = create_target(device, "singularity-egui-target", width, height);
        let target_view = target.create_view(&wgpu::TextureViewDescriptor::default());
        let readback_row = padded_row_bytes(width);
        let readback = create_readback(device, "singularity-egui-readback", width, height);
        let egui_renderer =
            egui_wgpu::Renderer::new(device, wgpu::TextureFormat::Rgba8Unorm, None, 1);
        let ctx = egui::Context::default();
        install_fonts(&ctx);
        install_style(&ctx);
        Self {
            ctx,
            renderer: egui_renderer,
            target,
            target_view,
            readback,
            width,
            height,
            dpi,
            readback_row,
            pixels: vec![0; (width * height * 4) as usize],
        }
    }

    pub fn ctx(&self) -> &egui::Context {
        &self.ctx
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn raw_input(&self, now: f64, dt: f32, events: Vec<egui::Event>) -> egui::RawInput {
        egui::RawInput {
            screen_rect: Some(Rect::from_min_size(
                Pos2::ZERO,
                Vec2::new(self.width as f32 / self.dpi, self.height as f32 / self.dpi),
            )),
            time: Some(now),
            predicted_dt: dt,
            focused: true,
            modifiers: egui::Modifiers::default(),
            events,
            ..Default::default()
        }
    }

    pub fn render(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        raw: egui::RawInput,
        build: impl FnOnce(&egui::Context),
    ) -> &[u8] {
        let output = self.ctx.run(raw, build);
        let paint_jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        let screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [self.width, self.height],
            pixels_per_point: self.dpi,
        };
        for (id, delta) in &output.textures_delta.set {
            self.renderer.update_texture(device, queue, *id, delta);
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("singularity-egui-encoder"),
        });
        let user_buffers =
            self.renderer
                .update_buffers(device, queue, &mut encoder, &paint_jobs, &screen);
        queue.submit(user_buffers);
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("singularity-egui-pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &self.target_view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
            self.renderer.render(&mut pass, &paint_jobs, &screen);
        }
        encoder.copy_texture_to_buffer(
            wgpu::ImageCopyTexture {
                texture: &self.target,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::ImageCopyBuffer {
                buffer: &self.readback,
                layout: wgpu::ImageDataLayout {
                    offset: 0,
                    bytes_per_row: Some(self.readback_row),
                    rows_per_image: Some(self.height),
                },
            },
            wgpu::Extent3d {
                width: self.width,
                height: self.height,
                depth_or_array_layers: 1,
            },
        );
        queue.submit(Some(encoder.finish()));
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }

        let slice = self.readback.slice(..);
        slice.map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::Maintain::Wait);
        let mapped = slice.get_mapped_range();
        let row_bytes = (self.width * 4) as usize;
        for y in 0..self.height as usize {
            let src =
                &mapped[y * self.readback_row as usize..y * self.readback_row as usize + row_bytes];
            let dst = &mut self.pixels[y * row_bytes..(y + 1) * row_bytes];
            for x in 0..self.width as usize {
                let s = &src[x * 4..x * 4 + 4];
                let o = x * 4;
                dst[o] = s[2];
                dst[o + 1] = s[1];
                dst[o + 2] = s[0];
                dst[o + 3] = s[3];
            }
        }
        drop(mapped);
        self.readback.unmap();
        &self.pixels
    }
}

pub fn section_header(ui: &mut egui::Ui, title: &str, accent: Color32) {
    ui.add_space(7.0);
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 13.0), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        Rect::from_min_size(
            Pos2::new(rect.left(), rect.center().y - 5.0),
            Vec2::new(2.5, 10.0),
        ),
        Rounding::same(1.5),
        accent,
    );
    painter.text(
        Pos2::new(rect.left() + 9.0, rect.center().y),
        Align2::LEFT_CENTER,
        title,
        FontId::proportional(10.5),
        MUTED,
    );
    ui.add_space(2.0);
}

pub fn value_row(ui: &mut egui::Ui, label: &str, value: &str, value_color: Color32) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).size(12.0).color(TEXT));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value).size(10.5).color(value_color));
        });
    });
}

pub fn hint(ui: &mut egui::Ui, text: &str) {
    ui.label(RichText::new(text).size(9.5).color(DIM));
}

pub fn hairline(ui: &mut egui::Ui) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 1.0), Sense::hover());
    ui.painter().hline(
        rect.x_range(),
        rect.center().y,
        Stroke::new(1.0_f32, Color32::from_white_alpha(10)),
    );
}

pub fn tab_bar(
    ui: &mut egui::Ui,
    labels: &[&str],
    selected: usize,
    accent: Color32,
) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 32.0), Sense::hover());
    let painter = ui.painter().clone();
    painter.rect_filled(rect, Rounding::same(10.0), Color32::from_white_alpha(5));
    painter.rect_stroke(
        rect,
        Rounding::same(10.0),
        Stroke::new(1.0_f32, Color32::from_white_alpha(9)),
    );
    let inner = rect.shrink(3.0);
    let cell_width = inner.width() / labels.len() as f32;
    let mut clicked = None;
    for (index, label) in labels.iter().enumerate() {
        let cell = Rect::from_min_size(
            Pos2::new(inner.left() + index as f32 * cell_width, inner.top()),
            Vec2::new(cell_width, inner.height()),
        );
        let response = ui.interact(cell, ui.id().with(("ui-tab", index)), Sense::click());
        let is_selected = index == selected;
        if is_selected {
            painter.rect_filled(cell, Rounding::same(8.0), alpha(accent, 30));
            painter.rect_stroke(
                cell,
                Rounding::same(8.0),
                Stroke::new(1.0_f32, alpha(accent, 85)),
            );
        } else if response.hovered() {
            painter.rect_filled(cell, Rounding::same(8.0), Color32::from_white_alpha(6));
        }
        let color = if is_selected {
            accent
        } else if response.hovered() {
            TEXT
        } else {
            MUTED
        };
        painter.text(
            cell.center(),
            Align2::CENTER_CENTER,
            *label,
            FontId::proportional(11.0),
            color,
        );
        if response.clicked() {
            clicked = Some(index);
        }
    }
    clicked
}

pub fn pill_button(
    ui: &mut egui::Ui,
    text: &str,
    accent: Color32,
    primary: bool,
    width: f32,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 28.0), Sense::click());
    let hovered = response.hovered();
    let fill = if primary {
        if hovered {
            alpha(accent, 46)
        } else {
            alpha(accent, 28)
        }
    } else if hovered {
        FIELD_HOVER
    } else {
        FIELD
    };
    let stroke = if primary {
        Stroke::new(1.0_f32, alpha(accent, 95))
    } else if hovered {
        Stroke::new(1.0_f32, alpha(accent, 60))
    } else {
        Stroke::new(1.0_f32, STROKE)
    };
    let painter = ui.painter();
    painter.rect_filled(rect, Rounding::same(8.0), fill);
    painter.rect_stroke(rect, Rounding::same(8.0), stroke);
    let color = if primary {
        if hovered {
            Color32::WHITE
        } else {
            accent
        }
    } else if hovered {
        TEXT
    } else {
        MUTED
    };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(10.8),
        color,
    );
    response
}

pub fn text_link(ui: &mut egui::Ui, text: &str, color: Color32, accent: Color32) -> Response {
    let font = FontId::proportional(10.5);
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(galley.size().x + 10.0, 22.0), Sense::click());
    let painter = ui.painter();
    let hovered = response.hovered();
    let draw_color = if hovered { accent } else { color };
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(10.5),
        draw_color,
    );
    if hovered {
        painter.hline(
            (rect.left() + 5.0)..=(rect.right() - 5.0),
            rect.center().y + 8.0,
            Stroke::new(1.0_f32, alpha(draw_color, 120)),
        );
    }
    response
}

pub fn accent_slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    min: f32,
    max: f32,
    accent: Color32,
    enabled: bool,
) -> Response {
    let sense = if enabled {
        Sense::click_and_drag()
    } else {
        Sense::hover()
    };
    let (rect, mut response) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 18.0), sense);
    if enabled && (response.dragged() || response.clicked()) {
        if let Some(pos) = response.interact_pointer_pos() {
            let t = ((pos.x - rect.left() - 7.0) / (rect.width() - 14.0).max(1.0)).clamp(0.0, 1.0);
            let next = min + (max - min) * t;
            if (next - *value).abs() > 1e-4 {
                *value = next;
                response.mark_changed();
            }
        }
    }
    let t = ((*value - min) / (max - min).max(0.0001)).clamp(0.0, 1.0);
    let center_y = rect.center().y;
    let track = Rect::from_min_size(
        Pos2::new(rect.left() + 2.0, center_y - 2.5),
        Vec2::new(rect.width() - 4.0, 5.0),
    );
    let painter = ui.painter();
    painter.rect_filled(
        track,
        Rounding::same(2.5),
        Color32::from_white_alpha(if enabled { 10 } else { 6 }),
    );
    let filled_width = track.width() * t;
    if filled_width > 0.5 {
        let filled =
            Rect::from_min_size(track.min, Vec2::new(filled_width.max(3.0), track.height()));
        let color = if enabled {
            accent
        } else {
            accent.gamma_multiply(0.35)
        };
        painter.rect_filled(filled, Rounding::same(2.5), color.gamma_multiply(0.92));
        painter.circle_filled(
            Pos2::new(filled.right(), center_y),
            6.5,
            alpha(color, if enabled { 28 } else { 10 }),
        );
    }
    let knob = Pos2::new(track.left() + filled_width, center_y);
    let knob_color = if enabled {
        mix(accent, Color32::WHITE, 0.62)
    } else {
        DIM
    };
    painter.circle_filled(knob, 5.0, knob_color);
    painter.circle_stroke(
        knob,
        5.0,
        Stroke::new(1.0_f32, alpha(accent, if enabled { 170 } else { 50 })),
    );
    response
}

pub fn toggle_row(ui: &mut egui::Ui, label: &str, value: &mut bool, accent: Color32) -> bool {
    let (rect, response) =
        ui.allocate_exact_size(Vec2::new(ui.available_width(), 28.0), Sense::click());
    let hovered = response.hovered();
    if hovered {
        ui.painter().rect_filled(
            rect.shrink2(Vec2::new(2.0, 1.0)),
            Rounding::same(7.0),
            Color32::from_white_alpha(5),
        );
    }
    ui.painter().text(
        Pos2::new(rect.left() + 4.0, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(12.0),
        TEXT,
    );
    draw_switch(ui, rect, response.id, *value, accent, hovered);
    if response.clicked() {
        *value = !*value;
        return true;
    }
    false
}

fn draw_switch(
    ui: &egui::Ui,
    row: Rect,
    id: egui::Id,
    value: bool,
    accent: Color32,
    hovered: bool,
) {
    let t = ui
        .ctx()
        .animate_bool_with_time(id.with("switch"), value, 0.14);
    let track = Rect::from_center_size(
        Pos2::new(row.right() - 22.0, row.center().y),
        Vec2::new(32.0, 18.0),
    );
    let fill = if value {
        alpha(accent, if hovered { 225 } else { 205 })
    } else if hovered {
        FIELD_HOVER
    } else {
        FIELD
    };
    ui.painter().rect_filled(track, Rounding::same(9.0), fill);
    if value {
        ui.painter().rect_stroke(
            track,
            Rounding::same(9.0),
            Stroke::new(1.0_f32, alpha(accent, 90)),
        );
    }
    let knob_x = track.left() + 9.0 + (track.width() - 18.0) * t;
    let knob_color = if value {
        Color32::from_rgb(32, 23, 14)
    } else {
        MUTED
    };
    ui.painter()
        .circle_filled(Pos2::new(knob_x, track.center().y), 6.0, knob_color);
}

pub fn mini_check(ui: &mut egui::Ui, checked: bool, accent: Color32, enabled: bool) -> Response {
    let sense = if enabled {
        Sense::click()
    } else {
        Sense::hover()
    };
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(18.0), sense);
    let inner = rect.shrink(2.0);
    let painter = ui.painter();
    let hovered = response.hovered() && enabled;
    if checked {
        painter.rect_filled(
            inner,
            Rounding::same(5.0),
            if enabled {
                accent
            } else {
                accent.gamma_multiply(0.35)
            },
        );
        let center = inner.center();
        let stroke = Stroke::new(1.6_f32, Color32::from_rgb(26, 19, 12));
        painter.line_segment(
            [
                Pos2::new(center.x - 3.5, center.y),
                Pos2::new(center.x - 1.0, center.y + 2.5),
            ],
            stroke,
        );
        painter.line_segment(
            [
                Pos2::new(center.x - 1.0, center.y + 2.5),
                Pos2::new(center.x + 4.0, center.y - 3.0),
            ],
            stroke,
        );
    } else {
        painter.rect_filled(
            inner,
            Rounding::same(5.0),
            Color32::from_white_alpha(if hovered { 9 } else { 4 }),
        );
        painter.rect_stroke(
            inner,
            Rounding::same(5.0),
            Stroke::new(
                1.0_f32,
                if hovered {
                    alpha(accent, 130)
                } else {
                    Color32::from_white_alpha(30)
                },
            ),
        );
    }
    response
}

pub fn close_button(ui: &mut egui::Ui, color: Color32, size: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(size), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    if hovered {
        painter.rect_filled(rect, Rounding::same(7.0), Color32::from_white_alpha(8));
    }
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "×",
        FontId::proportional(size * 0.62),
        if hovered { TEXT } else { color },
    );
    response
}

pub fn progress_bar(ui: &mut egui::Ui, progress: f32, color: Color32, height: f32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), height), Sense::hover());
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        Rounding::same(height * 0.5),
        Color32::from_white_alpha(10),
    );
    let filled = progress.clamp(0.0, 1.0);
    if filled > 0.0 {
        let bar = Rect::from_min_size(
            rect.min,
            Vec2::new((rect.width() * filled).max(2.0), rect.height()),
        );
        painter.rect_filled(bar, Rounding::same(height * 0.5), color);
        painter.circle_filled(
            Pos2::new(bar.right(), bar.center().y),
            height * 1.7,
            alpha(color, 30),
        );
    }
}

pub fn status_dot(ui: &mut egui::Ui, color: Color32, now: f64) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
    let pulse = ((now * 1.6).sin() as f32 * 0.5 + 0.5) * 0.35 + 0.65;
    let center = rect.center();
    ui.painter()
        .circle_filled(center, 3.0, alpha(color, (80.0 + 120.0 * pulse) as u8));
    ui.painter().circle_filled(center, 1.4, color);
}

pub fn orb(ui: &mut egui::Ui, size: f32, accent: Color32, now: f64) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(size), Sense::hover());
    let center = rect.center();
    let painter = ui.painter();
    painter.circle_filled(center, size * 0.50, alpha(accent, 7));
    painter.circle_filled(center, size * 0.38, alpha(accent, 10));
    painter.circle_stroke(center, size * 0.30, Stroke::new(1.0_f32, alpha(accent, 65)));
    painter.circle_filled(center, size * 0.205, Color32::from_rgb(4, 6, 10));
    painter.circle_stroke(center, size * 0.225, Stroke::new(1.6_f32, accent));
    let angle = now as f32 * 0.8;
    let orbit = Pos2::new(
        center.x + angle.cos() * size * 0.34,
        center.y + angle.sin() * size * 0.34,
    );
    painter.circle_filled(orbit, 1.5, alpha(accent, 215));
    let angle2 = angle + 2.5;
    let orbit2 = Pos2::new(
        center.x + angle2.cos() * size * 0.40,
        center.y + angle2.sin() * size * 0.40,
    );
    painter.circle_filled(orbit2, 1.0, alpha(accent, 120));
}

pub fn top_accent(ui: &egui::Ui, accent: Color32) {
    let rect = ui.max_rect();
    ui.painter().hline(
        (rect.left() + 16.0)..=(rect.right() - 16.0),
        rect.top() + 0.5,
        Stroke::new(1.0_f32, alpha(accent, 45)),
    );
}

#[derive(Clone, Copy)]
pub enum MediaIcon {
    Previous,
    Play,
    Next,
    Mute,
}

pub fn media_button(ui: &mut egui::Ui, icon: MediaIcon, accent: Color32, width: f32) -> Response {
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, 26.0), Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter();
    painter.rect_filled(
        rect,
        Rounding::same(7.0),
        if hovered { FIELD_HOVER } else { FIELD },
    );
    painter.rect_stroke(
        rect,
        Rounding::same(7.0),
        Stroke::new(
            1.0_f32,
            if hovered {
                alpha(accent, 80)
            } else {
                Color32::from_white_alpha(10)
            },
        ),
    );
    let color = if hovered { accent } else { TEXT };
    let center = rect.center();
    match icon {
        MediaIcon::Previous => {
            painter.line_segment(
                [
                    Pos2::new(center.x - 6.0, center.y - 5.0),
                    Pos2::new(center.x - 6.0, center.y + 5.0),
                ],
                Stroke::new(1.8_f32, color),
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(center.x + 6.0, center.y - 5.0),
                    Pos2::new(center.x + 6.0, center.y + 5.0),
                    Pos2::new(center.x - 2.0, center.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        MediaIcon::Play => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(center.x - 4.0, center.y - 5.0),
                    Pos2::new(center.x - 4.0, center.y + 5.0),
                    Pos2::new(center.x + 5.0, center.y),
                ],
                color,
                Stroke::NONE,
            ));
        }
        MediaIcon::Next => {
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(center.x - 6.0, center.y - 5.0),
                    Pos2::new(center.x - 6.0, center.y + 5.0),
                    Pos2::new(center.x + 2.0, center.y),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(center.x + 6.0, center.y - 5.0),
                    Pos2::new(center.x + 6.0, center.y + 5.0),
                ],
                Stroke::new(1.8_f32, color),
            );
        }
        MediaIcon::Mute => {
            painter.rect_filled(
                Rect::from_min_max(
                    Pos2::new(center.x - 9.0, center.y - 2.0),
                    Pos2::new(center.x - 5.0, center.y + 2.0),
                ),
                Rounding::same(1.0),
                color,
            );
            painter.add(egui::Shape::convex_polygon(
                vec![
                    Pos2::new(center.x - 5.0, center.y - 2.0),
                    Pos2::new(center.x - 5.0, center.y + 2.0),
                    Pos2::new(center.x + 1.0, center.y + 6.0),
                    Pos2::new(center.x + 1.0, center.y - 6.0),
                ],
                color,
                Stroke::NONE,
            ));
            painter.line_segment(
                [
                    Pos2::new(center.x + 4.0, center.y - 3.5),
                    Pos2::new(center.x + 9.0, center.y + 3.5),
                ],
                Stroke::new(1.5_f32, color),
            );
            painter.line_segment(
                [
                    Pos2::new(center.x + 9.0, center.y - 3.5),
                    Pos2::new(center.x + 4.0, center.y + 3.5),
                ],
                Stroke::new(1.5_f32, color),
            );
        }
    }
    response
}

pub fn stat_bar(
    ui: &mut egui::Ui,
    label: &str,
    value: f32,
    color: Color32,
    progress: f32,
    override_value: Option<&str>,
) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(ui.available_width(), 22.0), Sense::hover());
    let painter = ui.painter();
    let center_y = rect.center().y;
    painter.circle_filled(Pos2::new(rect.left() + 3.0, center_y), 2.2, color);
    painter.text(
        Pos2::new(rect.left() + 11.0, center_y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(10.5),
        MUTED,
    );
    let bar = Rect::from_min_size(
        Pos2::new(rect.left() + 48.0, center_y - 3.0),
        Vec2::new((rect.width() - 90.0).max(24.0), 6.0),
    );
    painter.rect_filled(bar, Rounding::same(3.0), Color32::from_white_alpha(9));
    let ratio = (value.clamp(0.0, 100.0) / 100.0 * progress).clamp(0.0, 1.0);
    if ratio > 0.0 {
        let filled = Rect::from_min_size(
            bar.min,
            Vec2::new((bar.width() * ratio).max(2.0), bar.height()),
        );
        painter.rect_filled(filled, Rounding::same(3.0), color.gamma_multiply(0.92));
        painter.hline(
            filled.x_range(),
            filled.top() + 1.0,
            Stroke::new(1.0_f32, Color32::from_white_alpha(45)),
        );
        painter.circle_filled(Pos2::new(filled.right(), center_y), 4.5, alpha(color, 45));
    }
    let text = override_value
        .map(str::to_owned)
        .unwrap_or_else(|| format!("{value:.0}%"));
    painter.text(
        Pos2::new(rect.right(), center_y),
        Align2::RIGHT_CENTER,
        text,
        FontId::proportional(10.8),
        if override_value.is_some() {
            MUTED
        } else {
            TEXT
        },
    );
}
