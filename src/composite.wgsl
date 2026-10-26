struct Uniforms {
    resolution: vec2f,
    time: f32,
    hole_radius: f32,
    camera: vec4f,
    disk: vec4f,
    look: vec4f,
    motion: vec4f,
    flags: vec4f,
    capture: vec4f,
    accent: vec4f,
    fun: vec4f,
    extra: vec4f,
};

@group(0) @binding(0) var<uniform> U: Uniforms;
@group(0) @binding(1) var scene_tex: texture_2d<f32>;
@group(0) @binding(2) var scene_sampler: sampler;
@group(0) @binding(3) var backdrop_tex: texture_2d<f32>;

fn aces(x: vec3f) -> vec3f {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp((x * (a * x + b)) / (x * (c * x + d) + e), vec3f(0.0), vec3f(1.0));
}

fn srgb(c: vec3f) -> vec3f {
    let x = max(c, vec3f(0.0));
    let lo = x * 12.92;
    let hi = 1.055 * pow(x, vec3f(0.41666667)) - 0.055;
    return select(hi, lo, x <= vec3f(0.0031308));
}

fn hash12(p: vec2f) -> f32 {
    let p3 = fract(vec3f(p.xyx) * 0.1031);
    let q = p3 + vec3f(dot(p3, p3.yzx + 33.33));
    return fract((q.x + q.y) * q.z);
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
    let uv = frag.xy / res;
    let center = textureSampleLevel(scene_tex, scene_sampler, uv, 0.0);

    let debug = U.flags.x > 0.5;
    if (debug) {
        let mapped = aces(center.rgb * U.look.x);
        return vec4f(srgb(mapped), 1.0);
    }

    // A cheap two-ring Poisson bloom. Its radius is tied to the window size so
    // the ring keeps the same thickness at every DPI and pet size.
    let radius = res.x * 0.018;
    var dirs = array<vec2f, 8>(
        vec2f(1.0, 0.0),
        vec2f(0.7071, 0.7071),
        vec2f(0.0, 1.0),
        vec2f(-0.7071, 0.7071),
        vec2f(-1.0, 0.0),
        vec2f(-0.7071, -0.7071),
        vec2f(0.0, -1.0),
        vec2f(0.7071, -0.7071),
    );
    var bloom = center.rgb * 0.10;
    for (var i = 0; i < 8; i = i + 1) {
        let d = dirs[i];
        let o1 = d * radius * 0.55 / res;
        let o2 = d * radius * 1.15 / res;
        let o3 = d * radius * 1.85 / res;
        bloom += textureSampleLevel(scene_tex, scene_sampler, uv + o1, 0.0).rgb * 0.050;
        bloom += textureSampleLevel(scene_tex, scene_sampler, uv + o2, 0.0).rgb * 0.020;
        bloom += textureSampleLevel(scene_tex, scene_sampler, uv + o3, 0.0).rgb * 0.006;
    }

    let hdr = center.rgb + bloom * 0.34;
    let scene_rgb = srgb(aces(hdr * U.look.x));
    let d = length(frag.xy - res * 0.5) / U.hole_radius;

    let bloom_lum = max(bloom.r, max(bloom.g, bloom.b)) * U.look.x;
    let edge_fade = 1.0 - smoothstep(2.70, 2.88, d);
    let scene_alpha = saturate(max(center.a, saturate(bloom_lum * 1.8)) * edge_fade * U.motion.x);

    // The captured desktop is already display-referred, so it bypasses the
    // scene's HDR tone mapping. Lensing is a radial displacement with a small
    // chromatic split, matching the C# reference lens.
    let absorb = saturate(U.fun.x);
    let ripple_age = U.fun.y;
    let ripple_strength = max(U.fun.z, 0.0);
    let white = saturate(U.fun.w);
    let accent = U.accent.rgb;

    var backdrop = vec3f(0.0);
    var backdrop_alpha = 0.0;
    var ripple_ring = 0.0;
    var ripple_radius = 0.0;
    if (ripple_age >= 0.0) {
        ripple_radius = 1.02 + ripple_age * 2.35;
        ripple_ring = exp(-pow((d - ripple_radius) / 0.075, 2.0)) * ripple_strength;
    }
    if (U.capture.z > 0.5 && U.capture.w > 0.5) {
        let capture_size = vec2f(U.capture.z, U.capture.w);
        let offset = vec2f(U.capture.x, U.capture.y);
        let delta = frag.xy - res * 0.5;
        let dist = max(length(delta), 0.0001);
        let dir = delta / dist;
        let tangent = vec2f(-dir.y, dir.x);
        let falloff = 1.0 - smoothstep(0.92, 2.70, d);
        let outer_soft = 1.0 - smoothstep(2.20, 2.90, d);
        let suction = absorb * U.hole_radius * 0.62 / max(d, 0.28)
            * (1.0 - smoothstep(0.72, 2.35, d));
        let swirl = absorb * U.hole_radius * 0.15
            * (1.0 - smoothstep(0.90, 2.10, d)) / max(d, 0.55);
        var ripple = 0.0;
        if (ripple_age >= 0.0) {
            let wave = exp(-pow((d - ripple_radius) / 0.24, 2.0));
            ripple = sin((d - ripple_radius) * 21.0) * wave
                * ripple_strength * U.hole_radius * 0.16;
        }
        let bend = U.flags.y * U.hole_radius / max(d, 0.16)
            * (0.30 + 0.70 * falloff * falloff) * outer_soft
            + suction + ripple;
        let local = frag.xy + dir * bend + tangent * swirl;
        let base_uv = (local + offset) / capture_size;
        let split = dir * bend * 0.012 / capture_size;
        let uv_c = clamp(base_uv, vec2f(0.0), vec2f(1.0));
        let uv_r = clamp(base_uv + split, vec2f(0.0), vec2f(1.0));
        let uv_b = clamp(base_uv - split, vec2f(0.0), vec2f(1.0));
        backdrop.r = textureSampleLevel(backdrop_tex, scene_sampler, uv_r, 0.0).r;
        backdrop.g = textureSampleLevel(backdrop_tex, scene_sampler, uv_c, 0.0).g;
        backdrop.b = textureSampleLevel(backdrop_tex, scene_sampler, uv_b, 0.0).b;
        backdrop_alpha = (1.0 - smoothstep(2.45, 2.98, d)) * U.motion.x;
    }

    var alpha = saturate(max(scene_alpha, backdrop_alpha));
    var rgb = scene_rgb * scene_alpha
        + backdrop * backdrop_alpha * (1.0 - scene_alpha);

    let absorb_glow = exp(-pow((d - 1.12) / 0.24, 2.0)) * absorb * 0.36;
    let absorb_alpha = absorb_glow * 0.55 * U.motion.x;
    rgb += accent * absorb_alpha;
    alpha = max(alpha, absorb_alpha);

    if (ripple_ring > 0.001) {
        let edge_fade = 1.0 - smoothstep(2.60, 2.90, d);
        let ripple_alpha = ripple_ring * 0.55 * edge_fade * U.motion.x;
        rgb += accent * ripple_alpha;
        alpha = max(alpha, ripple_alpha);
    }

    if (white > 0.001) {
        let white_core = (1.0 - smoothstep(0.0, 1.06, d)) * white;
        let white_ring = exp(-pow((d - 1.22) / 0.15, 2.0)) * white;
        let white_alpha = max(white_core * 0.96, white_ring * 0.78) * U.motion.x;
        rgb = mix(rgb, vec3f(1.0, 0.99, 0.96) * white_alpha, white_alpha);
        rgb += vec3f(0.86, 0.94, 1.0) * white_ring * 0.22 * U.motion.x;
        alpha = max(alpha, white_alpha);
    }

    let dither = (hash12(frag.xy * 1.37) - 0.5) * 2.0 / 255.0;
    let out_alpha = saturate(alpha + dither);
    let out_rgb = max(rgb + vec3f(dither * out_alpha), vec3f(0.0));

    return vec4f(out_rgb, out_alpha);
}
