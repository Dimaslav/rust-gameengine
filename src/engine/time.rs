use std::time::Instant;

pub struct Time {
    last: Instant,
    /// Сырой delta последнего кадра — для физики, анимаций, геймплея.
    ///
    /// ИЗМЕНЕНО (#11): именно это значение передаётся в
    /// `physics.step`, `game.update`, `update_player`,
    /// `update_particles`, `update_projectiles`.
    ///
    /// Раньше движок везде использовал `delta_smooth`, и при рывке
    /// (загрузка ресурса, GC, breakpoint) физика получала среднее
    /// за 8 кадров вместо реального dt — движение «плыло».
    ///
    /// Клампится по `MAX_DELTA`. Значение синхронизировано с
    /// `physics::world::MAX_DT` через прямой re-export (см. ниже).
    pub delta: f32,
    /// Сглаженный delta (скользящее среднее по 8 кадрам).
    ///
    /// ИЗМЕНЕНО (#11): используется **только** для UI (панель Stats,
    /// "Frame: X ms", FPS-индикатор). В интеграцию физики/геймплея
    /// не идёт — см. `delta` выше.
    pub delta_smooth: f32,
    pub elapsed: f32,
    pub frame_count: u64,
    fps_avg: f32,
    frame_time_max: f32,
    pub hitches: u64,

    samples: [f32; SMOOTH_N],
    sample_idx: usize,
    sample_count: usize,

    /// Сколько первых кадров игнорировать: их delta недостоверна
    /// (инициализация, компиляция шейдеров, загрузка сцены).
    warmup_frames: u32,
}

const SMOOTH_N: usize = 8;
const WARMUP_FRAMES: u32 = 3;

/// ИЗМЕНЕНО (#20): единственная константа вместо двух.
///
/// Раньше здесь было `const MAX_DELTA: f32 = 0.05;` и параллельно
/// `physics::world::MAX_DT = 0.05`. Синхронизация поддерживалась
/// комментарием — если кто-то менял одну, вторая оставалась старой,
/// и `time` мог выдать dt, который физика клампнёт к другому
/// значению. Теперь — прямой re-export.
///
/// `time` клампит `delta` здесь, чтобы движок никогда не получал
/// `dt > MAX_DT`. Это важно для:
///   * `physics.step` (внутри тоже клампит, но мы не хотим два
///     разных значения в одном кадре),
///   * интеграции игрока (`update_player`),
///   * снарядов (`update_projectiles`),
///   * частиц (`update_particles`).
///
/// Всё, что медленнее 20 FPS, «замедляет» игровое время. Лучше
/// пауза, чем взрыв физики.
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
            // Инициализируем правдоподобным значением, а не 0 —
            // иначе первый `tick()` даёт всплеск FPS.
            fps_avg: 60.0,
            frame_time_max: 0.0,
            hitches: 0,
            samples: [DEFAULT_DT; SMOOTH_N],
            sample_idx: 0,
            sample_count: 0,
            warmup_frames: WARMUP_FRAMES,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        let raw_delta = (now - self.last).as_secs_f32();
        self.last = now;

        // Пропускаем warmup-кадры — не портим статистику мусором.
        if self.warmup_frames > 0 {
            self.warmup_frames -= 1;
            self.delta = DEFAULT_DT;
            self.elapsed += self.delta;
            self.frame_count += 1;
            return;
        }

        // Клампим «зависшие» кадры — иначе следующий сглаженный delta
        // будет искажён, и физика получит dt=5 секунд.
        self.delta = raw_delta.min(MAX_DELTA);
        self.elapsed += self.delta;
        self.frame_count += 1;

        self.samples[self.sample_idx] = self.delta;
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
        self.fps_avg = self.fps_avg * (1.0 - ALPHA) + inst_fps * ALPHA;
    }

    pub fn fps(&self) -> f32 { self.fps_avg }
    pub fn frame_time_max_ms(&self) -> f32 { self.frame_time_max * 1000.0 }
    pub fn frame_time_avg_ms(&self) -> f32 { self.delta_smooth * 1000.0 }
}

impl Default for Time {
    fn default() -> Self { Self::new() }
}