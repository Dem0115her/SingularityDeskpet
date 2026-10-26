use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use crate::log;
use crate::render::{FrameParams, Renderer, WINDOW_SCALE};

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

pub fn run(args: &[String]) {
    let mut out = PathBuf::from("singularity.png");
    if let Some(index) = args.iter().position(|a| a == "--snapshot") {
        if let Some(value) = args.get(index + 1) {
            if !value.starts_with("--") {
                out = PathBuf::from(value);
            }
        }
    }
    let size: u32 = arg_value(args, "--size")
        .and_then(|v| v.parse().ok())
        .unwrap_or(1000)
        .clamp(128, 2400);
    let time: f32 = arg_value(args, "--time")
        .and_then(|v| v.parse().ok())
        .unwrap_or(2.0);
    let yaw: f32 = arg_value(args, "--yaw")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.055);
    let pitch: f32 = arg_value(args, "--pitch")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.020);
    let pulse: f32 = arg_value(args, "--pulse")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.0);
    let debug: u32 = arg_value(args, "--debug")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    let lens_strength: f32 = arg_value(args, "--lens")
        .and_then(|v| v.parse().ok())
        .unwrap_or(0.60);
    let accent = arg_value(args, "--accent")
        .and_then(|value| crate::settings::parse_accent_hex(&value))
        .unwrap_or([1.0, 0.79, 0.62]);
    let fun_absorb = arg_value(args, "--fun-absorb")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);
    let ripple_age = arg_value(args, "--ripple-age")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(-1.0);
    let ripple_strength = arg_value(args, "--ripple-strength")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(0.0, 2.0);
    let white = arg_value(args, "--white")
        .and_then(|v| v.parse::<f32>().ok())
        .unwrap_or(0.0)
        .clamp(0.0, 1.0);

    let mut renderer = match Renderer::new(size, size) {
        Ok(renderer) => renderer,
        Err(error) => {
            log::error(&format!("snapshot renderer: {error}"));
            return;
        }
    };
    let side = size as f32;
    let params = FrameParams {
        time,
        hole_radius: side / WINDOW_SCALE,
        yaw,
        pitch,
        pulse,
        pulse_age: if pulse > 0.0 { 0.45 } else { 0.0 },
        fade: 1.0,
        lens_strength,
        accent,
        fun: [fun_absorb, ripple_age, ripple_strength, white],
        debug,
        ..Default::default()
    };
    if args.iter().any(|a| a == "--lens-test") {
        let backdrop = build_lens_test_backdrop(size);
        renderer.set_backdrop_image(&backdrop, size, size);
    }
    let pixels = match renderer.render(&params) {
        Ok(pixels) => pixels,
        Err(error) => {
            log::error(&format!("snapshot render: {error}"));
            return;
        }
    };

    // The renderer emits premultiplied BGRA. Convert once to straight RGBA for
    // the transparent PNG, and once composited over deep space for review.
    let (straight, preview) = convert_frame(pixels);

    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "singularity".into());
    let ext = out
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_else(|| ".png".into());
    let straight_path = out.with_file_name(format!("{stem}_rgba{ext}"));
    let preview_path = out.with_file_name(format!("{stem}_preview{ext}"));

    if let Err(error) = write_png(&preview_path, size, size, &preview) {
        log::error(&format!("snapshot preview save: {error}"));
    }
    if let Err(error) = write_png(&straight_path, size, size, &straight) {
        log::error(&format!("snapshot rgba save: {error}"));
    }
    log::info(&format!(
        "snapshot saved: {} and {} ({}x{}, adapter={}, backend={})",
        preview_path.display(),
        straight_path.display(),
        size,
        size,
        renderer.adapter_name(),
        renderer.backend()
    ));

    if args.iter().any(|a| a == "--dump-texture") {
        let tex = crate::texture::build_disk_texture();
        let path = out.with_file_name("disk_texture.png");
        if let Err(error) = write_png(
            &path,
            crate::texture::DISK_TEXTURE_WIDTH,
            crate::texture::DISK_TEXTURE_HEIGHT,
            &tex,
        ) {
            log::error(&format!("texture dump: {error}"));
        } else {
            log::info(&format!("texture dumped to {}", path.display()));
        }
    }
}

fn convert_frame(pixels: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let mut straight = vec![0u8; pixels.len()];
    let mut preview = vec![0u8; pixels.len()];
    for (i, src) in pixels.as_chunks::<4>().0.iter().enumerate() {
        let b = src[0] as f32;
        let g = src[1] as f32;
        let r = src[2] as f32;
        let a = src[3] as f32 / 255.0;
        let inv = if a > 0.0 { 1.0 / a } else { 0.0 };
        let o = i * 4;
        straight[o] = (r * inv).min(255.0) as u8;
        straight[o + 1] = (g * inv).min(255.0) as u8;
        straight[o + 2] = (b * inv).min(255.0) as u8;
        straight[o + 3] = src[3];

        let bg = [6.0f32, 9.0, 15.0];
        preview[o] = (r + bg[0] * (1.0 - a)).min(255.0) as u8;
        preview[o + 1] = (g + bg[1] * (1.0 - a)).min(255.0) as u8;
        preview[o + 2] = (b + bg[2] * (1.0 - a)).min(255.0) as u8;
        preview[o + 3] = 255;
    }
    (straight, preview)
}

pub fn dump_frame(path: &std::path::Path, pixels: &[u8], width: u32, height: u32) {
    let (straight, preview) = convert_frame(pixels);
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "frame".into());
    let ext = path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_else(|| ".png".into());
    let preview_path = path.with_file_name(format!("{stem}_preview{ext}"));
    let straight_path = path.with_file_name(format!("{stem}_rgba{ext}"));
    if let Err(error) = write_png(&preview_path, width, height, &preview) {
        log::error(&format!("frame preview dump failed: {error}"));
    }
    if let Err(error) = write_png(&straight_path, width, height, &straight) {
        log::error(&format!("frame rgba dump failed: {error}"));
    }
    log::info(&format!(
        "frame dumped: {} and {} ({width}x{height})",
        preview_path.display(),
        straight_path.display()
    ));
}

fn build_lens_test_backdrop(size: u32) -> Vec<u8> {
    let size = size.max(2);
    let mut pixels = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 / size as f32;
            let fy = y as f32 / size as f32;
            let grid = if x % 48 == 0 || y % 48 == 0 { 90 } else { 0 };
            let diagonal = if ((x + y) % 160) < 2 || (x as i32 - y as i32).rem_euclid(160) < 2 {
                40
            } else {
                0
            };
            let vignette = 1.0 - 0.35 * ((fx - 0.5).abs() + (fy - 0.5).abs());
            let base = (26.0 * vignette) as u8;
            let r = base.saturating_add(grid).saturating_add(diagonal);
            let g = base.saturating_add((grid as f32 * 0.80) as u8);
            let b = base.saturating_add((grid as f32 * 0.55) as u8);
            let i = ((y * size + x) * 4) as usize;
            pixels[i] = b;
            pixels[i + 1] = g;
            pixels[i + 2] = r;
            pixels[i + 3] = 255;
        }
    }
    pixels
}

fn write_png(path: &PathBuf, width: u32, height: u32, rgba: &[u8]) -> Result<(), String> {
    let file = File::create(path).map_err(|e| e.to_string())?;
    let writer = BufWriter::new(file);
    let mut encoder = png::Encoder::new(writer, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().map_err(|e| e.to_string())?;
    writer.write_image_data(rgba).map_err(|e| e.to_string())?;
    Ok(())
}
