use std::collections::HashMap;
use std::num::NonZero;
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{bail, Context};
use glam::Vec3;
use rodio::source::Source;
use rodio::stream::{DeviceSinkBuilder, MixerDeviceSink};
use rodio::Player;

use crate::ecs::{Entity, World};
use crate::game::audio::{attenuation, source_position, AudioBus, AudioSource};

#[derive(Clone)]
struct SoundEntry {
    samples: Arc<Vec<f32>>,
    sample_rate: u32,
}

struct SharedSamples {
    data: Arc<Vec<f32>>,
    pos: usize,
    sample_rate: u32,
}

impl Iterator for SharedSamples {
    type Item = f32;
    fn next(&mut self) -> Option<f32> {
        let s = self.data.get(self.pos).copied();
        if s.is_some() {
            self.pos += 1;
        }
        s
    }
}

impl Source for SharedSamples {
    fn current_span_len(&self) -> Option<usize> { None }
    fn channels(&self) -> NonZero<u16> {
        NonZero::new(1).unwrap()
    }
    fn sample_rate(&self) -> NonZero<u32> {
        NonZero::new(self.sample_rate).unwrap_or(NonZero::new(44_100).unwrap())
    }
    fn total_duration(&self) -> Option<Duration> { None }
}

struct ActiveSound {
    player: Player,
    last_volume: f32,
    last_pitch: f32,
    last_occlusion: f32,
}

pub struct AudioSystem {
    _stream: MixerDeviceSink,
    cache: HashMap<String, SoundEntry>,
    bus_volumes: HashMap<AudioBus, f32>,
    active: HashMap<Entity, ActiveSound>,
}

pub fn effective_bus_volume(
    bus_volumes: &HashMap<AudioBus, f32>,
    bus: AudioBus,
) -> f32 {
    let v = bus_volumes.get(&bus).copied().unwrap_or(1.0);
    if bus == AudioBus::Master {
        v
    } else {
        v * bus_volumes.get(&AudioBus::Master).copied().unwrap_or(1.0)
    }
}

impl AudioSystem {
    pub fn new() -> Option<Self> {
        let stream = match DeviceSinkBuilder::open_default_sink() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("audio: cannot open output stream: {}", e);
                return None;
            }
        };

        let sr = 44_100u32;
        let mut cache = HashMap::new();
        cache.insert("shot".into(), SoundEntry { samples: Arc::new(gen_shot(sr)), sample_rate: sr });
        cache.insert("explosion".into(), SoundEntry { samples: Arc::new(gen_explosion(sr)), sample_rate: sr });
        cache.insert("pickup".into(), SoundEntry { samples: Arc::new(gen_pickup(sr)), sample_rate: sr });
        cache.insert("ding".into(), SoundEntry { samples: Arc::new(gen_ding(sr)), sample_rate: sr });

        let bus_volumes = AudioBus::ALL.iter().map(|b| (*b, b.default_volume())).collect();

        log::info!(
            "audio: initialized @ {} Hz, {} procedural sounds, {} buses",
            sr, cache.len(), AudioBus::ALL.len(),
        );

        Some(Self {
            _stream: stream,
            cache,
            bus_volumes,
            active: HashMap::new(),
        })
    }

    pub fn register(&mut self, name: impl Into<String>, samples: Vec<f32>) {
        let sr = 44_100u32;
        self.cache.insert(name.into(), SoundEntry { samples: Arc::new(samples), sample_rate: sr });
    }

    pub fn load_sound_from_file(
        &mut self,
        name: impl Into<String>,
        path: impl AsRef<Path>,
    ) -> anyhow::Result<()> {
        let name: String = name.into();
        let path = path.as_ref();

        let file = std::fs::File::open(path)
            .with_context(|| format!("open audio {}", path.display()))?;
        let reader = std::io::BufReader::new(file);
        let decoder = rodio::Decoder::new(reader)
            .with_context(|| format!("decode audio {}", path.display()))?;
        let channels = decoder.channels().get() as usize;
        let sample_rate = decoder.sample_rate().get();
        let raw: Vec<f32> = decoder.collect();
        let samples = if channels > 1 {
            let frames = raw.len() / channels;
            let mut out = Vec::with_capacity(frames);
            let inv = 1.0 / channels as f32;
            for f in 0..frames {
                let mut sum = 0.0f32;
                for c in 0..channels { sum += raw[f * channels + c]; }
                out.push(sum * inv);
            }
            out
        } else {
            raw
        };

        if samples.is_empty() {
            bail!("decoded '{}' to empty sample buffer", path.display());
        }

        let dur_s = samples.len() as f32 / sample_rate.max(1) as f32;
        log::info!(
            "audio: loaded '{}' from {} ({:.2}s, {} Hz, {} channels → mono)",
            name, path.display(), dur_s, sample_rate, channels,
        );

        self.cache.insert(name, SoundEntry { samples: Arc::new(samples), sample_rate });
        Ok(())
    }

    pub fn sound_names(&self) -> Vec<&str> {
        let mut v: Vec<&str> = self.cache.keys().map(|s| s.as_str()).collect();
        v.sort();
        v
    }

    pub fn has_sound(&self, name: &str) -> bool { self.cache.contains_key(name) }

    pub fn set_bus_volume(&mut self, bus: AudioBus, volume: f32) {
        let v = volume.clamp(0.0, 2.0);
        self.bus_volumes.insert(bus, v);
    }

    pub fn bus_volume(&self, bus: AudioBus) -> f32 {
        self.bus_volumes.get(&bus).copied().unwrap_or(1.0)
    }

    pub fn bus_volume_effective(&self, bus: AudioBus) -> f32 {
        effective_bus_volume(&self.bus_volumes, bus)
    }

    pub fn all_bus_volumes(&self) -> Vec<(AudioBus, f32)> {
        AudioBus::ALL.iter().map(|b| (*b, self.bus_volume(*b))).collect()
    }

    pub fn play(&mut self, name: &str) { self.play_on_bus(name, AudioBus::Sfx); }

    pub fn play_on_bus(&mut self, name: &str, bus: AudioBus) {
        self.play_on_bus_with_volume(name, bus, 1.0);
    }

    pub fn play_on_bus_with_volume(&mut self, name: &str, bus: AudioBus, extra_volume: f32) {
        let Some(entry) = self.cache.get(name).cloned() else {
            log::debug!("audio: sound '{}' not found", name);
            return;
        };
        let volume = (extra_volume * self.bus_volume_effective(bus)).clamp(0.0, 4.0);
        if volume <= 1e-4 { return; }
        let source = SharedSamples {
            data: entry.samples, pos: 0, sample_rate: entry.sample_rate,
        };
        let player = Player::connect_new(&self._stream.mixer());
        player.set_volume(volume);
        player.append(source);
        player.detach();
    }

    pub fn play_at(&mut self, name: &str, position: Vec3, listener: Vec3) {
        let Some(entry) = self.cache.get(name).cloned() else {
            log::debug!("audio: sound '{}' not found (play_at)", name);
            return;
        };
        let dist = (position - listener).length();
        let atten = attenuation(dist, 1.0, 20.0);
        let bus_vol = self.bus_volume_effective(AudioBus::Sfx);
        let volume = (atten * bus_vol).clamp(0.0, 4.0);
        if volume <= 1e-4 {
            return;
        }
        let source = SharedSamples {
            data: entry.samples,
            pos: 0,
            sample_rate: entry.sample_rate,
        };
        let player = Player::connect_new(&self._stream.mixer());
        player.set_volume(volume);
        player.append(source);
        player.detach();
    }

    pub fn update(&mut self, world: &mut World, listener_pos: Vec3, dt: f32) {
        let sources: Vec<(Entity, AudioSource, Vec3)> = world
            .query::<AudioSource>()
            .map(|(e, src)| (e, src.clone(), source_position(world, e)))
            .collect();

        let alive: std::collections::HashSet<Entity> =
            sources.iter().map(|(e, _, _)| *e).collect();

        let mut to_remove: Vec<Entity> = Vec::new();
        let mut finished: Vec<Entity> = Vec::new();
        for (e, active) in self.active.iter() {
            if active.player.empty() {
                to_remove.push(*e);
                if alive.contains(e)
                    && let Some(src) = world.get::<AudioSource>(*e)
                    && src.playing
                    && !src.looping
                {
                    finished.push(*e);
                }
            } else if !alive.contains(e) {
                to_remove.push(*e);
            }
        }
        for e in to_remove {
            self.active.remove(&e);
        }
        for e in finished {
            if let Some(src) = world.get_mut::<AudioSource>(e) {
                src.playing = false;
            }
        }

        for (e, src, pos) in sources {
            if !src.playing {
                if let Some(active) = self.active.remove(&e) { active.player.stop(); }
                continue;
            }

            if !self.active.contains_key(&e) {
                let Some(entry) = self.cache.get(&src.sound).cloned() else {
                    log::debug!("audio: entity #{} wants '{}', not in cache", e, src.sound);
                    continue;
                };
                let source = SharedSamples {
                    data: entry.samples, pos: 0, sample_rate: entry.sample_rate,
                };
                let player = Player::connect_new(&self._stream.mixer());
                let initial_vol = (src.volume * self.bus_volume_effective(src.bus)).clamp(0.0, 4.0);
                player.set_volume(initial_vol);
                player.set_speed(src.pitch.max(0.01));

                if src.looping { player.append(source.repeat_infinite()); }
                else { player.append(source); }

                self.active.insert(e, ActiveSound {
                    player, last_volume: initial_vol,
                    last_pitch: src.pitch, last_occlusion: 1.0,
                });
                continue;
            }

            let is_spatial = src.max_distance > src.min_distance;
            let occl_target = if src.occlusion && is_spatial {
                let blocked = !crate::physics::navmesh::segment_clear(
                    world,
                    listener_pos + Vec3::Y * 0.5,
                    pos + Vec3::Y * 0.5,
                );
                if blocked { src.occlusion_volume } else { 1.0 }
            } else {
                1.0
            };

            let bus_vol = self.bus_volume_effective(src.bus);
            let Some(active) = self.active.get_mut(&e) else { continue };

            let occl_smoothing = (dt * 8.0).clamp(0.0, 1.0);
            let occl = active.last_occlusion
                + (occl_target - active.last_occlusion) * occl_smoothing;
            if (occl - active.last_occlusion).abs() > 1e-4 { active.last_occlusion = occl; }

            let dist = (pos - listener_pos).length();
            let atten = attenuation(dist, src.min_distance, src.max_distance);
            let target = (src.volume * atten * bus_vol * active.last_occlusion).clamp(0.0, 4.0);

            let smoothing = (dt * 10.0).clamp(0.0, 1.0);
            let smoothed = active.last_volume + (target - active.last_volume) * smoothing;
            if (smoothed - active.last_volume).abs() > 1e-4 {
                active.player.set_volume(smoothed);
                active.last_volume = smoothed;
            }

            if (src.pitch - active.last_pitch).abs() > 1e-4 {
                active.player.set_speed(src.pitch.max(0.01));
                active.last_pitch = src.pitch;
            }
        }
    }

    pub fn stop_all(&mut self) { for (_, a) in self.active.drain() { a.player.stop(); } }
    pub fn active_count(&self) -> usize { self.active.len() }
}

fn gen_shot(sr: u32) -> Vec<f32> {
    let n = (sr as f32 * 0.08) as usize;
    let mut out = Vec::with_capacity(n);
    let mut seed: u32 = 0x1234_5678;
    for i in 0..n {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((seed >> 8) & 0xFF_FFFF) as f32 / 16_777_215.0 * 2.0 - 1.0;
        let t = i as f32 / n as f32;
        let env = (1.0 - t).powi(3);
        out.push(noise * env * 0.55);
    }
    out
}

fn gen_explosion(sr: u32) -> Vec<f32> {
    let n = (sr as f32 * 0.6) as usize;
    let mut out = Vec::with_capacity(n);
    let mut seed: u32 = 0xCAFE_BABE;
    let mut lp = 0.0f32;
    for i in 0..n {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((seed >> 8) & 0xFF_FFFF) as f32 / 16_777_215.0 * 2.0 - 1.0;
        lp = lp * 0.93 + noise * 0.07;
        let t = i as f32 / n as f32;
        let env = (1.0 - t).powi(2);
        out.push(lp * env * 1.8);
    }
    out
}

fn gen_pickup(sr: u32) -> Vec<f32> {
    let n = (sr as f32 * 0.25) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / n as f32;
        let freq = 600.0 + 900.0 * t;
        let phase = 2.0 * std::f32::consts::PI * freq * (i as f32 / sr as f32);
        let env = (1.0 - t).powi(2);
        out.push(phase.sin() * env * 0.45);
    }
    out
}

fn gen_ding(sr: u32) -> Vec<f32> {
    let n = (sr as f32 * 0.5) as usize;
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let t = i as f32 / n as f32;
        let phase1 = 2.0 * std::f32::consts::PI * 880.0 * (i as f32 / sr as f32);
        let phase2 = 2.0 * std::f32::consts::PI * 1320.0 * (i as f32 / sr as f32);
        let env = (1.0 - t).powi(3);
        out.push((phase1.sin() * 0.4 + phase2.sin() * 0.2) * env);
    }
    out
}