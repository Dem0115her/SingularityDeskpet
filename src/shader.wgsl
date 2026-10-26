struct Uniforms {
    resolution: vec2f,
    time: f32,
    hole_radius: f32,
    camera: vec4f, // yaw, pitch, roll, distance
    disk: vec4f,   // inner, outer, thickness, brightness
    look: vec4f,   // exposure, glow, pulse, pulse_age
    motion: vec4f, // fade, hover_x, hover_y, motion_amount
    flags: vec4f,  // seed, unused, unused, unused
    capture: vec4f,
    accent: vec4f,
    fun: vec4f, // absorption, ripple age, ripple strength, white flash
    extra: vec4f, // focus breath, reserved
};

@group(0) @binding(0) var<uniform> U: Uniforms;
@group(0) @binding(1) var disk_tex: texture_2d<f32>;
@group(0) @binding(2) var disk_sampler: sampler;

const PI: f32 = 3.14159265359;
const TAU: f32 = 6.28318530718;

fn saturate(x: f32) -> f32 {
    return clamp(x, 0.0, 1.0);
}

fn srgb(c: vec3f) -> vec3f {
    let x = max(c, vec3f(0.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3f(0.41666667)) - 0.055;
    return select(hi, lo, x <= vec3f(0.0031308));
}

fn aces(x: vec3f) -> vec3f {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3f(0.0), vec3f(1.0));
}

fn hash12(p: vec2f) -> f32 {
    let p3 = fract(vec3f(p.xyx) * 0.1031);
    let q = p3 + vec3f(dot(p3, p3.yzx + 33.33));
    return fract((q.x + q.y) * q.z);
}

fn noise2(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let a = hash12(i);
    let b = hash12(i + vec2f(1.0, 0.0));
    let c = hash12(i + vec2f(0.0, 1.0));
    let d = hash12(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn fbm2(p0: vec2f) -> f32 {
    var p = p0;
    var sum = 0.0;
    var amp = 0.5;
    var norm = 0.0;
    for (var i = 0; i < 4; i = i + 1) {
        sum += noise2(p) * amp;
        norm += amp;
        p = p * 2.03 + vec2f(1.7, 9.2);
        amp *= 0.5;
    }
    return sum / norm;
}

fn disk_emission(p: vec3f, ray_dir: vec3f, rho: f32, vertical: f32) -> vec3f {
    let inner = U.disk.x;
    let outer = U.disk.y;
    if (rho < inner || rho > outer) {
        return vec3f(0.0);
    }
    let t = saturate((rho - inner) / (outer - inner));
    let phi = atan2(p.z, p.x);
    let time = U.time;

    // Sampled in (angle, log-radius) space. The texture is periodic in angle,
    // so the seam never shows; the slow rotation makes the filaments drift.
    let u = phi / TAU + 0.30 * log(rho / inner) + time * 0.013;
    let v = saturate(log(rho / inner) / log(outer / inner));
    let tex = textureSampleLevel(disk_tex, disk_sampler, vec2f(u, v), 0.0);

    let clump = smoothstep(0.18, 0.88, tex.r);
    let fine = smoothstep(0.25, 0.92, tex.g);
    let thread = smoothstep(0.35, 0.95, tex.b);
    let hot = pow(tex.a, 1.2);

    let edge_in = smoothstep(0.0, 0.10, t);
    let edge_out = 1.0 - smoothstep(0.35, 1.0, t);
    let radial = edge_in * edge_out;
    let lane = 0.10 + 0.90 * mix(clump, thread, 0.35);
    let density = radial * lane * (0.55 + 0.75 * fine);
    if (density < 0.002) {
        return vec3f(0.0);
    }

    let temperature = pow(inner / rho, 0.85);
    var color = mix(vec3f(1.0, 0.76, 0.48), vec3f(0.58, 0.78, 1.0), saturate(temperature));
    color = mix(color, vec3f(1.0, 0.97, 0.92), 0.30);
    color += vec3f(1.0, 0.70, 0.34) * hot * 0.75;

    // Relativistic beaming: the side rotating toward the observer gets hotter
    // and brighter. This is what gives the band its one-sided glow.
    let vdir = normalize(vec3f(p.z, 0.0, -p.x) + vec3f(0.00001, 0.0, 0.0));
    let beta = clamp(0.48 / sqrt(max(rho, 1.2)), 0.0, 0.65);
    let cos_angle = dot(vdir * beta, -ray_dir);
    let gamma = 1.0 / sqrt(max(1.0 - beta * beta, 0.05));
    let delta = 1.0 / (gamma * (1.0 - cos_angle));
    let doppler = pow(clamp(delta, 0.30, 2.8), 2.5);
    let tint = mix(vec3f(0.86, 0.93, 1.0), vec3f(1.0, 0.82, 0.62), saturate(0.45 - 0.45 * cos_angle));

    let radial_weight = pow(inner / rho, 1.30);
    let emission_floor = 0.024 * radial * mix(0.55, 1.0, saturate(temperature));
    let energy = emission_floor
        + density * radial_weight * mix(0.35, 1.25, saturate(temperature)) * vertical * doppler;
    return color * tint * energy * U.disk.w;
}

@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4f {
    var points = array<vec2f, 3>(
        vec2f(-1.0, -1.0),
        vec2f(3.0, -1.0),
        vec2f(-1.0, 3.0),
    );
    return vec4f(points[vertex_index], 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let res = U.resolution;
    let p = (frag.xy - res * 0.5) / U.hole_radius;
    let screen_dist = length(p);
    let time = U.time;

    let yaw = U.camera.x + U.motion.z * 0.018 * U.motion.w + sin(time * 0.137) * 0.006;
    let pitch = U.camera.y + U.motion.z * 0.012 * U.motion.w + sin(time * 0.109 + 1.7) * 0.004;
    let roll = U.camera.z + sin(time * 0.071 + 0.4) * 0.0035;
    let distance = U.camera.w;

    let camera_dir = vec3f(sin(yaw) * cos(pitch), sin(pitch), cos(yaw) * cos(pitch));
    let origin = camera_dir * distance;
    let forward = -camera_dir;
    let right0 = normalize(cross(vec3f(0.0, 1.0, 0.0), forward));
    let up0 = cross(forward, right0);
    let cr = cos(roll);
    let sr = sin(roll);
    let right = right0 * cr + up0 * sr;
    let up = up0 * cr - right0 * sr;

    // The critical impact parameter of a Schwarzschild black hole is 3*sqrt(3)/2.
    // Choosing the tangent-plane scale from it puts the apparent shadow exactly
    // on |p| = 1 for any camera distance, which keeps the pet size deterministic.
    let bcrit = 2.598076211;
    let sin_alpha = bcrit * sqrt(max(1.0 - 1.0 / distance, 0.0)) / distance;
    let tan_alpha = sin_alpha / sqrt(max(1.0 - sin_alpha * sin_alpha, 0.0001));
    let ray_dir = normalize(forward + (right * p.x - up * p.y) * tan_alpha);

    var pos = origin;
    var dir = ray_dir;
    let angular = cross(pos, dir);
    let h2 = dot(angular, angular);
    var light = vec3f(0.0);
    var swallowed = false;

    // Null geodesics in Schwarzschild geometry (rs = 1):
    //   a = -3/2 * h^2 * r / |r|^5
    // This path is used for the optional true-3D preview; the pet's main
    // presentation uses the stable screen-space lensing model below.
    if (U.flags.z > 0.5) {
        for (var i = 0u; i < 300u; i = i + 1u) {
            let r2 = dot(pos, pos);
            let r = sqrt(r2);
            if (r < 1.0) {
                swallowed = true;
                break;
            }
            if (r > 46.0) {
                break;
            }

            let dt = min(0.14 * r, 0.45);
            let rho = length(pos.xz);

            if (rho < U.disk.y * 1.04) {
                let radial_t = clamp(rho / U.disk.y, 0.0, 1.0);
                let edge_taper = 1.0 - smoothstep(0.68, 1.0, radial_t);
                let h = U.disk.z * (0.50 + 0.50 * sqrt(radial_t)) * edge_taper;
                let yn = pos.y / max(h, 0.001);
                let vertical = exp(-0.5 * yn * yn);
                if (vertical > 0.01) {
                    light += disk_emission(pos, dir, rho, vertical) * (dt * 0.20);
                }
            }

            let accel = -1.5 * h2 * pos / (r2 * r2 * r);
            let new_dir = normalize(dir + accel * dt);
            let next = pos + new_dir * dt;

            if (pos.y * next.y <= 0.0 && rho < U.disk.y * 1.10) {
                let frac = pos.y / (pos.y - next.y);
                let hit = mix(pos, next, frac);
                let hit_rho = length(hit.xz);
                if (hit_rho > U.disk.x * 0.96 && hit_rho < U.disk.y * 1.02) {
                    let path = clamp(1.0 / max(abs(dir.y), 0.028), 0.0, 12.0);
                    light += disk_emission(hit, dir, hit_rho, 1.0) * (path * 0.24);
                }
            }

            dir = new_dir;
            pos = next;
        }
    }

    let exposure = U.look.x;
    let pulse = U.look.z;
    let focus_breath = U.extra.x;
    let accent = U.accent.rgb;
    let accent_tint = mix(vec3f(1.0), accent, 0.34);
    let raw_light = light * accent_tint * (1.0 + pulse * 0.55 + focus_breath * 0.65);

    // Keep a soft volumetric base from the previous raytraced shape so the
    // sheet still merges into the sphere/ring. The reference-style thin ring
    // and line are layered on top.
    var shaped_light = raw_light;
    let early_y = abs(p.y + 0.012 * p.x);
    let early_center = 1.0 - smoothstep(1.10, 1.55, abs(p.x));
    let early_band = 1.0 - smoothstep(0.12, 0.30, early_y);
    let early_thin = 1.0 - smoothstep(0.008, 0.020, early_y);
    shaped_light *= mix(1.0, early_thin, early_center * early_band);
    shaped_light *= 1.0 - smoothstep(1.38, 1.62, screen_dist);
    let raw_base = raw_light * 0.30
        * (1.0 - smoothstep(1.45, 1.70, screen_dist));
    var hdr = raw_base + shaped_light * 0.70;

    // The bright ring is the lensed image of the whole accretion flow, not a
    // second disk. It is a broad white-blue annulus with a hotter inner edge,
    // modulated by the same instabilities as the sheet.
    let outside = smoothstep(1.0, 1.07, screen_dist);
    let angle = atan2(p.y, p.x);
    let ring_warp = 0.010 * sin(angle * 2.0 + 0.7 + time * 0.12)
        + 0.006 * sin(angle * 5.0 + time * 0.21)
        + 0.0035 * sin(angle * 11.0 - time * 0.13);
    let ring_d = screen_dist - ring_warp;
    let side = 0.68 + 0.44 * smoothstep(-1.2, 1.2, -p.x);
    let vertical_bias = 0.78 + 0.40 * smoothstep(0.2, 1.2, abs(p.y));
    let ring_core = exp(-pow((ring_d - 1.038) / (0.011 + pulse * 0.010), 2.0));
    let ring_span = 0.33 + 0.06 * vertical_bias;
    let ring_x = saturate((ring_d - 1.095) / ring_span);
    let ring_profile = smoothstep(0.0, 0.12, ring_x)
        * (1.0 - smoothstep(0.84, 1.0, ring_x))
        * (0.62 + 0.42 * ring_x);
    let ring_outer_rim = exp(-pow((ring_d - 1.405) / 0.022, 2.0))
        * smoothstep(1.33, 1.39, screen_dist);
    let ring_halo = exp(-max(screen_dist - 1.22, 0.0) * 2.2) * outside;
    let ring_base = mix(
        vec3f(1.0, 0.93, 0.84),
        vec3f(0.74, 0.85, 1.0),
        smoothstep(1.10, 1.42, screen_dist),
    );
    let ring_color = mix(ring_base, accent, 0.42);
    let outer_halo = exp(-pow((screen_dist - 1.50) / 0.38, 2.0)) * outside;
    let self_halo = (ring_halo * 0.016 + outer_halo * 0.007) * U.look.y;

    // Seam-free polar noise gives the ring cloudy structure instead of the
    // regular spokes that a plain sine wave produces.
    let ring_dir = vec2f(cos(angle), sin(angle));
    let q_low = ring_dir * (2.2 + 4.5 * ring_x) + vec2f(ring_x * 2.2, time * 0.008);
    let q_high = ring_dir * (7.5 + 9.0 * ring_x) + vec2f(ring_x * 6.0, -time * 0.011);
    let n_low = fbm2(q_low);
    let n_high = fbm2(q_high);
    let ridge = 1.0 - abs(2.0 * n_high - 1.0);
    let ring_mod = 0.38 + 0.80 * smoothstep(0.20, 0.80, n_low)
        + 0.18 * (ridge - 0.5);
    let ring_dust = 0.80 + 0.20 * smoothstep(0.25, 0.80, n_low);
    let fine_streak = (0.82 + 0.18 * n_high) * ring_dust;
    let rim_clean = 0.86 + 0.14 * n_low;

    let arc = pow(max(0.0, sin(angle * 3.0 + 1.3 + time * 0.16)), 3.0) * 0.70;
    let hotspot = pow(max(0.0, sin(angle * 2.0 + 0.8)), 10.0) * 0.40
        + pow(max(0.0, sin(angle * 5.0 - 1.2)), 12.0) * 0.28;
    let ring_textured = ring_core * 0.65;
    let ring_light = ring_color * ring_textured * (side + arc + hotspot) * U.look.y;
    let inner_rim = vec3f(0.85, 0.93, 1.0)
        * exp(-pow((screen_dist - 1.052) / 0.012, 2.0))
        * 0.25 * U.look.y;

    // Faint filamentary echo farther out, still part of the lensed image.
    let echo = exp(-pow((screen_dist - 1.46) / 0.20, 2.0)) * 0.008 * outside * ring_mod;
    let echo_color = vec3f(0.95, 0.85, 0.72) * echo * U.look.y;

    // The near side of the disk: a thin, textured, slightly warped band. Its
    // Doppler-brightened side is cooler and brighter, as in the reference.
    let band_tilt = -0.012 * p.x + 0.005 * sin(p.x * 2.1 + 0.4);
    let by = p.y - band_tilt;
    let band_reach = 1.0 - smoothstep(2.10, 2.48, abs(p.x));
    let band_n1 = fbm2(vec2f(p.x * 0.55 + 2.3, 0.15 + time * 0.002));
    let band_n2 = fbm2(vec2f(p.x * 2.9 + 1.1, 0.70 - time * 0.003));
    let band_fine = noise2(vec2f(p.x * 24.0 + 5.0, 0.40 + time * 0.02));
    let band_mod = 0.20 + 0.90 * smoothstep(0.20, 0.80, band_n1)
        + 0.30 * (band_n2 - 0.5) + 0.12 * (band_fine - 0.5);
    let band_streak = 0.92 + 0.08 * sin(p.x * 48.0 + 3.0 * sin(p.x * 7.0) + time * 0.4);
    let band_dop = 0.72 + 0.55 * smoothstep(1.6, -1.6, p.x);
    let band_color = mix(
        vec3f(0.86, 0.94, 1.0),
        vec3f(1.0, 0.80, 0.58),
        smoothstep(-1.6, 1.6, p.x),
    );
    let sparkle = pow(max(0.0, band_n2 - 0.68) / 0.32, 2.0) * 0.35;
    let streak_core = exp(-pow(by / 0.017, 2.0)) * band_reach
        * (band_mod + sparkle) * band_streak;
    let streak_glow = exp(-pow(by / 0.078, 2.0)) * band_reach * (0.28 + 0.72 * band_mod);
    let streak_wide = exp(-pow(by / 0.18, 2.0)) * band_reach * 0.32;

    // Click ripple: a faint shock wave travelling outward through the halo.
    let ripple_r = 1.0 + U.look.w * 1.55;
    let ripple = exp(-pow((screen_dist - ripple_r) / 0.055, 2.0)) * pulse * 0.20 * outside;

    // Wide, cool bloom that fades before the window edge.
    let bloom = (exp(-max(screen_dist - 1.20, 0.0) * 2.2) * 0.025
        + exp(-max(screen_dist - 1.70, 0.0) * 0.85) * 0.006) * outside;

    hdr += ring_light + inner_rim + echo_color + ring_color * self_halo;
    hdr += band_color * (streak_core * 0.0 + streak_glow * 0.0 + streak_wide * 0.0) * band_dop * U.look.y;
    hdr += vec3f(0.55, 0.72, 1.0) * (bloom + ripple) * U.look.y;
    hdr += vec3f(0.78, 0.90, 1.0) * (ring_core * pulse * 1.0 + ripple * 1.5);

    // Reference-style ring: a complete soft white annulus with a brighter
    // upper/left side, filled over the raytraced texture.
    let lobe = 0.014 * sin(angle + 0.8) + 0.007 * sin(angle * 2.0 - 0.3);
    let ref_d = screen_dist - ring_warp * 0.8 - lobe;
    let ref_ring = smoothstep(1.44, 1.50, ref_d)
        * (1.0 - smoothstep(1.56, 1.68, ref_d));
    let ref_core = smoothstep(1.44, 1.50, ref_d)
        * (1.0 - smoothstep(1.50, 1.60, ref_d));
    let top_bias = 0.74 + 0.30 * smoothstep(-1.0, 1.0, -p.y);
    let ref_texture = 0.72 + 0.46 * smoothstep(0.20, 0.80, n_low)
        * (0.90 + 0.20 * (n_high - 0.5) * 2.0);
    let left_warm = smoothstep(-1.4, 1.6, -p.x);
    let ring_render_color = mix(ring_color, vec3f(1.0, 0.72, 0.45), left_warm * 0.20);
    hdr += ring_render_color * (ref_ring * 0.32 + ref_core * 0.28) * top_bias * ref_texture;
    let ring_soft = smoothstep(1.42, 1.50, screen_dist)
        * (1.0 - smoothstep(1.72, 1.90, screen_dist));
    hdr += ring_render_color * ring_soft * 0.07 * top_bias * ref_texture;

    let ref_halo = exp(-max(screen_dist - 1.46, 0.0) * 1.90) * outside;
    hdr += ring_color * ref_halo * 0.024;

    // Near-side sheet: very thin, slightly tilted, long and tapered.
    let ref_band_y = abs(p.y + 0.012 * p.x + 0.004 * sin(p.x * 2.4 + 0.6));
    let ref_taper = 1.0 - smoothstep(1.10, 2.85, abs(p.x));
    let ref_half = 0.004 + 0.022 * ref_taper;
    let ref_line = exp(-pow(ref_band_y / max(ref_half, 0.001), 2.0))
        * (1.0 - smoothstep(2.15, 2.85, abs(p.x)));
    let band_tex = 0.82 + 0.30 * smoothstep(0.20, 0.80, fbm2(vec2f(p.x * 0.85 + time * 0.006, 5.3)));
    let band_render_color = mix(ring_color, vec3f(1.0, 0.52, 0.20), left_warm * 0.55);
    hdr += band_render_color * (ref_line * 0.90 + exp(-pow(ref_band_y / 0.065, 2.0)) * 0.050) * band_tex;

    let ref_warm = smoothstep(0.0, 1.4, -p.x)
        * exp(-pow(ref_band_y / 0.050, 2.0))
        * (1.0 - smoothstep(2.45, 2.9, abs(p.x)));
    hdr += vec3f(1.0, 0.38, 0.12) * ref_warm * 0.160;
    let left_fringe = smoothstep(0.2, 1.4, -p.x)
        * exp(-pow((screen_dist - 1.20) / 0.35, 2.0));
    hdr += vec3f(1.0, 0.32, 0.10) * left_fringe * 0.035;

    let bridge = exp(-pow((abs(p.x) - 1.44) / 0.30, 2.0))
        * exp(-pow(p.y / 0.16, 2.0));
    hdr += ring_color * bridge * 0.10;

    // Transparent moat between the central sphere and the outer ring. Only
    // the thin sheet crossing this annulus remains emissive.
    let gap = smoothstep(1.040, 1.090, screen_dist)
        * (1.0 - smoothstep(1.38, 1.44, screen_dist));
    let band_keep = saturate(max(ref_line * 1.25, ref_warm * 1.6));
    let gap_preserve = mix(1.0, band_keep, gap);
    hdr *= gap_preserve;

    // The reference is brighter on the approaching (left) side.
    let side_gain = 0.55 + 0.55 * smoothstep(-1.6, 1.6, -p.x);
    hdr *= side_gain;

    let grain = (hash12(frag.xy * 0.73) - 0.5) * 0.0012;
    hdr += vec3f(grain);

    let debug = U.flags.x;
    if (debug > 0.5) {
        var dbg = vec3f(0.0);
        if (debug < 1.5) {
            dbg = light;
        } else if (debug < 2.5) {
            dbg = ring_light + inner_rim;
        } else if (debug < 3.5) {
            dbg = band_color * (streak_core * 1.15 + streak_glow * 0.20);
        } else if (debug < 4.5) {
            dbg = light;
        } else {
            dbg = ring_light + inner_rim
                + band_color * (streak_core * 1.15 + streak_glow * 0.20);
        }
        return vec4f(dbg, 1.0);
    }

    // The scene pass writes linear HDR + coverage. The composite pass adds
    // bloom, tone maps, and converts to premultiplied BGRA.
    let aa = 1.35 / U.hole_radius;
    let shadow = smoothstep(1.0 + aa, 1.0 - aa, screen_dist);
    hdr *= 1.0 - smoothstep(2.70, 2.88, screen_dist);

    let mapped = aces(hdr * exposure);
    let display_rgb = srgb(mapped);
    let luminance = max(display_rgb.r, max(display_rgb.g, display_rgb.b));
    var alpha = saturate(luminance * 3.0 + ring_core * 0.30);
    alpha = max(alpha, shadow);
    alpha *= mix(1.0, band_keep, gap);
    return vec4f(hdr, alpha);
}
