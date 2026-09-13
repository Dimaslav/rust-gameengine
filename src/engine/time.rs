use std::time::Instant;

pub struct Time {
    last: Instant,
    pub delta: f32,
    pub elapsed: f32,
    pub frame_count: u64,
    /// Скользящее среднее FPS по 30 кадрам — стабильнее в заголовке.
    fps_avg: f32,
}

impl Time {
    pub fn new() -> Self {
        Self {
            last: Instant::now(),
            delta: 0.0,
            elapsed: 0.0,
            frame_count: 0,
            fps_avg: 0.0,
        }
    }

    pub fn tick(&mut self) {
        let now = Instant::now();
        self.delta = (now - self.last).as_secs_f32();
        self.elapsed += self.delta;
        self.last = now;
        self.frame_count += 1;

        let inst_fps = if self.delta > 0.0001 { 1.0 / self.delta } else { 0.0 };
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
}

impl Default for Time {
    fn default() -> Self {
        Self::new()
    }
}