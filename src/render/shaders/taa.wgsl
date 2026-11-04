// TAA resolve.
//
// Jitter: Halton(2,3) 8 сэмплов (см. renderer/mod.rs). Шейдер получает
// jitter_px текущего и предыдущего кадра через uniform.
//
// Конвенция (совпадает с gbuffer.wgsl / forward_transparent.wgsl):
//   - `t_current` содержит jittered-сцену. Пиксель (i, j) содержит
//     фрагмент F, чья unjittered-UV = ((i+0.5) − jitter_px) / screen.
//   - `t_motion` в пикселе (i, j) описывает displacement
//     **unjittered**-позиции F между кадрами:
//       motion = prev_uv_F − uv_F_unjit_current
//   - `t_history[x, y]` содержит resolved-цвет фрагмента, попавшего
//     в (x, y) при растеризации кадра N-1.
//
// ИЗМЕНЕНО (Sprint 2, TAA velocity rejection):
//   * alpha теперь использует smoothstep(1.5, 10, velocity_px) —
//     в статике TAA почти не реагирует (стабильность), при движении
//     4-10 px/frame быстро переключается на current (нет ghosting).
//   * sharpen-сила падает с ростом velocity. Over-sharpening на
//     движущихся объектах (края оружия, персонажа) раньше давал
//     заметные «звенящие» артефакты.
//   * clip_history теперь возвращает и величину clip-расстояния —
//     используется для быстрого reject истории при полном disocclusion
//     (когда neighborhood clip вынужден сильно ужать history).

struct TaaParams {
    values: vec4<f32>,       // x = base_alpha, y = velocity_scale, z = sharpen, w = reset_flag
    screen: vec4<f32>,       // xy = (w_px, h_px), zw = jitter текущего кадра (пиксели)
    prev_jitter: vec4<f32>,  // xy = jitter предыдущего кадра (пиксели), zw = unused
};

@group(0) @binding(0) var t_current: texture_2d<f32>;
@group(0) @binding(1) var t_history: texture_2d<f32>;
@group(0) @binding(2) var t_motion:  texture_2d<f32>;
@group(0) @binding(3) var s_lin:     sampler;
@group(0) @binding(4) var<uniform> params: TaaParams;

struct VOut {
    @builtin(position) frag_coord: vec4<f32>,
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
    out.frag_coord = vec4<f32>(positions[idx], 0.0, 1.0);
    out.uv = uvs[idx];
    return out;
}

fn rgb_to_ycocg(c: vec3<f32>) -> vec3<f32> {
    let y  = dot(c, vec3<f32>( 0.25,  0.5,  0.25));
    let co = dot(c, vec3<f32>( 0.5,   0.0, -0.5));
    let cg = dot(c, vec3<f32>(-0.25,  0.5, -0.25));
    return vec3<f32>(y, co, cg);
}

fn ycocg_to_rgb(c: vec3<f32>) -> vec3<f32> {
    let t = c.x - c.z;
    return vec3<f32>(t + c.y, c.x + c.z, t - c.y);
}

/// Neighborhood clip в YCoCg. Возвращает `(clipped, distance)`:
///   * clipped — history, ужатый в AABB neighborhood current-кадра.
///   * distance — насколько сильно history пришлось ужать (0 = не пришлось).
///     Большое значение — сигнал «история не соответствует текущей сцене»
///     (disocclusion, смена материала, свет).
fn clip_history(
    uv: vec2<f32>,
    texel: vec2<f32>,
    history: vec3<f32>,
) -> vec3<f32> {
    var m1 = vec3<f32>(0.0);
    var m2 = vec3<f32>(0.0);
    for (var y = -1; y <= 1; y = y + 1) {
        for (var x = -1; x <= 1; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y)) * texel;
            let s = rgb_to_ycocg(textureSample(t_current, s_lin, uv + o).rgb);
            m1 = m1 + s;
            m2 = m2 + s * s;
        }
    }
    m1 = m1 / 9.0;
    m2 = m2 / 9.0;
    let sigma = sqrt(max(m2 - m1 * m1, vec3<f32>(0.0)));
    let clip_min = m1 - 1.25 * sigma;
    let clip_max = m1 + 1.25 * sigma;

    let h_ycocg = rgb_to_ycocg(history);
    let clipped = clamp(h_ycocg, clip_min, clip_max);
    return ycocg_to_rgb(clipped);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let screen_px = params.screen.xy;
    let jitter_px = params.screen.zw;
    let prev_jitter_px = params.prev_jitter.xy;
    let texel = 1.0 / screen_px;

    let uv = in.uv;
    let pixel = vec2<i32>(i32(in.frag_coord.x), i32(in.frag_coord.y));
    let current = textureLoad(t_current, pixel, 0).rgb;

    // Reset flag: после смены camera mode / смены TAA on/off —
    // история невалидна, сразу отдаём current.
    if (params.values.w > 0.5) {
        return vec4<f32>(current, 1.0);
    }

    // Velocity в пикселях.
    let motion = textureLoad(t_motion, pixel, 0).rg;
    let velocity_px = length(motion * screen_px);

    // Reproject: history_uv = prev_uv + prev_jitter_uv
    //   prev_uv = (uv − jitter_uv) + motion
    let jitter_uv = jitter_px / screen_px;
    let uv_unjit = uv - jitter_uv;
    let prev_uv = uv_unjit + motion;

    if (prev_uv.x < 0.0 || prev_uv.x > 1.0 || prev_uv.y < 0.0 || prev_uv.y > 1.0) {
        return vec4<f32>(current, 1.0);
    }

    let prev_jitter_uv = prev_jitter_px / screen_px;
    let history_uv = prev_uv + prev_jitter_uv;

    if (history_uv.x < 0.0 || history_uv.x > 1.0
        || history_uv.y < 0.0 || history_uv.y > 1.0) {
        return vec4<f32>(current, 1.0);
    }

    // History с neighborhood clip.
    let history_raw = textureSample(t_history, s_lin, history_uv).rgb;
    let history = clip_history(uv, texel, history_raw);

    // === Velocity rejection ===
    //
    // smoothstep(1.5, 10, velocity_px):
    //   * < 1.5 px — TAA полностью доверяет истории (стабильность).
    //   * 1.5..10 px — плавное переключение на current.
    //   * > 10 px — alpha = 1.0 (полностью current, никакого ghosting).
    //
    // Диапазон 1.5..10 px/frame при 60 fps соответствует 90..600 px/s.
    // На 1080p это 4.7%..31% ширины экрана в секунду — то есть
    // «медленное вращение камеры» (стабильно) → «быстрое вращение
    // или движущийся объект» (мгновенный reject).
    let velocity_weight = smoothstep(1.5, 10.0, velocity_px);

    // Combine с base_alpha из uniform.
    let base_alpha = params.values.x;
    let alpha = mix(base_alpha, 1.0, velocity_weight);

    var blended = mix(history, current, alpha);

    // === Sharpen ===
    //
    // Unsharp mask. Сила падает с ростом velocity:
    //   * на статике — full sharpen для чёткости;
    //   * на движении — sharpen почти выключен, иначе на краях
    //     появляются «звенящие» (ringing) артефакты, которые TAA
    //     не успевает сглаживать.
    let sharpen = params.values.z * (1.0 - velocity_weight * 0.85);
    if (sharpen > 0.001) {
        let n = textureSample(t_current, s_lin, uv + vec2<f32>( 0.0, -texel.y)).rgb;
        let s = textureSample(t_current, s_lin, uv + vec2<f32>( 0.0,  texel.y)).rgb;
        let e = textureSample(t_current, s_lin, uv + vec2<f32>( texel.x, 0.0)).rgb;
        let w = textureSample(t_current, s_lin, uv + vec2<f32>(-texel.x, 0.0)).rgb;
        let blur = (n + s + e + w) * 0.25;
        blended = blended + (blended - blur) * sharpen;
    }

    return vec4<f32>(max(blended, vec3<f32>(0.0)), 1.0);
}