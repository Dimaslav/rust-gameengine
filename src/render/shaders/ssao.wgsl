// Horizon-based ambient occlusion (HBAO) — подмножество GTAO.
//
// Заменяет старый hemisphere-sampling SSAO, который давал заметные
// артефакты на крупных плоскостях и не давал правильного заслонения
// от «нависающей» геометрии (потолок, арки).
//
// Алгоритм (Jimenez et al., "Practical Realtime Strategies for
// Accurate Indirect Occlusion", 2016):
//
//   1. Восстанавливаем view-space позицию из G-buffer (R=normal,
//      A=depth_norm).
//   2. Строим локальный tangent frame (T, B, N) вокруг view-space
//      нормали.
//   3. Для 8 азимутальных направлений (с шумом-поворотом на пиксель)
//      маршируем в +direction и −direction по 5 шагов. На каждом шаге
//      читаем depth, реконструируем поверхность, вычисляем угол
//      горизонта от tangent plane.
//   4. Для найденных горизонтов h⁺ и h⁻ считаем заслонение
//      по формуле  occ(h) = 0.5 * (sin(h) − h·cos(h)).
//   5. AO = 1 − avg(occ⁺ + occ⁻).
//
// Ключевое отличие от старого SSAO: тот сэмплировал фиксированные
// точки на полусфере и проверял depth в их UV — без учёта реального
// угла заслонения. Здесь мы находим РЕАЛЬНЫЙ угол горизонта, что
// даёт корректную кривую заслонения на плоскостях, стыках и в углах.
//
// Стоимость: 8 dirs × 5 steps × 2 directions = 80 texture reads.
// Для 1080p и radius 0.65 м это < 1 ms на средней GPU. Temporal
// шум (rotation от 4×4 noise texture) размывается TAA в композите.

struct SsaoUniform {
    // x = proj[0][0], y = proj[1][1], z = far, w = radius
    proj_scale: vec4<f32>,
    // x = bias, y = strength, zw = noise_uv_scale (screen_w/4, screen_h/4)
    params: vec4<f32>,
    // x = time (не используется для HBAO — оставлено для совместимости)
    time: vec4<f32>,
    // world → view. G-buffer хранит world-space нормаль,
    // HBAO работает во view-space (reconstruct + sampling + projection).
    view: mat4x4<f32>,
};

@group(0) @binding(0) var t_gbuffer: texture_2d<f32>;
@group(0) @binding(1) var t_noise: texture_2d<f32>;
@group(0) @binding(2) var s_lin: sampler;
@group(0) @binding(3) var<uniform> params: SsaoUniform;
// binding 4:
//   ssao pass — тот же gbuffer (для marching depth);
//   blur pass — gbuffer (для bilateral depth weight).
@group(0) @binding(4) var t_depth_src: texture_2d<f32>;

const NUM_DIRS: u32 = 8u;
const NUM_STEPS: u32 = 5u;
const MAX_HORIZON_TAN: f32 = 10.0; // ≈84°, клампит «почти вертикальные» артефакты

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
// Helpers
// ============================================================

fn load_depth(uv: vec2<f32>) -> f32 {
    if (uv.x < 0.0 || uv.x > 1.0 || uv.y < 0.0 || uv.y > 1.0) {
        return 1.0;
    }
    return textureSample(t_depth_src, s_lin, uv).a;
}

/// Восстанавливает view-space позицию из UV + normalized depth.
///
/// depth_norm ∈ [0,1] — линейная view-depth, делённая на far (см.
/// gbuffer.wgsl: `depth_norm = in.view_depth / camera.near_far.y`).
///
/// Обратная проекция:
///   ld = depth_norm * far              — линейная глубина
///   ndc = (uv * 2 − 1) с y-flip         — NDC
///   view = (ndc.x · ld / proj[0][0],
///           ndc.y · ld / proj[1][1],
///           −ld)                         — view-space
fn reconstruct_view_pos(uv: vec2<f32>, depth_norm: f32) -> vec3<f32> {
    let ndc = vec2<f32>(uv.x * 2.0 - 1.0, (1.0 - uv.y) * 2.0 - 1.0);
    let ld = depth_norm * params.proj_scale.z;
    let vx = ndc.x * ld / params.proj_scale.x;
    let vy = ndc.y * ld / params.proj_scale.y;
    return vec3<f32>(vx, vy, -ld);
}

/// Проекция view-space точки в UV.
///
/// Здесь очень важно делить на `-sp.z` (положительная дистанция),
/// а не на 1.0 — иначе точки уезжают от центра пропорционально
/// глубине, и horizon-samples не попадают в правильные пиксели.
fn project_view_to_uv(sp: vec3<f32>) -> vec2<f32> {
    let w = max(-sp.z, 1e-4);
    let ndc_x = sp.x * params.proj_scale.x / w;
    let ndc_y = sp.y * params.proj_scale.y / w;
    return vec2<f32>(ndc_x * 0.5 + 0.5, 0.5 - ndc_y * 0.5);
}

/// Один марш в направлении `march_dir * sign` от `view_pos`.
/// Возвращает максимальный tan(horizon angle) — то есть
/// height / horizontal_distance для всех найденных окклюдеров.
///
/// `sign = +1` — вперёд (положительный горизонт);
/// `sign = −1` — назад (отрицательный горизонт).
fn march_horizon(
    view_pos: vec3<f32>,
    view_normal: vec3<f32>,
    march_dir: vec3<f32>,
    radius: f32,
    bias: f32,
    sign: f32,
) -> f32 {
    var max_tan = 0.0;

    for (var j: u32 = 1u; j <= NUM_STEPS; j = j + 1u) {
        let t = f32(j) / f32(NUM_STEPS) * radius;
        let ray_pos = view_pos + march_dir * (t * sign);
        let ray_uv = project_view_to_uv(ray_pos);

        if (ray_uv.x < 0.0 || ray_uv.x > 1.0 || ray_uv.y < 0.0 || ray_uv.y > 1.0) {
            break;
        }

        let depth = load_depth(ray_uv);
        if (depth >= 0.9999) {
            continue; // sky
        }

        let surface_pos = reconstruct_view_pos(ray_uv, depth);
        if (surface_pos.z >= 0.0) {
            continue; // behind camera
        }

        let delta = surface_pos - view_pos;
        let height = dot(delta, view_normal);
        if (height <= bias) {
            // Окклюдер ниже tangent plane — не влияет на полусферу.
            // Это отсекает артефакты от «пола» (height ≈ 0).
            continue;
        }

        let horiz = delta - view_normal * height;
        let horiz_len = length(horiz);
        if (horiz_len < 0.01) {
            continue; // окклюдер прямо над фрагментом — некорректная геометрия
        }

        let tan_h = height / horiz_len;
        if (tan_h > max_tan) {
            max_tan = tan_h;
        }
    }

    return max_tan;
}

/// HBAO-интеграл для одного горизонта.
/// Вход: h ∈ [0, π/2]. Выход: occ ∈ [0, 0.5].
///
/// Формула из Jimenez 2016 (упрощённая из полного интеграла
/// ∫cos(θ)[H(θ,h⁺) − H(θ,h⁻)]dθ).
fn horizon_occlusion(h: f32) -> f32 {
    return 0.5 * (sin(h) - h * cos(h));
}

/// Полный HBAO для одного фрагмента.
///
/// Замечание про temporal-стабильность: rotation-шум поворачивает
/// 8 азимутальных направлений per-pixel (через 4×4 noise texture).
/// TAA в композите усредняет шум по ~8 кадрам — в результате AO
/// выглядит гладким без дополнительного denoise-pass.
fn compute_hbao_occlusion(
    view_pos: vec3<f32>,
    view_normal: vec3<f32>,
    uv: vec2<f32>,
) -> f32 {
    let radius = params.proj_scale.w;
    let bias = params.params.x;

    // Per-pixel rotation из noise-текстуры.
    // params.params.zw = (screen_w/4, screen_h/4), noise = 4×4.
    // fract() гарантирует [0,1) для bilinear-сэмплинга с clamp-sampler.
    let noise_uv = fract(uv * params.params.zw);
    let noise_vec = textureSample(t_noise, s_lin, noise_uv).xy;
    let rot_angle = atan2(noise_vec.y, noise_vec.x);

    // Tangent frame. up выбираем так, чтобы T не был вырожден.
    let up = select(
        vec3<f32>(0.0, 1.0, 0.0),
        vec3<f32>(1.0, 0.0, 0.0),
        abs(view_normal.y) > 0.99,
    );
    let T = normalize(cross(up, view_normal));
    let B = cross(view_normal, T);

    var total_occlusion = 0.0;

    for (var d: u32 = 0u; d < NUM_DIRS; d = d + 1u) {
        let phi = f32(d) * 6.28318530718 / f32(NUM_DIRS) + rot_angle;
        let march_dir = T * cos(phi) + B * sin(phi);

        let tan_pos = march_horizon(view_pos, view_normal, march_dir, radius, bias,  1.0);
        let tan_neg = march_horizon(view_pos, view_normal, march_dir, radius, bias, -1.0);

        // Клампим «почти вертикальные» горизонты — иначе из-за
        // шума depth они могут давать выбросы до π/2, что даёт
        // избыточное заслонение на edge-пикселях.
        let h_pos = atan(min(tan_pos, MAX_HORIZON_TAN));
        let h_neg = atan(min(tan_neg, MAX_HORIZON_TAN));

        total_occlusion += horizon_occlusion(h_pos) + horizon_occlusion(h_neg);
    }

    return total_occlusion / f32(NUM_DIRS);
}

// ============================================================
// SSAO pass
// ============================================================

@fragment
fn fs_ssao(in: VertexOutput) -> @location(0) vec4<f32> {
    let g = textureSample(t_gbuffer, s_lin, in.uv);
    let depth_norm = g.a;
    if (depth_norm >= 0.9999) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0); // sky = no AO
    }

    // G-buffer хранит WORLD-space нормаль. HBAO работает во view-space.
    let n_world = normalize(g.xyz);
    let normal = normalize((params.view * vec4<f32>(n_world, 0.0)).xyz);

    let view_pos = reconstruct_view_pos(in.uv, depth_norm);

    let occlusion = compute_hbao_occlusion(view_pos, normal, in.uv);

    // Strength: раньше `params.params.y` (ssao_strength) был
    // no-op (не использовался в шейдере). Теперь применяем здесь,
    // чтобы слайдер в Renderer panel работал.
    let strength = params.params.y;
    let ao = clamp(1.0 - occlusion * strength, 0.0, 1.0);

    return vec4<f32>(ao, 0.0, 0.0, 1.0);
}

// ============================================================
// Bilateral blur 5×5
// ============================================================
//
// Weight = exp(-depth_diff² · k) · exp(-xy_dist² / 2).
// При большом расхождении глубин weight → 0, поэтому края не
// размываются (это ключевое отличие от обычного box-blur,
// которое сделало бы AO «грязным» на границах объектов).
@fragment
fn fs_blur(in: VertexOutput) -> @location(0) vec4<f32> {
    let texel = 1.0 / vec2<f32>(textureDimensions(t_gbuffer));
    let center_g = textureSample(t_depth_src, s_lin, in.uv);
    let center_depth = center_g.a;

    var sum = 0.0;
    var w_sum = 0.0;

    for (var y = -2; y <= 2; y = y + 1) {
        for (var x = -2; x <= 2; x = x + 1) {
            let o = vec2<f32>(f32(x), f32(y));
            let suv = in.uv + o * texel;

            // `t_gbuffer` в blur-пасе — raw SSAO-текстура,
            // `t_depth_src` — gbuffer (для depth-веса).
            let ssao_sample = textureSample(t_gbuffer, s_lin, suv);
            let sample_depth = textureSample(t_depth_src, s_lin, suv).a;

            let depth_diff = abs(sample_depth - center_depth);
            let depth_w = exp(-depth_diff * depth_diff * 8000.0);

            let dist2 = dot(o, o);
            let spatial_w = exp(-dist2 * 0.5);

            let w = depth_w * spatial_w;
            sum += ssao_sample.r * w;
            w_sum += w;
        }
    }

    if (w_sum > 0.0) {
        sum /= w_sum;
    }
    return vec4<f32>(sum, 0.0, 0.0, 1.0);
}