// Screen-space contact shadows.
//
// Для каждого пикселя:
//   1. Восстанавливаем world-space позицию из NDC depth через inv_view_proj,
//      затем переводим её в view-space через view.
//   2. Ray-march в направлении солнца (в view-space) на ~0.5 м.
//   3. На каждом шаге проецируем ray-point в UV, читаем depth.
//   4. Если sample ближе к камере, чем ray-point на толщину > thickness
//      и в пределах ray_length — occlusion.
//
// Результат: R-канал = visibility (1.0 = освещён, 0.0 = в тени).
// Composite в `deferred_lighting.wgsl`: directional light shadow
// умножается на этот visibility.
//
// # WGSL-замечания
//
// * `inverse()` для матриц в WGSL **отсутствует**. Поэтому мы не
//   вычисляем обратную матрицу проекции, а восстанавливаем
//   world-space позицию через `camera.inv_view_proj`, после чего
//   переводим её в view-space через `camera.view`.
// * `textureSampleLevel` для `texture_depth_2d` возвращает `f32`,
//   а не `vec4<f32>` → нельзя писать `.r`.
// * Параметр `level` для depth-текстур — `i32` (номер мипа),
//   для обычных — `f32`.

struct Camera {
    view_proj:      mat4x4<f32>,
    inv_view_proj:  mat4x4<f32>,
    view:           mat4x4<f32>,
    inv_view:       mat4x4<f32>,
    camera_pos:     vec4<f32>,
    near_far:       vec4<f32>,
    prev_view_proj: mat4x4<f32>,
    screen_size:    vec4<f32>,
};

struct ContactShadowParams {
    // x = ray length (m), y = thickness (m), z = strength, w = unused
    params: vec4<f32>,
    // xyz = sun direction in view-space (normalized), w = unused
    sun_view_dir: vec4<f32>,
};

@group(0) @binding(0) var t_depth: texture_depth_2d;
@group(0) @binding(1) var s_lin: sampler;
@group(0) @binding(2) var<uniform> camera: Camera;
@group(0) @binding(3) var<uniform> contact: ContactShadowParams;

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

/// Восстанавливает view-space позицию из UV и NDC depth.
///
/// Вместо обратной матрицы проекции (которой нет в WGSL) используем
/// цепочку: NDC → world-space (через `camera.inv_view_proj`) →
/// view-space (через `camera.view`).
fn reconstruct_view_pos(uv: vec2<f32>, ndc_depth: f32) -> vec3<f32> {
    let ndc = vec4<f32>(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, ndc_depth, 1.0);
    let world_h = camera.inv_view_proj * ndc;
    let world = world_h.xyz / max(world_h.w, 1e-6);
    let view_h = camera.view * vec4<f32>(world, 1.0);
    return view_h.xyz;
}

/// Проекция view-space точки в UV.
/// `proj` передаётся как `view_proj * inv_view` — это и есть матрица
/// проекции, потому что `view_proj = proj * view`.
fn project_view_to_uv(view_pos: vec3<f32>, proj: mat4x4<f32>) -> vec2<f32> {
    let clip = proj * vec4<f32>(view_pos, 1.0);
    if (clip.w <= 1e-6) { return vec2<f32>(-1.0); }
    let ndc = clip.xy / clip.w;
    return vec2<f32>(ndc.x * 0.5 + 0.5, 1.0 - (ndc.y * 0.5 + 0.5));
}

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    let strength = contact.params.z;
    if (strength < 0.001) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    // textureSampleLevel на texture_depth_2d возвращает f32.
    // level для depth-текстуры — i32 (0, не 0.0).
    let ndc_depth = textureSampleLevel(t_depth, s_lin, in.uv, 0);
    if (ndc_depth >= 0.9999) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }

    let ray_length = contact.params.x;
    let thickness = contact.params.y;

    let light_dir_raw = contact.sun_view_dir.xyz;
    if (length(light_dir_raw) < 0.001) {
        return vec4<f32>(1.0, 0.0, 0.0, 1.0);
    }
    let light_dir = normalize(light_dir_raw);

    // proj = proj * view * inv_view = proj.
    // Вычисляем один раз на фрагмент — используется в цикле.
    // (inverse здесь больше не нужен, см. reconstruct_view_pos.)
    let proj = camera.view_proj * camera.inv_view;

    let view_pos = reconstruct_view_pos(in.uv, ndc_depth);

    let steps = 12;
    let step_size = ray_length / f32(steps);
    let start_offset = 0.01;

    var ray_pos = view_pos + light_dir * start_offset;
    var occlusion = 0.0;

    for (var i = 0; i < steps; i = i + 1) {
        ray_pos = ray_pos + light_dir * step_size;
        let sample_uv = project_view_to_uv(ray_pos, proj);
        if (sample_uv.x < 0.0 || sample_uv.x > 1.0
            || sample_uv.y < 0.0 || sample_uv.y > 1.0) {
            break;
        }
        // Опять: depth-текстура → f32, level = i32.
        let sample_ndc = textureSampleLevel(t_depth, s_lin, sample_uv, 0);
        if (sample_ndc >= 0.9999) {
            continue;
        }
        let sample_view = reconstruct_view_pos(sample_uv, sample_ndc);
        let expected_z = -ray_pos.z;
        let sample_z = -sample_view.z;

        // Occlusion: sample ближе к камере, чем ray-point, на
        // величину > thickness, но в пределах ray_length.
        if (sample_z < expected_z - thickness
            && sample_z > expected_z - ray_length) {
            occlusion = 1.0;
            break;
        }
    }

    let visibility = 1.0 - occlusion * strength;
    return vec4<f32>(visibility, 0.0, 0.0, 1.0);
}