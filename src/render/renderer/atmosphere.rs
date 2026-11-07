use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct AtmosphereSettings {
    #[serde(default)]
    pub enabled: bool,

    #[serde(default = "default_noon")]
    pub time_of_day: f32,

    #[serde(default = "default_sun_intensity")]
    pub sun_intensity: f32,

    #[serde(default = "default_turbidity")]
    pub turbidity: f32,

    #[serde(default = "default_rayleigh")]
    pub rayleigh_strength: f32,

    #[serde(default = "default_mie")]
    pub mie_strength: f32,

    #[serde(default)]
    pub horizon_haze: f32,
}

fn default_noon() -> f32 { 12.0 }
fn default_sun_intensity() -> f32 { 12.0 }
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

pub fn time_to_sun_dir(time_of_day: f32) -> Vec3 {
    let theta = (time_of_day.rem_euclid(24.0) - 6.0) / 12.0 * std::f32::consts::PI;
    Vec3::new(theta.cos(), theta.sin(), 0.0)
}

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