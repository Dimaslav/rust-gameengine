use std::time::Instant;

pub struct Time {
    last: Instant,
    /// Игровой delta последнего кадра. Применяется time_scale и
    /// hit_stop. Используется физикой, анимацией, геймплеем.
    pub delta: f32,
    /// Сглаженный delta (скользящее среднее по 8 кадрам).
    pub delta_smooth: f32,
    /// Общее игровое время (с учётом time_scale и hit-stop).
    pub elapsed: f32,
    pub frame_count: u64,
    fps_avg: f32,
    frame_time_max: f32,
    pub hitches: u64,

    /// Пауза. При `true` `tick()` возвращает delta = 0.
    pub paused: bool,

    // === ИЗМЕНЕНО (Sprint A): time scale + hit-stop ===
    /// Глобальный множитель времени. 1.0 = норма, 0.5 = half-speed,
    /// 0.0 = заморозка. Применяется к `delta`, `elapsed`,
    /// анимациям, физике, AI. Не влияет на FPS-счётчики.
    pub time_scale: f32,
    /// Таймер hit-stop (slow-motion при ударе/килле).
    /// Пока > 0, `time_scale` временно умножается на `hit_stop_scale`.
    hit_stop_timer: f32,
    /// Множитель hit-stop (например 0.15 = замедление в ~7 раз).
    hit_stop_scale: f32,

    samples: [f32; SMOOTH_N],
    sample_idx: usize,
    sample_count: usize,

    warmup_frames: u32,
}

const SMOOTH_N: usize = 8;
const WARMUP_FRAMES: u32 = 3;

const MAX_DELTA: f32 = crate::physics::world::MAX_DT;
const DEFAULT_DT: f32 = 1.0 / 60.0;

impl Time {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            delta: DEFAULT_DT,
            delta_smooth: DEFAULT_DT,
            elapsed: 0.0,
            frame_count: 0,
            fps_avg: 60.0,
            frame_time_max: 0.0,
            hitches: 0,
            paused: false,

            time_scale: 1.0,
            hit_stop_timer: 0.0,
            hit_stop_scale: 1.0,

            samples: [DEFAULT_DT; SMOOTH_N],
            sample_idx: 0,
            sample_count: 0,
            warmup_frames: WARMUP_FRAMES,
        }
    }

    /// Запустить hit-stop: на `duration` секунд реального времени
    /// игровой time_scale умножается на `scale`.
    ///
    /// Классические значения:
    /// - Попадание: (0.04, 0.3) — 4 кадра замедления в 3 раза.
    /// - Kill:      (0.08, 0.15) — 8 кадров замедления в 6 раз.
    /// - Критический удар: (0.12, 0.05) — 12 кадров почти стоп.
    ///
    /// Если hit-stop уже активен — берём более длинный.
    pub fn add_hit_stop(&mut self, duration: f32, scale: f32) {
        if duration <= 0.0 || scale <= 0.0 { return; }
        let scale = scale.clamp(0.0, 1.0);
        if duration > self.hit_stop_timer {
            self.hit_stop_timer = duration;
            self.hit_stop_scale = scale;
        }
    }

    /// Эффективный time scale с учётом hit-stop.
    pub fn effective_time_scale(&self) -> f32 {
        if self.hit_stop_timer > 0.0 {
            self.time_scale * self.hit_stop_scale
        } else {
            self.time_scale
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let raw_delta = (now - self.last).as_secs_f32();
        self.last = now;

        // Считаем hit-stop таймер в реальном времени (не в игровом).
        if self.hit_stop_timer > 0.0 {
            self.hit_stop_timer -= raw_delta;
            if self.hit_stop_timer <= 0.0 {
                self.hit_stop_timer = 0.0;
            }
        }

        let effective_scale = self.effective_time_scale();

        if self.warmup_frames > 0 {
            self.warmup_frames -= 1;
            self.delta = if self.paused { 0.0 } else { DEFAULT_DT * effective_scale };
            if !self.paused { self.elapsed += self.delta; }
            self.frame_count += 1;
            return;
        }

        // Реальный dt для статистики (FPS, hitch-детект) — БЕЗ time_scale.
        let real_delta = raw_delta.min(MAX_DELTA);

        // Игровой dt — с time_scale и hit-stop.
        self.delta = if self.paused {
            0.0
        } else {
            real_delta * effective_scale
        };
        if !self.paused {
            self.elapsed += self.delta;
        }
        self.frame_count += 1;

        // Сглаживание считаем по real_delta, чтобы Stats не врал.
        self.samples[self.sample_idx] = real_delta;
        self.sample_idx = (self.sample_idx + 1) % SMOOTH_N;
        if self.sample_count < SMOOTH_N {
            self.sample_count += 1;
        }

        let sum: f32 = self.samples[..self.sample_count].iter().sum();
        self.delta_smooth = sum / self.sample_count as f32;

        self.frame_time_max = self.samples[..self.sample_count]
            .iter()
            .copied()
            .fold(0.0_f32, f32::max);

        // Hitch-детект тоже по real_delta.
        if !self.paused
            && self.sample_count >= SMOOTH_N
            && real_delta > self.delta_smooth * 2.0
            && real_delta > 0.003
        {
            self.hitches += 1;
        }

        let inst_fps = if self.delta_smooth > 0.0001 {
            1.0 / self.delta_smooth
        } else {
            0.0
        };
        const ALPHA: f32 = 0.05;
        self.fps_avg = self.fps_avg * (1.0 - ALPHA) + inst_fps * ALPHA;
    }

    pub fn fps(&self) -> f32 { self.fps_avg }
    pub fn frame_time_max_ms(&self) -> f32 { self.frame_time_max * 1000.0 }
    pub fn frame_time_avg_ms(&self) -> f32 { self.delta_smooth * 1000.0 }

    /// Сколько осталось hit-stop в реальных секундах (для UI/дебага).
    pub fn hit_stop_remaining(&self) -> f32 { self.hit_stop_timer }
}

impl Default for Time {
    fn default() -> Self { Self::new() }
}