//! Процедурный аудио-движок на базе `rodio` 0.21.
//!
//! API 0.21:
//!   - `OutputStreamBuilder::open_default_stream()` — открыть устройство.
//!   - `Sink::connect_new(&stream.mixer())` — создать sink.
//!   - `SamplesBuffer::new(channels: u16, sample_rate: u32, data)`.
//!
//! ИЗМЕНЕНО (#18): добавлена реализация `rodio::Source` поверх
//! `Arc<Vec<f32>>` — `SharedSamples`. Раньше `play` делал
//! `SamplesBuffer::new(1, sr, samples.clone())`, копируя весь буфер
//! сэмплов (для `explosion` — ~105 КБ на каждый звук). Теперь
//! `play` — дешёвая атомарная операция `Arc::clone`, а сам `Source`
//! отдаёт по одному `f32` из общего буфера.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use rodio::{OutputStreamBuilder, Sink, Source};

/// ИЗМЕНЕНО (#18): обёртка над `Arc<Vec<f32>>`, реализующая
/// `rodio::Source`.
///
/// Раньше `play` создавал `SamplesBuffer::new(1, sr, samples.clone())` —
/// `Vec::clone()` копировал весь буфер сэмплов в новый Vec. Для
/// `shot` это ~14 КБ, для `explosion` — ~105 КБ **на каждый вызов**.
/// При стрельбе ~10 раз/сек это до мегабайта в секунду лишних
/// аллокаций+memcpy.
///
/// `SamplesBuffer::new` принимает `Into<Vec<f32>>` (владение). Замена
/// на `Arc<Vec<f32>>` через `SamplesBuffer::new` невозможна — нужен
/// `Vec` по значению. Поэтому — своя реализация `Source`, которая
/// читает данные из `Arc` без копирования: rodio вызывает `next()`
/// по одному сэмплу, не требуя владения всем буфером.
///
/// `Send + 'static`: `Arc<Vec<f32>>` — `Send + Sync`, `usize` и
/// `u32` — тоже. `Sink::detach()` переносит source на аудио-поток,
/// где это обязательно.
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
    /// rodio 0.21: `current_span_len` вместо старого `current_frame_len`.
    /// `None` — «длина следующего блока неизвестна, читай по одному
    /// сэмплу до `None`». Для коротких процедурных эффектов это
    /// нормально: rodio сам буферизует.
    fn current_span_len(&self) -> Option<usize> {
        None
    }

    fn channels(&self) -> u16 {
        1
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn total_duration(&self) -> Option<Duration> {
        None
    }
}

pub struct AudioSystem {
    _stream: rodio::OutputStream,

    /// ИЗМЕНЕНО (#18): `Arc<Vec<f32>>` вместо `Vec<f32>`.
    ///
    /// `Arc::clone` — одна атомарная операция (инкремент счётчика),
    /// `Vec::clone` — аллокация + memcpy всего буфера. Для
    /// коротких звуков (shot, pickup, ding) разница заметна только
    /// на больших частотах, но убирать её бесплатно — правильный
    /// trade-off.
    cache: HashMap<&'static str, Arc<Vec<f32>>>,
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
        // ИЗМЕНЕНО (#18): каждая запись обёрнута в `Arc`.
        cache.insert("shot",      Arc::new(gen_shot(sample_rate)));
        cache.insert("explosion", Arc::new(gen_explosion(sample_rate)));
        cache.insert("pickup",    Arc::new(gen_pickup(sample_rate)));
        cache.insert("ding",      Arc::new(gen_ding(sample_rate)));

        log::info!(
            "audio: initialized @ {} Hz, {} procedural sounds",
            sample_rate,
            cache.len()
        );

        Some(Self { _stream: stream, cache, sample_rate })
    }

    pub fn play(&self, name: &'static str) {
        let Some(samples) = self.cache.get(name) else { return; };

        // ИЗМЕНЕНО (#18): `Arc::clone` вместо `samples.clone()`.
        // Сам `Source` (`SharedSamples`) читает данные из общего
        // `Arc<Vec<f32>>` без копирования.
        let source = SharedSamples {
            data: Arc::clone(samples),
            pos: 0,
            sample_rate: self.sample_rate,
        };

        let sink = Sink::connect_new(self._stream.mixer());
        sink.append(source);
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