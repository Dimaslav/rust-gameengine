// Real-time atmospheric scattering (Rayleigh + Mie).
//
// Модель: сферическая атмосфера вокруг сферической планеты, наблюдатель
// на поверхности, лучи пускаются в небо. Интегрируем single scattering
// вдоль primary ray, для каждой точки интегрируем optical depth до
// солнца вдоль light ray.
//
// Параметры (в метрах):
//   Rg = 6360 km   — радиус планеты
//   Ra = 6420 km   — радиус атмосферы (60 km над уровнем моря)
//   H_r = 8000 m   — scale height для Rayleigh (синяя дымка)
//   H_m = 1200 m   — scale height для Mie (аэрозоли/пыль)
//   β_r = (5.8, 13.5, 33.1) · 1e-6 / m   — коэффициенты рассеяния Rayleigh
//   β_m = 21 · 1e-6 / m                   — коэффициент рассеяния Mie
//   g   = 0.76                            — anisotropy Mie (Henyey-Greenstein)
//
// Приближения:
//   * Планета считается идеальной сферой без рельефа.
//   * Наблюдатель всегда на поверхности (Rg + camera_height).
//   * Ozone-слой не моделируется (добавит фиолетовый оттенок в зените,
//     редко нужно).
//   * Нет multiple scattering (даёт недооценку голубизны в зените —
//     компенсируется `sun_intensity`).
//   * Phase function — HG для Mie, Rayleigh phase для Rayleigh.

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

struct AtmosphereParams {
    // xyz = sun direction (normalized, from camera TO sun). w = sun intensity.
    sun: [f32; 4],
    // x = turbidity (1..10), y = rayleigh_strength (0.5..2.0), z = mie_strength (0.1..3.0),
    // w = time (для future animation, сейчас unused).
    params: [f32; 4],
    // x = camera height над уровнем моря (m), y = fog density (используется как
    // высотный туман в дополнение к атмосфере), zw = unused.
    misc: [f32; 4],
};

@group(0) @binding(0) var<uniform> camera: Camera;
@group(0) @binding(1) var<uniform> atmosphere: AtmosphereParams;

// === Физические константы ===
const Rg: f32 = 6360000.0;
const Ra: f32 = 6420000.0;
const H_r: f32 = 8000.0;
const H_m: f32 = 1200.0;
const PI: f32 = 3.14159265359;

// Базовые коэффициенты (в 1/м). Turbidity и strengths умножаются позже.
const BETA_R_BASE: vec3<f32> = vec3<f32>(5.8e-6, 13.5e-6, 33.1e-6);
const BETA_M_BASE: f32 = 21e-6;

struct VOut {
    @builtin(position) @invariant clip: vec4<f32>,
    @location(0) ndc: vec2<f32>,
};

@vertex
fn vs_main(@builtin(vertex_index) idx: u32) -> VOut {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>( 3.0, -1.0),
        vec2<f32>(-1.0,  3.0),
    );
    var out: VOut;
    let p = positions[idx];
    out.clip = vec4<f32>(p, 1.0, 1.0);
    out.ndc = p;
    return out;
}

// ============================================================
// Ray-sphere intersect
// ============================================================

/// Возвращает ближайший положительный t, или -1 если нет попадания.
/// origin — снаружи сферы (проверяется вызывающим).
fn ray_sphere_near(origin: vec3<f32>, dir: vec3<f32>, radius: f32) -> f32 {
    let b = dot(origin, dir);
    let c = dot(origin, origin) - radius * radius;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    let sq = sqrt(disc);
    let t1 = -b - sq;
    let t2 = -b + sq;
    if (t1 > 0.0) { return t1; }
    if (t2 > 0.0) { return t2; }
    return -1.0;
}

/// Возвращает дальний t (положительный), или -1.
fn ray_sphere_far(origin: vec3<f32>, dir: vec3<f32>, radius: f32) -> f32 {
    let b = dot(origin, dir);
    let c = dot(origin, origin) - radius * radius;
    let disc = b * b - c;
    if (disc < 0.0) { return -1.0; }
    let sq = sqrt(disc);
    let t2 = -b + sq;
    if (t2 > 0.0) { return t2; }
    return -1.0;
}

// ============================================================
// Phase functions
// ============================================================

/// Rayleigh phase (симметричная, слабая).
fn phase_rayleigh(mu: f32) -> f32 {
    return 3.0 / (16.0 * PI) * (1.0 + mu * mu);
}

/// Henyey-Greenstein phase для Mie. g > 0 → forward scattering
/// (яркий ореол вокруг солнца).
fn phase_mie(mu: f32, g: f32) -> f32 {
    let g2 = g * g;
    let denom = 1.0 + g2 - 2.0 * g * mu;
    return (1.0 - g2) / (4.0 * PI * pow(max(denom, 1e-4), 1.5));
}

// ============================================================
// Optical depth к солнцу
// ============================================================

/// Интегрирует оптическую толщину от точки `p` до границы атмосферы
/// вдоль направления `sun_dir`. 8 шагов — достаточно для плавного
/// заката, при 16+ разница незаметна.
fn light_optical_depth(p: vec3<f32>, sun_dir: vec3<f32>, beta_r: vec3<f32>, beta_m: f32) -> vec3<f32> {
    let t_atm = ray_sphere_far(p, sun_dir, Ra);
    if (t_atm < 0.0) {
        // Направление не пересекает атмосферу (солнце вне конуса) —
        // возвращаем 0 (полностью прозрачно).
        return vec3<f32>(0.0);
    }

    // Ground shadow: если light ray пересекает планету — точка в тени.
    // Это даёт корректное затемнение при закате (солнце под горизонтом
    // → атмосфера над горизонтом не освещена).
    let t_ground = ray_sphere_near(p, sun_dir, Rg);
    if (t_ground > 0.0 && t_ground < t_atm) {
        // Полная тень: 100 единиц оптической толщины = exp(-100) ≈ 0.
        return vec3<f32>(100.0);
    }

    let steps: i32 = 8;
    let step_size = t_atm / f32(steps);
    var depth_r = 0.0;
    var depth_m = 0.0;

    for (var i: i32 = 0; i < steps; i = i + 1) {
        let lp = p + sun_dir * (f32(i) + 0.5) * step_size;
        let h = max(length(lp) - Rg, 0.0);
        depth_r += exp(-h / H_r) * step_size;
        depth_m += exp(-h / H_m) * step_size;
    }

    return beta_r * depth_r + vec3<f32>(beta_m * depth_m);
}

// ============================================================
// Main atmosphere ray
// ============================================================

/// Возвращает HDR-цвет атмосферы для луча (origin, dir).
/// origin — позиция наблюдателя в world space (camera).
/// dir — направление взгляда (normalized).
///
/// Если луч смотрит ниже горизонта (dir.y < -0.1) и не пересекает
/// атмосферу — возвращает чёрный. В противном случае считает
/// single scattering.
fn atmosphere_ray(origin: vec3<f32>, dir: vec3<f32>) -> vec3<f32> {
    let sun_dir = atmosphere.sun.xyz;
    let sun_intensity = atmosphere.sun.w;
    let turbidity = atmosphere.params.x;
    let r_strength = atmosphere.params.y;
    let m_strength = atmosphere.params.z;

    // Модифицированные коэффициенты.
    let beta_r = BETA_R_BASE * r_strength;
    let beta_m = BETA_M_BASE * m_strength * (1.0 + (turbidity - 1.0) * 0.15);

    // Пересечение с верхней границей атмосферы.
    let t_atm = ray_sphere_far(origin, dir, Ra);
    if (t_atm < 0.0) {
        return vec3<f32>(0.0);
    }

    // Пересечение с землёй — если есть, ограничиваем интеграл.
    // Для неба t_max = t_atm (чистое небо). Для лучей ниже горизонта
    // большая часть пути — под землёй, что даёт закатный оранжевый
    // цвет. Ограничение t_ground убирает это, поэтому НЕ ограничиваем —
    // пусть атмосфера интегрируется полностью (планета потом перекроет
    // эти пиксели через depth test).
    let t_max = t_atm;

    // === Raymarch primary ray ===
    let primary_steps: i32 = 32;
    let step_size = t_max / f32(primary_steps);

    var total_r = 0.0;
    var total_m = 0.0;
    var transmittance = vec3<f32>(1.0);

    let mu = dot(dir, sun_dir);
    let phase_r = phase_rayleigh(mu);
    let phase_m = phase_mie(mu, 0.76);

    for (var i: i32 = 0; i < primary_steps; i = i + 1) {
        let t_i = (f32(i) + 0.5) * step_size;
        let p = origin + dir * t_i;
        let h = max(length(p) - Rg, 0.0);
        let rho_r = exp(-h / H_r);
        let rho_m = exp(-h / H_m);

        // Optical depth до солнца.
        let light_t = light_optical_depth(p, sun_dir, beta_r, beta_m);
        let light_trans = exp(-light_t);

        // Накопление.
        total_r += rho_r * light_trans.x * step_size;
        total_m += rho_m * light_trans.x * step_size;

        // Ослабление вдоль primary ray.
        let ext = beta_r * rho_r + vec3<f32>(beta_m * rho_m);
        transmittance *= exp(-ext * step_size);

        // Ранний выход если почти полностью поглощено.
        if (max(transmittance.x, max(transmittance.y, transmittance.z)) < 0.01) {
            break;
        }
    }

    // Вклад от Rayleigh (синяя дымка) и Mie (аэрозоль/ореол).
    let color_r = beta_r * total_r * phase_r;
    // Mie — серо-белый (у аэрозоля нет цветовой селективности).
    let color_m = vec3<f32>(beta_m * total_m * phase_m);

    // Итоговый цвет с учётом transmittance (чтобы не подсвечивать
    // атмосферу насквозь через плотные слои).
    let avg_trans = (transmittance.x + transmittance.y + transmittance.z) / 3.0;
    return (color_r + color_m) * sun_intensity * avg_trans;
}

/// Полезный солярный тон для случая, когда atmosphere слабая —
/// даёт базовую синеву неба при `sun.y > 0`.
fn sky_baseline(sun_y: f32) -> vec3<f32> {
    let day = clamp(sun_y, 0.0, 1.0);
    let night = 1.0 - day;
    let day_color = vec3<f32>(0.35, 0.55, 0.95) * 0.8;
    let sunset = vec3<f32>(0.95, 0.45, 0.2) * 0.4;
    let night_color = vec3<f32>(0.02, 0.03, 0.08);
    // Плавный переход day → sunset → night.
    let sunset_weight = smoothstep(0.0, 0.15, 0.15 - day);
    let base = mix(day_color, sunset, sunset_weight) * day;
    return base + night_color * night;
}

// ============================================================
// Fragment
// ============================================================

@fragment
fn fs_main(in: VOut) -> @location(0) vec4<f32> {
    // Восстанавливаем направление луча из NDC через inv_view_proj.
    let near_h = camera.inv_view_proj * vec4<f32>(in.ndc, 0.0, 1.0);
    let far_h  = camera.inv_view_proj * vec4<f32>(in.ndc, 1.0, 1.0);
    let near_w = near_h.xyz / max(near_h.w, 1e-6);
    let far_w  = far_h.xyz  / max(far_h.w,  1e-6);
    let dir = normalize(far_w - near_w);

    // Камера в "планетарной" системе координат: origin на поверхности.
    let cam_height = max(atmosphere.misc.x, 1.0);
    let origin = vec3<f32>(0.0, Rg + cam_height, 0.0);

    // Считаем атмосферный свет.
    var color = atmosphere_ray(origin, dir);

    // Fallback: если total color близок к нулю (ночь, солнце за
    // горизонтом), добавляем лёгкий baseline, чтобы небо не было
    // абсолютно чёрным.
    let sun_y = atmosphere.sun.y;
    let baseline = sky_baseline(sun_y);
    color += baseline * 0.15;

    // Простая высотная дымка у горизонта (поверх атмосферы).
    let fog_density = atmosphere.misc.y;
    if (fog_density > 0.0) {
        let horizon_fade = pow(clamp(1.0 - abs(dir.y), 0.0, 1.0), 4.0);
        let fog_color = vec3<f32>(0.75, 0.72, 0.62);
        color = mix(color, fog_color, horizon_fade * fog_density * 0.5);
    }

    return vec4<f32>(max(color, vec3<f32>(0.0)), 1.0);
}