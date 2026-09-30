use std::time::Instant;

pub struct Time {
    last: Instant,
    /// Сырой delta последнего кадра — для физики, анимаций.
    pub delta: f32,
    /// Сглаженный delta (скользящее среднее по 8 кадрам) — для камеры
    /// и движения. Устраняет дёрганье при неравномерном приходе кадров.
    pub delta_smooth: f32,
    pub elapsed: f32,
    pub frame_count: u64,
    fps_avg: f32,
    frame_time_max: f32,
    /// Счётчик «плохих» кадров (в 2× длиннее сглаженного).
    pub hitches: u64,

    samples: [f32; SMOOTH_N],
    sample_idx: usize,
    sample_count: usize,
}

const SMOOTH_N: usize = 8;

impl Time {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            delta: 0.0,
            delta_smooth: 0.0,
            elapsed: 0.0,
            frame_count: 0,
            fps_avg: 0.0,
            frame_time_max: 0.0,
            hitches: 0,
            samples: [0.0; SMOOTH_N],
            sample_idx: 0,
            sample_count: 0,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.delta = (now - self.last).as_secs_f32();
        self.last = now;
        self.elapsed += self.delta;
        self.frame_count += 1;

        // Кольцевой буфер
        self.samples[self.sample_idx] = self.delta;
        self.sample_idx = (self.sample_idx + 1) % SMOOTH_N;
        if self.sample_count < SMOOTH_N {
            self.sample_count += 1;
        }

        // Скользящее среднее
        let sum: f32 = self.samples[..self.sample_count].iter().sum();
        self.delta_smooth = sum / self.sample_count as f32;

        // Пиковое время кадра за окно
        self.frame_time_max = self.samples[..self.sample_count]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);

        // Hitch: кадр в 2+ раза длиннее среднего.
        if self.sample_count >= SMOOTH_N
            && self.delta > self.delta_smooth * 2.0
            && self.delta > 0.003
        {
            self.hitches += 1;
        }

        let inst_fps = if self.delta_smooth > 0.0001 {
            1.0 / self.delta_smooth
        } else {
            0.0
        };
        const ALPHA: f32 = 0.05;
        if self.fps_avg == 0.0 {
            self.fps_avg = inst_fps;
        } else {
            self.fps_avg = self.fps_avg * (1.0 - ALPHA) + inst_fps * ALPHA;
        }
    }

    pub fn fps(&self) -> f32 {
        self.fps_avg
    }

    /// Максимальное время кадра за окно (мс).
    pub fn frame_time_max_ms(&self) -> f32 {
        self.frame_time_max * 1000.0
    }

    /// Среднее время кадра за окно (мс).
    pub fn frame_time_avg_ms(&self) -> f32 {
        self.delta_smooth * 1000.0
    }
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}