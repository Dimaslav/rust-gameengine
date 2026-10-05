//! Процедурный аудио-движок на базе `rodio` 0.21.
//!
//! API 0.21:
//!   - `OutputStreamBuilder::open_default_stream()` — открыть устройство.
//!   - `Sink::connect_new(&stream.mixer())` — создать sink.
//!   - `SamplesBuffer::new(channels: u16, sample_rate: u32, data)`.

use std::collections::HashMap;

use rodio::buffer::SamplesBuffer;
use rodio::{OutputStreamBuilder, Sink};

pub struct AudioSystem {
    _stream: rodio::OutputStream,
    cache: HashMap<&'static str, Vec<f32>>,
    sample_rate: u32,
}

impl AudioSystem {
    pub fn new() -> Option<Self> {
        let stream = match OutputStreamBuilder::open_default_stream() {
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
        cache.insert("ding",      gen_ding(sample_rate));

        log::info!(
            "audio: initialized @ {} Hz, {} procedural sounds",
            sample_rate,
            cache.len()
        );

        Some(Self { _stream: stream, cache, sample_rate })
    }

    pub fn play(&self, name: &'static str) {
        let Some(samples) = self.cache.get(name) else { return; };

        let buf = SamplesBuffer::new(1u16, self.sample_rate, samples.clone());

        // rodio 0.21: Sink::connect_new(&mixer).
        let sink = Sink::connect_new(self._stream.mixer());
        sink.append(buf);
        sink.detach();
    }
}

// ============================================================
// Процедурные генераторы (без изменений)
// ============================================================

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