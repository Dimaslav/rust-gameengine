// Lens flare — экранный постэффект.
//
// ИЗМЕНЕНО (Спринт 1.4): новый шейдер.
//
// Реализация:
//   * ghost'ы (кружки) вдоль линии, соединяющей солнце и центр экрана;
//   * radial streaks (звёзды) — 8 лучей вокруг солнца;
//   * additive blending поверх HDR-финала (до tonemap).
//
// Порог по яркости: flare реагирует только если солнце «горячее»
// (HDR > threshold). Это не даёт лучам лезть на пасмурное небо.
//
// Occlusion НЕ реализован — если солнце за стеной, flare всё равно
// появится. Окклюзию добавим в отдельном патче (нужен depth sample).

struct Params {
    // xy = позиция солнца в UV [0..1], zw = unused
    sun_screen: vec4<f32>,
    // rgb = цвет солнца, a = видимость (0 если за кадром)
    sun_color: vec4<f32>,
    // x = intensity, y = threshold, z = ghost_count, w = streak_length
    params: vec4<f32>,
};

@group(0) @binding(0) var t_hdr: texture_2d<f32>;
@group(0) @binding(1) var s_lin: sampler;
@group(0) @binding(2) var<uniform> params: Params;

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

fn hash11(p: f32) -> f32 {
    let x = fract(p * 0.1031);
    return fract(x * (x + 33.33));
}

fn ghost_intensity(uv: vec2<f32>, center: vec2<f32>, radius: f32) -> f32 {
    let d = length(uv - center) / max(radius, 1e-4);
    return clamp(1.0 - d * d, 0.0, 1.0);
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let intensity = params.params.x;
    if (intensity < 0.001) {
        return vec4<f32>(0.0);
    }

    let visibility = params.sun_color.a;
    if (visibility < 0.001) {
        return vec4<f32>(0.0);
    }

    let uv = in.uv;
    let sun_uv = params.sun_screen.xy;
    let center = vec2<f32>(0.5, 0.5);
    let sun_color = params.sun_color.rgb;

    // Проверка яркости в точке солнца.
    let sun_lum = dot(
        textureSample(t_hdr, s_lin, sun_uv).rgb,
        vec3<f32>(0.2126, 0.7152, 0.0722),
    );
    let threshold = params.params.y;
    let bright_factor = clamp(
        (sun_lum - threshold) / max(threshold, 1e-3),
        0.0,
        1.0,
    );
    if (bright_factor < 0.001) {
        return vec4<f32>(0.0);
    }

    // === Radial streaks ===
    let streak_length = params.params.w;
    var streak = 0.0;
    if (streak_length > 0.001) {
        let to_sun = uv - sun_uv;
        let dist = length(to_sun);
        let N_DIR: i32 = 8;
        for (var i: i32 = 0; i < N_DIR; i = i + 1) {
            let ang = f32(i) * 6.28318 / f32(N_DIR);
            let dir = vec2<f32>(cos(ang), sin(ang));
            let along = abs(dot(normalize(to_sun + vec2<f32>(1e-6)), dir));
            let falloff = clamp(1.0 - dist / max(streak_length, 1e-4), 0.0, 1.0);
            let angle_weight = pow(along, 16.0);
            streak = streak + falloff * falloff * angle_weight;
        }
        streak = streak / f32(N_DIR);
    }

    // === Ghost'ы ===
    let ghost_count_f = max(params.params.z, 0.0);
    let ghost_count = i32(ghost_count_f);
    var ghosts = 0.0;
    if (ghost_count > 0) {
        let sun_dir = sun_uv - center;
        let sun_dist = length(sun_dir);
        let sun_dir_n = select(vec2<f32>(1.0, 0.0), sun_dir / sun_dist, sun_dist > 1e-4);

        for (var k: i32 = 0; k < 16; k = k + 1) {
            if (k >= ghost_count) { break; }
            let t = f32(k + 1) / ghost_count_f;
            let signed_t = select(t, -t, (k % 2) == 1);
            let ghost_center = center + sun_dir_n * signed_t;
            let radius = 0.04 + hash11(f32(k) * 1.7) * 0.06;
            ghosts = ghosts + ghost_intensity(uv, ghost_center, radius) * 0.25;
        }
    }

    var flare_color = sun_color * (streak * 0.6 + ghosts);
    flare_color = flare_color * intensity * bright_factor * visibility;

    return vec4<f32>(flare_color, 1.0);
}