//! Атмосферные настройки и утилиты для процедурного неба.

use glam::Vec3;
use serde::{Deserialize, Serialize};

/// Настройки процедурной атмосферы.
///
/// Физика: см. `sky_atmosphere.wgsl`. Все поля сериализуются
/// в `settings.ron` (Sprint 5).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AtmosphereSettings {
    /// Включена ли процедурная атмосфера. Если `false`, используется
    /// глобальный IBL (sky.hdr) как раньше.
    #[serde(default)]
    pub enabled: bool,

    /// Время суток: 0.0 = полночь, 6.0 = восход, 12.0 = полдень,
    /// 18.0 = закат. Солнце автоматически вращается по этому значению.
    #[serde(default = "default_noon")]
    pub time_of_day: f32,

    /// Общая интенсивность атмосферы (HDR multiplier). 22.0 — «нормальное»
    /// солнце. Уменьшайте для пасмурной погоды, увеличивайте для
    /// «кинематографичного» заката.
    #[serde(default = "default_sun_intensity")]
    pub sun_intensity: f32,

    /// Turbidity (мутность атмосферы): 1.0 — ясно, 5.0 — обычная дымка,
    /// 10.0 — смог. Влияет на Mie-рассеяние.
    #[serde(default = "default_turbidity")]
    pub turbidity: f32,

    /// Усиление Rayleigh (синяя дымка). 1.0 — стандарт. Меньше — меньше
    /// синевы, больше — «марсианская» атмосфера.
    #[serde(default = "default_rayleigh")]
    pub rayleigh_strength: f32,

    /// Усиление Mie (аэрозоли/ореол вокруг солнца). 1.0 — стандарт.
    #[serde(default = "default_mie")]
    pub mie_strength: f32,

    /// Плотность высотной дымки у горизонта (0.0 = выкл., 0.5 = обычная,
    /// 1.0 = плотный туман вдалеке). Не физика — просто декорация поверх
    /// атмосферы.
    #[serde(default)]
    pub horizon_haze: f32,
}

fn default_noon() -> f32 { 12.0 }
fn default_sun_intensity() -> f32 { 22.0 }
fn default_turbidity() -> f32 { 3.0 }
fn default_rayleigh() -> f32 { 1.0 }
fn default_mie() -> f32 { 1.0 }

impl Default for AtmosphereSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            time_of_day: default_noon(),
            sun_intensity: default_sun_intensity(),
            turbidity: default_turbidity(),
            rayleigh_strength: default_rayleigh(),
            mie_strength: default_mie(),
            horizon_haze: 0.3,
        }
    }
}

impl AtmosphereSettings {
    pub fn validate(&mut self) {
        self.time_of_day = self.time_of_day.rem_euclid(24.0);
        self.sun_intensity = self.sun_intensity.clamp(1.0, 100.0);
        self.turbidity = self.turbidity.clamp(1.0, 10.0);
        self.rayleigh_strength = self.rayleigh_strength.clamp(0.1, 5.0);
        self.mie_strength = self.mie_strength.clamp(0.0, 10.0);
        self.horizon_haze = self.horizon_haze.clamp(0.0, 1.0);
    }
}

/// Направление ОТ камеры К солнцу для данного `time_of_day`.
///
/// Модель:
///   * `time = 6`  → солнце на востоке (+X), восход;
///   * `time = 12` → солнце в зените (+Y), полдень;
///   * `time = 18` → солнце на западе (-X), закат;
///   * `time = 0`  → солнце под горизонтом (-Y), полночь.
///
/// Возвращает единичный вектор.
pub fn time_to_sun_dir(time_of_day: f32) -> Vec3 {
    let theta = (time_of_day.rem_euclid(24.0) - 6.0) / 12.0 * std::f32::consts::PI;
    Vec3::new(theta.cos(), theta.sin(), 0.0)
}

/// Интенсивность, скорректированная под высоту солнца.
///
/// Днём — полная. Ночью — слабый moonlight (~0.05% от полной).
/// В сумерках (sun.y ∈ [-0.1, 0.1]) — плавный переход.
pub fn sun_intensity_at(time_of_day: f32, base_intensity: f32) -> f32 {
    let sun = time_to_sun_dir(time_of_day);
    let day = smoothstep(-0.1, 0.1, sun.y);
    let night_intensity = 0.02 * base_intensity;
    night_intensity + (base_intensity - night_intensity) * day
}

fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}