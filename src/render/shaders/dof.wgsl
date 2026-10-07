// Depth of Field — CoC + bokeh blur.
//
// ИЗМЕНЕНО (Спринт 2.1): новый шейдер.
//
// Алгоритм:
//   1. Восстанавливаем линейную глубину из G-buffer normal.a
//      (там depth_norm = view_depth / far).
//   2. Считаем CoC (Circle of Confusion) для этого пикселя.
//   3. Если CoC < 0.5 px — passthrough.
//   4. Иначе — 16 тапов по golden-angle спирали в диске радиуса CoC.

struct Params {
    // x = focus_distance, y = focus_range, z = max_blur, w = blur_falloff
    params: vec4<f32>,
    // xy = (1/w, 1/h), zw = unused
    screen: vec4<f32>,
    // x = near, y = far, zw = unused
    depth: vec4<f32>,
};

@group(0) @binding(0) var t_hdr: texture_2d<f32>;
@group(0) @binding(1) var t_gbuffer_normal: texture_2d<f32>;
@group(0) @binding(2) var s_lin: sampler;
@group(0) @binding(3) var<uniform> params: Params;

struct VOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) uv: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VOut {
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
    var out: VOut;
    out.clip = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

const GOLDEN_ANGLE: f32 = 2.39996323;
const SAMPLE_COUNT: i32 = 16;

fn linearize_depth(depth_norm: f32) -> f32 {
    let near = params.depth.x;
    let far = params.depth.y;
    return near + depth_norm * (far - near);
}

fn compute_coc(depth_m: f32) -> f32 {
    let focus = params.params.x;
    let range = params.params.y;
    let max_blur = params.params.z;
    let falloff = max(params.params.w, 1e-4);

    let offset = max(abs(depth_m - focus) - range, 0.0);
    return min(offset / falloff, max_blur);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let uv = in.uv;

    let enabled = params.params.z > 0.001;

    let g = textureSample(t_gbuffer_normal, s_lin, uv);
    let depth_norm = g.a;

    let depth_m = linearize_depth(min(depth_norm, 0.999));
    let coc = compute_coc(depth_m);

    if (!enabled || coc < 0.5) {
        return vec4<f32>(textureSample(t_hdr, s_lin, uv).rgb, 1.0);
    }

    let inv_screen = params.screen.xy;
    var sum = vec3<f32>(0.0);
    var weight = 0.0;

    for (var i: i32 = 0; i < SAMPLE_COUNT; i = i + 1) {
        let fi = f32(i);
        let angle = fi * GOLDEN_ANGLE;
        let radius = sqrt((fi + 0.5) / f32(SAMPLE_COUNT));
        let offset = vec2<f32>(cos(angle), sin(angle)) * radius * coc;
        let tap_uv = uv + offset * inv_screen;

        let safe_uv = clamp(tap_uv, vec2<f32>(0.0), vec2<f32>(1.0));
        sum += textureSample(t_hdr, s_lin, safe_uv).rgb;
        weight += 1.0;
    }

    let result = sum / max(weight, 1.0);
    return vec4<f32>(result, 1.0);
}