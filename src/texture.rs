const WIDTH: usize = 512;
const HEIGHT: usize = 256;

fn hash2(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32)
        .wrapping_mul(0x8da6_b343)
        .wrapping_add((y as u32).wrapping_mul(0xd8163841))
        .wrapping_add(seed.wrapping_mul(0xcb1a_b31f));
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0x00ff_ffff) as f32 / 16_777_215.0
}

fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}

fn noise(x: f32, y: f32, cells_x: i32, _cells_y: i32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let fx = smooth(x - xi as f32);
    let fy = smooth(y - yi as f32);
    let wrap = |v: i32| v.rem_euclid(cells_x);
    let h00 = hash2(wrap(xi), yi, seed);
    let h10 = hash2(wrap(xi + 1), yi, seed);
    let h01 = hash2(wrap(xi), yi + 1, seed);
    let h11 = hash2(wrap(xi + 1), yi + 1, seed);
    let a = h00 + (h10 - h00) * fx;
    let b = h01 + (h11 - h01) * fx;
    a + (b - a) * fy
}

fn fbm(mut x: f32, mut y: f32, cx: i32, cy: i32, octaves: u32, seed: u32) -> f32 {
    let mut sum = 0.0;
    let mut amp = 0.5;
    let mut norm = 0.0;
    let mut sx = cx;
    let mut sy = cy;
    for i in 0..octaves {
        sum += noise(x, y, sx, sy, seed.wrapping_add(i * 977)) * amp;
        norm += amp;
        x *= 2.03;
        y *= 2.01;
        sx *= 2;
        sy *= 2;
        amp *= 0.5;
    }
    sum / norm
}

fn wrap_delta(a: f32) -> f32 {
    let mut d = a;
    if d > 0.5 {
        d -= 1.0;
    }
    if d < -0.5 {
        d += 1.0;
    }
    d
}

pub fn build_disk_texture() -> Vec<u8> {
    let mut pixels = vec![0u8; WIDTH * HEIGHT * 4];
    let mut hotspots = [(0.0f32, 0.0f32, 0.0f32, 0.0f32, 0.0f32); 28];
    let mut rng = 0x1234_5678u32;
    let mut next = || {
        rng ^= rng << 13;
        rng ^= rng >> 17;
        rng ^= rng << 5;
        (rng & 0x00ff_ffff) as f32 / 16_777_215.0
    };
    for spot in hotspots.iter_mut() {
        *spot = (
            next(),
            next(),
            0.010 + next() * 0.055,
            0.012 + next() * 0.11,
            0.25 + next() * 0.75,
        );
    }

    for y in 0..HEIGHT {
        let v = y as f32 / (HEIGHT - 1) as f32;
        for x in 0..WIDTH {
            let u = x as f32 / WIDTH as f32;

            let cloud = fbm(u, v, 5, 16, 5, 17);
            let fine = fbm(u, v, 16, 52, 4, 401);
            let thread = fbm(u, v, 9, 64, 3, 907);

            let ridge = 1.0 - (2.0 * cloud - 1.0).abs();
            let ridge2 = 1.0 - (2.0 * thread - 1.0).abs();

            let mut hot = 0.0f32;
            for (hu, hv, su, sv, amp) in hotspots {
                let du = wrap_delta(u - hu) / su;
                let dv = (v - hv) / sv;
                let d2 = du * du + dv * dv;
                hot += (-d2 * 1.7).exp() * amp;
            }
            hot = hot.min(1.6);

            let radial = (0.32 * std::f32::consts::PI * (v * 2.0 - 0.72)).sin() * 0.5 + 0.5;
            let density =
                (0.25 + 0.75 * ridge.powf(2.2)) * (0.55 + 0.45 * fine) * (0.72 + 0.28 * radial);
            let filament = ridge2.powf(3.0);
            let fine_out = (fine * 1.35).min(1.0);

            let i = (y * WIDTH + x) * 4;
            pixels[i] = (density.clamp(0.0, 1.0) * 255.0) as u8;
            pixels[i + 1] = (fine_out * 255.0) as u8;
            pixels[i + 2] = (filament * 255.0) as u8;
            pixels[i + 3] = (hot.min(1.0) * 255.0) as u8;
        }
    }
    pixels
}

pub const DISK_TEXTURE_WIDTH: u32 = WIDTH as u32;
pub const DISK_TEXTURE_HEIGHT: u32 = HEIGHT as u32;
