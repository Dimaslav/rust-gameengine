// Motion Blur — 12-tap directional blur вдоль motion vector.
//
// ИЗМЕНЕНО (Спринт 2.2): новый шейдер.
//
// Motion buffer (Rg16Float) содержит per-pixel displacement:
//   motion = prev_uv − current_uv
// Он уже включает camera + object движение (заполняется в gbuffer.wgsl
// и forward_transparent.wgsl).
//
// Здесь мы просто размываем HDR вдоль этого вектора.
//
// Упрощение v1: без разбиения на ближний/дальний план по глубине,
// без occlusion-aware. Достаточно для игр среднего темпа.

struct Params {
    // x = intensity, y = max_blur_px, z = samples, w = unused
    params: vec4<f32>,
    // xy = (w_px, h_px), zw = unused
    screen: vec4<f32>,
};

@group(0) @binding(0) var t_hdr: texture_2d<f32>;
@group(0) @binding(1) var t_motion: texture_2d<f32>;
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

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let uv = in.uv;
    let intensity = params.params.x;
    let max_blur_px = params.params.y;
    let samples = i32(params.params.z + 0.5);

    // Если выключено — passthrough.
    if (intensity < 0.001 || samples < 2) {
        return vec4<f32>(textureSample(t_hdr, s_lin, uv).rgb, 1.0);
    }

    // Motion в UV space.
    let motion = textureSample(t_motion, s_lin, uv).rg;

    // Переводим в пиксели, ограничиваем max_blur_px.
    let screen_px = params.screen.xy;
    var vel_px = motion * screen_px * intensity;
    let vel_len = length(vel_px);

    // Если смещение меньше 1.5 пикселя — не тратим сэмплы.
    if (vel_len < 1.5) {
        return vec4<f32>(textureSample(t_hdr, s_lin, uv).rgb, 1.0);
    }

    // Ограничиваем длину.
    if (vel_len > max_blur_px) {
        vel_px = vel_px / vel_len * max_blur_px;
    }

    // Velocity в UV space (для offset).
    let vel_uv = vel_px / screen_px;

    // 12-tap с равномерными весами. Джиттер по hash от позиции
    // сглаживает banding при малом числе сэмплов.
    let hash = fract(sin(dot(uv, vec2<f32>(12.9898, 78.233))) * 43758.5453);
    let jitter = (hash - 0.5) / f32(samples);

    var sum = vec3<f32>(0.0);
    var weight = 0.0;

    let n = f32(samples);
    for (var i: i32 = 0; i < 32; i = i + 1) {
        if (i >= samples) { break; }
        // t идёт от -0.5 до +0.5.
        let t = (f32(i) / (n - 1.0)) - 0.5 + jitter;
        let tap_uv = uv + vel_uv * t;
        let safe_uv = clamp(tap_uv, vec2<f32>(0.0), vec2<f32>(1.0));

        // Веса: косинусный falloff от центра, чтобы центр пикселя
        // доминировал. Это уменьшает «двойной контур» на краях.
        let w = 1.0 - abs(t) * 1.2;
        sum += textureSample(t_hdr, s_lin, safe_uv).rgb * w;
        weight += w;
    }

    let result = sum / max(weight, 1e-4);
    return vec4<f32>(result, 1.0);
}