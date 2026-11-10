use glam::Vec3;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[derive(Default)]
pub enum AudioBus {
    Master,
    #[default]
    Sfx,
    Music,
    Voice,
    Ui,
}

impl AudioBus {
    pub const ALL: [AudioBus; 5] = [
        AudioBus::Master,
        AudioBus::Sfx,
        AudioBus::Music,
        AudioBus::Voice,
        AudioBus::Ui,
    ];

    pub fn name(self) -> &'static str {
        match self {
            AudioBus::Master => "Master",
            AudioBus::Sfx => "SFX",
            AudioBus::Music => "Music",
            AudioBus::Voice => "Voice",
            AudioBus::Ui => "UI",
        }
    }

    pub fn default_volume(self) -> f32 {
        match self {
            AudioBus::Master => 1.0,
            AudioBus::Sfx => 1.0,
            AudioBus::Music => 0.7,
            AudioBus::Voice => 1.0,
            AudioBus::Ui => 0.9,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSource {
    pub sound: String,
    pub bus: AudioBus,
    pub volume: f32,
    pub pitch: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub looping: bool,
    pub playing: bool,
    pub occlusion: bool,
    pub occlusion_volume: f32,
}

impl AudioSource {
    pub fn new(sound: impl Into<String>) -> Self {
        Self {
            sound: sound.into(),
            bus: AudioBus::Sfx,
            volume: 1.0,
            pitch: 1.0,
            min_distance: 1.0,
            max_distance: 20.0,
            looping: false,
            playing: true,
            occlusion: true,
            occlusion_volume: 0.3,
        }
    }

    pub fn non_spatial(sound: impl Into<String>) -> Self {
        Self {
            max_distance: 0.0,
            occlusion: false,
            ..Self::new(sound)
        }
    }

    pub fn looping(sound: impl Into<String>) -> Self {
        Self { looping: true, ..Self::new(sound) }
    }

    pub fn with_bus(mut self, bus: AudioBus) -> Self { self.bus = bus; self }
    pub fn with_volume(mut self, v: f32) -> Self { self.volume = v; self }
    pub fn with_pitch(mut self, p: f32) -> Self { self.pitch = p.max(0.01); self }
    pub fn with_range(mut self, min: f32, max: f32) -> Self {
        self.min_distance = min.max(0.0);
        self.max_distance = max.max(self.min_distance);
        self
    }
    pub fn with_occlusion(mut self, enabled: bool, volume_when_blocked: f32) -> Self {
        self.occlusion = enabled;
        self.occlusion_volume = volume_when_blocked.clamp(0.0, 1.0);
        self
    }
}

impl Default for AudioSource {
    fn default() -> Self { Self::new("pickup") }
}

// ============================================================
// Distance attenuation
// ============================================================

/// Квадратичное затухание: 1.0 внутри `min_dist`, 0.0 за `max_dist`.
pub fn attenuation(dist: f32, min_dist: f32, max_dist: f32) -> f32 {
    if max_dist <= min_dist || max_dist <= 0.0 {
        return 1.0;
    }
    if dist <= min_dist { return 1.0; }
    if dist >= max_dist { return 0.0; }
    let t = (dist - min_dist) / (max_dist - min_dist);
    (1.0 - t) * (1.0 - t)
}

/// Позиция entity в мире для целей audio.
///
/// ИЗМЕНЕНО (bugfix #4): теперь учитывает Parent-цепочку через
/// `world_position`. До этого возвращалась локальная
/// `Transform.position`, и звук дочерней сущности звучал из
/// родительского origin-а, а не из фактической позиции.
pub fn source_position(world: &crate::ecs::World, entity: crate::ecs::Entity) -> Vec3 {
    crate::game::world_position(world, entity).unwrap_or(Vec3::ZERO)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attenuation_inside_min() {
        assert_eq!(attenuation(0.5, 1.0, 10.0), 1.0);
        assert_eq!(attenuation(1.0, 1.0, 10.0), 1.0);
    }

    #[test]
    fn attenuation_outside_max() {
        assert_eq!(attenuation(10.0, 1.0, 10.0), 0.0);
        assert_eq!(attenuation(50.0, 1.0, 10.0), 0.0);
    }

    #[test]
    fn attenuation_midpoint() {
        let a = attenuation(5.5, 1.0, 10.0);
        assert!((a - 0.25).abs() < 1e-4, "expected 0.25, got {}", a);
    }

    #[test]
    fn attenuation_no_spatial() {
        assert_eq!(attenuation(100.0, 1.0, 0.0), 1.0);
        assert_eq!(attenuation(100.0, 5.0, 5.0), 1.0);
    }

    #[test]
    fn bus_default_and_name() {
        assert_eq!(AudioBus::default(), AudioBus::Sfx);
        assert_eq!(AudioBus::Master.name(), "Master");
        assert_eq!(AudioBus::ALL.len(), 5);
        for b in AudioBus::ALL {
            let v = b.default_volume();
            assert!((0.0..=1.0).contains(&v), "{:?} default {}", b, v);
        }
    }

    #[test]
    fn builder_chain() {
        let src = AudioSource::new("hit")
            .with_bus(AudioBus::Ui)
            .with_volume(0.5)
            .with_pitch(1.5)
            .with_range(2.0, 8.0)
            .with_occlusion(false, 0.0);
        assert_eq!(src.bus, AudioBus::Ui);
        assert_eq!(src.volume, 0.5);
        assert_eq!(src.pitch, 1.5);
        assert_eq!(src.min_distance, 2.0);
        assert_eq!(src.max_distance, 8.0);
        assert!(!src.occlusion);
        assert!((src.occlusion_volume - 0.0).abs() < 1e-6);
    }

    #[test]
    fn occlusion_volume_clamps() {
        let src = AudioSource::new("x").with_occlusion(true, 5.0);
        assert!((src.occlusion_volume - 1.0).abs() < 1e-6);
        let src = AudioSource::new("x").with_occlusion(true, -3.0);
        assert!((src.occlusion_volume - 0.0).abs() < 1e-6);
    }
}