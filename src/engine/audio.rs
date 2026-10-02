//! Процедурный аудио-движок на базе `rodio` 0.22.
//!
//! Все звуки генерируются в память при старте — никаких файлов на диске.
//! Если аудиоустройство недоступно (headless, ошибка драйвера),
//! `AudioSystem::new()` возвращает `None`, и игра работает без звука.

use std::collections::HashMap;
use std::num::NonZero;

use rodio::buffer::SamplesBuffer;
use rodio::stream::{DeviceSinkBuilder, MixerDeviceSink};

pub struct AudioSystem {
    /// Держим `stream` живым — при дропе останавливается cpal-поток.
    /// Через `stream.mixer().add()` подкидываем новые источники.
    stream: MixerDeviceSink,
    /// Кэш PCM-сэмплов (mono, f32, -1..=1).
    cache: HashMap<&'static str, Vec<f32>>,
    sample_rate: u32,
}

impl AudioSystem {
    /// Попытаться создать аудио. `None` — если устройство не открылось.
    pub fn new() -> Option<Self> {
        let stream = match DeviceSinkBuilder::open_default_sink() {
            Ok(s) => s,
            Err(e) => {
                log::warn!("audio: cannot open output stream: {}", e);
                return None;
            }
        };

        let sample_rate: u32 = 44_100;
        let mut cache = HashMap::new();

        cache.insert("shot",      gen_shot(sample_rate));
        cache.insert("explosion", gen_explosion(sample_rate));
        cache.insert("pickup",    gen_pickup(sample_rate));

        log::info!(
            "audio: initialized @ {} Hz, {} procedural sounds",
            sample_rate,
            cache.len()
        );

        Some(Self {
            stream,
            cache,
            sample_rate,
        })
    }

    /// Проиграть звук по имени. Накладывается асинхронно.
    /// Если имени нет в кэше — no-op.
    pub fn play(&self, name: &'static str) {
        let Some(samples) = self.cache.get(name) else {
            return;
        };

        // rodio 0.22: SamplesBuffer::new требует NonZero<u16> (channels) и
        // NonZero<u32> (sample_rate). Создаём через NonZero::new().unwrap() —
        // оба числа заведомо > 0.
        let channels = NonZero::<u16>::new(1).unwrap();
        let sample_rate = NonZero::<u32>::new(self.sample_rate).unwrap();

        let buf = SamplesBuffer::new(channels, sample_rate, samples.clone());

        // rodio 0.22: mixer() возвращает &Mixer; add() добавляет источник
        // в очередь воспроизведения.
        self.stream.mixer().add(buf);
    }
}

// ============================================================
// Процедурные генераторы
// ============================================================

/// Быстрый "щелчок" — шум с резким спадом. ~80 мс.
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

/// Низкочастотный гул с шумом. ~600 мс.
fn gen_explosion(sr: u32) -> Vec<f32> {
    let n = (sr as f32 * 0.6) as usize;
    let mut out = Vec::with_capacity(n);
    let mut seed: u32 = 0xCAFE_BABE;
    let mut lp = 0.0f32;

    for i in 0..n {
        seed = seed.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let noise = ((seed >> 8) & 0xFF_FFFF) as f32 / 16_777_215.0 * 2.0 - 1.0;
        // Однополюсный ФНЧ — оставляет только бас.
        lp = lp * 0.93 + noise * 0.07;
        let t = i as f32 / n as f32;
        let env = (1.0 - t).powi(2);
        out.push(lp * env * 1.8);
    }
    out
}

/// Восходящий синус. ~250 мс.
fn gen_pickup(sr: u32) -> Vec<f32> {
    let n = (sr as f32 * 0.25) as usize;
    let mut out = Vec::with_capacity(n);

    for i in 0..n {
        let t = i as f32 / n as f32;
        let freq = 600.0 + 900.0 * t;      // 600 → 1500 Гц
        let phase = 2.0 * std::f32::consts::PI * freq * (i as f32 / sr as f32);
        let env = (1.0 - t).powi(2);
        out.push(phase.sin() * env * 0.45);
    }
    out
}