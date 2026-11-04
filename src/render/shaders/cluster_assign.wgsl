// Clustered lighting — назначение point lights в кластеры.
//
// Один поток = один point light. Поток:
//   1. Трансформирует world-space позицию лайта в view-space.
//   2. Считает AABB (сфера радиуса range).
//   3. Определяет depth-slice range (линейные срезы по view-z).
//   4. Проецирует центр в screen-space, вычисляет screen-радиус.
//   5. Для каждого cluster-tile в bounding box × для каждого depth slice:
//      atomicAdd count++; записывает light_idx в слот, если ещё есть место.
//
// Dispatch: `(light_count + 63) / 64` workgroups по 64 потока.
//
// Ограничение: MAX_LIGHTS_PER_CLUSTER = 16 на кластер. Если в тайле
// больше лайтов — лишние отбрасываются (их вклад теряется). Для
// типичных сцен этого хватает; полноценный variable-size cluster list
// требует prefix-sum и появится в следующем спринте.

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

struct ClusterParams {
    // x = grid_x, y = grid_y, z = grid_z, w = point_light_count
    grid:     vec4<f32>,
    // xy = screen_size px, zw = 1/w, 1/h
    screen:   vec4<f32>,
    // x = near, y = far, z = 1/far, w = unused
    near_far: vec4<f32>,
    proj:     mat4x4<f32>,
    view:     mat4x4<f32>,
};

struct PointLight {
    // xyz = world-space position, w = range (m)
    position: vec4<f32>,
    // rgb = color, a = intensity (multiplier)
    color:    vec4<f32>,
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> params: ClusterParams;
@group(0) @binding(2) var<storage, read> point_lights: array<PointLight>;
@group(0) @binding(3) var<storage, read_write> cluster_counts: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> cluster_lights: array<u32>;

const MAX_LIGHTS_PER_CLUSTER: u32 = 16u;
const TILE_SIZE: f32 = 16.0;

@compute @workgroup_size(64, 1, 1)
fn cs_main(@builtin(global_invocation_id) gid: vec3<u32>) {
    let light_idx = gid.x;
    let light_count = u32(params.grid.w);
    if (light_idx >= light_count) { return; }

    let world_light = point_lights[light_idx];
    let world_pos = world_light.position.xyz;
    let range = world_light.position.w;
    if (range <= 0.0) { return; }

    // View-space позиция центра.
    let view_pos = (camera.view * vec4<f32>(world_pos, 1.0)).xyz;
    // Камера смотрит в -Z. Если центр сзади (view_pos.z > 0) — грубо
    // отсекаем (свет всё равно может доставать до краёв экрана, но
    // это редкий случай, и bounding box был бы огромным).
    if (view_pos.z >= 0.0) { return; }

    let grid_x = u32(params.grid.x);
    let grid_y = u32(params.grid.y);
    let grid_z = u32(params.grid.z);
    let screen_w = params.screen.x;
    let screen_h = params.screen.y;
    let near = params.near_far.x;
    let far = params.near_far.y;

    // AABB лайта в view-space.
    let z_to_camera_near = -view_pos.z - range; // ближайшая к камере точка (положительная дистанция)
    let z_to_camera_far = -view_pos.z + range;  // самая дальняя

    if (z_to_camera_far < near) { return; } // позади near plane
    // Кламп z_near_of_light до near (частично за near plane — всё равно виден).

    // Depth-slice range (линейный по view-z).
    let slice_f0 = max(0.0, (z_to_camera_near - near) / (far - near) * f32(grid_z));
    let slice_f1 = min(f32(grid_z - 1u), (z_to_camera_far - near) / (far - near) * f32(grid_z));
    if (slice_f1 < slice_f0) { return; }
    let z0 = u32(slice_f0);
    let z1 = u32(slice_f1);

    // Проекция центра.
    let clip_center = params.proj * vec4<f32>(view_pos, 1.0);
    if (clip_center.w <= 1e-4) { return; }
    let ndc_center = clip_center.xy / clip_center.w;
    let screen_cx = (ndc_center.x * 0.5 + 0.5) * screen_w;
    let screen_cy = (1.0 - (ndc_center.y * 0.5 + 0.5)) * screen_h;

    // Screen-радиус через проекцию точки на границе сферы по X.
    // Приближение: для view-space точки `center + (range, 0, 0)`.
    // Точнее было бы использовать угловой размер, но для culling'а
    // этого достаточно.
    let edge_x = view_pos + vec3<f32>(range, 0.0, 0.0);
    let clip_edge = params.proj * vec4<f32>(edge_x, 1.0);
    var screen_radius = 100.0;
    if (clip_edge.w > 1e-4) {
        let ndc_edge = clip_edge.xy / clip_edge.w;
        let screen_ex = (ndc_edge.x * 0.5 + 0.5) * screen_w;
        screen_radius = abs(screen_ex - screen_cx);
    }

    // Bounding box в кластерах.
    let fx0 = max(0.0, floor((screen_cx - screen_radius) / TILE_SIZE));
    let fy0 = max(0.0, floor((screen_cy - screen_radius) / TILE_SIZE));
    let fx1 = floor((screen_cx + screen_radius) / TILE_SIZE);
    let fy1 = floor((screen_cy + screen_radius) / TILE_SIZE);

    if (fx1 < 0.0 || fy1 < 0.0) { return; }
    if (fx0 > f32(grid_x - 1u) || fy0 > f32(grid_y - 1u)) { return; }

    let bx0 = u32(fx0);
    let by0 = u32(fy0);
    let bx1 = min(grid_x - 1u, u32(fx1));
    let by1 = min(grid_y - 1u, u32(fy1));

    // Обход. Порядок: z → y → x (z-срез самый внешний: важно для
    // depth-coherence, но в compute это роли не играет, всё равно).
    for (var z = z0; z <= z1; z = z + 1u) {
        for (var y = by0; y <= by1; y = y + 1u) {
            for (var x = bx0; x <= bx1; x = x + 1u) {
                let cluster_idx = x + y * grid_x + z * grid_x * grid_y;
                let old = atomicAdd(&cluster_counts[cluster_idx], 1u);
                if (old < MAX_LIGHTS_PER_CLUSTER) {
                    cluster_lights[cluster_idx * MAX_LIGHTS_PER_CLUSTER + old] = light_idx;
                }
            }
        }
    }
}