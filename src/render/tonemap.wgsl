// Tonemap + post-processing финал.
//
// ИЗМЕНЕНО (Спринт 1.1 + 1.2):
//   * добавлен выбор тонального маппера (AgX / ACES / Reinhard / Uncharted2 / None);
//   * добавлен color grading: temperature, tint, contrast, saturation,
//     ASC CDL (slope/gain + offset/lift + power/gamma);
//   * exposure_bias (множитель к exposure).

const PI: f32 = 3.14159265359;

struct Params {
    // x = bloom strength, y = exposure, z = time, w = unused
    values: vec4<f32>,
    // x = vignette, y = grain, z = chromatic, w = unused
    effects: vec4<f32>,
    // temperature, tint, contrast, saturation
    grading_a: vec4<f32>,
    // exposure_bias, tonemapper_id, unused, unused
    grading_b: vec4<f32>,
    // lift.rgb
    lift: vec4<f32>,
    // gain.rgb
    gain: vec4<f32>,
    // gamma.rgb
    gamma: vec4<f32>,
};

@group(0) @binding(0) var t_hdr: texture_2d<f32>;
@group(0) @binding(1) var t_bloom: texture_2d<f32>;
@group(0) @binding(2) var s_lin: sampler;
@group(0) @binding(3) var<uniform> params: Params;

struct VertexOutput {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VertexOutput {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var uvs = array<vec2<f32>, 3>(
        vec2<f32>(0.0, 1.0),
        vec2<f32>(2.0, 1.0),
        vec2<f32>(0.0, -1.0),
    );
    var out: VertexOutput;
    out.clip = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

// ============================================================
// AgX (Sobotka 2022, "Minimal AgX")
// ============================================================

const AGX_MAT_IN: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>( 0.842479062253094,  0.0423282422610123, 0.0423756549057051),
    vec3<f32>( 0.0784335999999992, 0.878468636469772,  0.0784336),
    vec3<f32>( 0.0792237451477643, 0.0791661274605434, 0.879142973793104),
);

const AGX_MAT_OUT: mat3x3<f32> = mat3x3<f32>(
    vec3<f32>( 1.19687900512017,  -0.0528968517574562, -0.0529716355144438),
    vec3<f32>(-0.0980208811401368, 1.15190312990417,   -0.0980434501171241),
    vec3<f32>(-0.0990297440797205,-0.0989611768448433, 1.15107367264116),
);

const AGX_MIN_EV: f32 = -12.47393;
const AGX_MAX_EV: f32 = 4.026069;

fn agx_default_contrast_approx(x: vec3<f32>) -> vec3<f32> {
    let x2 = x * x;
    let x4 = x2 * x2;
    return 15.5 * x4 * x2
         - 40.14 * x4 * x
         + 31.96 * x4
         - 6.868 * x2 * x
         + 0.4298 * x2
         + 0.1191 * x
         - 0.00232;
}

fn tonemap_agx(x: vec3<f32>) -> vec3<f32> {
    var v = AGX_MAT_IN * x;

    let log_v = log2(max(v, vec3<f32>(1e-10)));
    v = clamp(
        (log_v - vec3<f32>(AGX_MIN_EV)) / (AGX_MAX_EV - AGX_MIN_EV),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );

    v = agx_default_contrast_approx(v);

    v = AGX_MAT_OUT * v;

    return max(v, vec3<f32>(0.0));
}

// ============================================================
// ACES Filmic (Narkowicz 2015 approximation)
// ============================================================

fn tonemap_aces(x: vec3<f32>) -> vec3<f32> {
    let a = 2.51;
    let b = 0.03;
    let c = 2.43;
    let d = 0.59;
    let e = 0.14;
    return clamp(
        (x * (a * x + b)) / (x * (c * x + d) + e),
        vec3<f32>(0.0),
        vec3<f32>(1.0),
    );
}

// ============================================================
// Reinhard: x / (1 + x)
// ============================================================

fn tonemap_reinhard(x: vec3<f32>) -> vec3<f32> {
    return x / (1.0 + x);
}

// ============================================================
// Uncharted 2 (Hable 2010)
// ============================================================

fn uncharted2_partial(x: vec3<f32>) -> vec3<f32> {
    let A = 0.15;
    let B = 0.50;
    let C = 0.10;
    let D = 0.20;
    let E = 0.02;
    let F = 0.30;
    return ((x * (A * x + C * B) + D * E) / (x * (A * x + B) + D * F)) - E / F;
}

fn tonemap_uncharted2(x: vec3<f32>) -> vec3<f32> {
    let W: f32 = 11.2;
    let curr = uncharted2_partial(x * 2.0);
    let white_scale = 1.0 / uncharted2_partial(vec3<f32>(W));
    return clamp(curr * white_scale, vec3<f32>(0.0), vec3<f32>(1.0));
}

// ============================================================
// Dispatcher
// ============================================================

fn tonemapper_apply(id: u32, x: vec3<f32>) -> vec3<f32> {
    switch (id) {
        case 0u: { return tonemap_agx(x); }
        case 1u: { return tonemap_aces(x); }
        case 2u: { return tonemap_reinhard(x); }
        case 3u: { return tonemap_uncharted2(x); }
        default: { return clamp(x, vec3<f32>(0.0), vec3<f32>(1.0)); }
    }
}

// ============================================================
// Color grading
// ============================================================

fn apply_color_grading(c_in: vec3<f32>, p: Params) -> vec3<f32> {
    let temperature = p.grading_a.x;
    let tint        = p.grading_a.y;
    let contrast    = p.grading_a.z;
    let saturation  = p.grading_a.w;

    var c = c_in;

    // Temperature: +t → теплее (R+, B-), -t → холоднее.
    let t = clamp(temperature, -1.0, 1.0);
    c.r = c.r * (1.0 + t * 0.20);
    c.b = c.b * (1.0 - t * 0.20);

    // Tint: +t → пурпурный (R+, B+, G-), -t → зелёный.
    let g = clamp(tint, -1.0, 1.0);
    c.g = c.g * (1.0 - g * 0.15);
    c.r = c.r * (1.0 + g * 0.05);
    c.b = c.b * (1.0 + g * 0.05);

    // ASC CDL: out = (in * slope + offset) ^ power
    // slope = gain, offset = lift, power = 1 / gamma
    let slope  = p.gain.rgb;
    let offset = p.lift.rgb;
    let power  = vec3<f32>(1.0) / max(p.gamma.rgb, vec3<f32>(0.01));
    c = pow(max(c * slope + offset, vec3<f32>(0.0)), power);

    // Contrast (pivot 0.5).
    c = (c - vec3<f32>(0.5)) * contrast + vec3<f32>(0.5);

    // Saturation (Rec.709 luma).
    let luma = dot(c, vec3<f32>(0.2126, 0.7152, 0.0722));
    c = mix(vec3<f32>(luma), c, saturation);

    return c;
}

// ============================================================
// Hash / noise
// ============================================================

fn hash13(p3_in: vec3<f32>) -> f32 {
    var p3 = fract(p3_in * 0.1031);
    p3 = p3 + dot(p3, p3.zyx + vec3<f32>(31.32));
    return fract((p3.x + p3.y) * p3.z);
}

fn film_grain(uv: vec2<f32>, t: f32) -> f32 {
    let p = vec3<f32>(uv * 1024.0, t);
    return hash13(p) * 2.0 - 1.0;
}

// ============================================================
// Main
// ============================================================

@fragment
fn fs_main(in: VertexOutput) -> @location(0) vec4<f32> {
    var uv = in.uv;

    // --- Хроматическая аберрация ---
    let chromatic = params.effects.z;
    var hdr: vec3<f32>;
    if (chromatic > 0.0001) {
        let center = vec2<f32>(0.5, 0.5);
        let to_center = uv - center;
        let r2 = dot(to_center, to_center);
        let shift = chromatic * r2 * 0.004;
        let dir = normalize(to_center + vec2<f32>(1e-6, 1e-6));

        let r = textureSample(t_hdr, s_lin, uv + dir * shift).r;
        let g = textureSample(t_hdr, s_lin, uv).g;
        let b = textureSample(t_hdr, s_lin, uv - dir * shift).b;
        hdr = vec3<f32>(r, g, b);
    } else {
        hdr = textureSample(t_hdr, s_lin, uv).rgb;
    }

    // --- Bloom ---
    let bloom = textureSample(t_bloom, s_lin, uv).rgb;
    let strength = params.values.x;
    hdr = hdr + bloom * strength;

    // --- Exposure ---
    let exposure_bias = params.grading_b.x;
    hdr = hdr * params.values.y * exposure_bias;

    // --- Tonemap ---
    let tm_id = u32(params.grading_b.y + 0.5);
    var out_rgb = tonemapper_apply(tm_id, max(hdr, vec3<f32>(0.0)));

    // --- Color grading ---
    out_rgb = apply_color_grading(out_rgb, params);

    // --- Vignette ---
    let vignette = params.effects.x;
    if (vignette > 0.0001) {
        let center = vec2<f32>(0.5, 0.5);
        let d = distance(uv, center) * 1.4142;
        let v = smoothstep(1.0, 0.3, d * (1.0 + vignette));
        out_rgb = out_rgb * mix(1.0, v, vignette);
    }

    // --- Film grain ---
    let grain = params.effects.y;
    if (grain > 0.0001) {
        let t = floor(params.values.z * 24.0);
        let g = film_grain(uv, t);
        let luma = dot(out_rgb, vec3<f32>(0.2126, 0.7152, 0.0722));
        let grain_mod = mix(1.0, 0.4, luma);
        out_rgb = clamp(out_rgb + g * grain * 0.08 * grain_mod, vec3<f32>(0.0), vec3<f32>(1.0));
    }

    return vec4<f32>(clamp(out_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), 1.0);
}