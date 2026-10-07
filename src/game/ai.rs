//! AI-компоненты: агент, состояние FSM, зрение, патруль, слух.
//!
//! Логика самого FSM — в `engine::ai_system`. Здесь только данные.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use crate::ecs::Entity;

/// Состояние агента. FSM переходы:
///
/// ```text
/// Idle ──(see_target)──> Chase ──(in_range)──> Attack
///   ▲                        │                    │
///   │                        │ lost_target         │ lost_target
///   │                        ▼                    ▼
///   │                    Investigate  ◄───────────┘
///   │                        │
///   │                   (reached | timeout)
///   │                        ▼
///   └────(has_patrol)───── Patrol
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AiState {
    /// Стоит, ждёт.
    Idle,
    /// Ходит по точкам маршрута.
    Patrol,
    /// ИЗМЕНЕНО (sound perception): расследует шум или последнюю
    /// известную позицию цели.
    Investigate,
    /// Видит игрока, бежит за ним.
    Chase,
    /// В зоне атаки, бьёт.
    Attack,
    /// Мёртв (Health.current <= 0). Система его игнорирует.
    Dead,
}

impl Default for AiState {
    fn default() -> Self { Self::Idle }
}

/// Агент с FSM.
#[derive(Debug, Clone)]
pub struct AiAgent {
    pub state: AiState,

    /// Скорость патруля / погони.
    pub speed: f32,
    /// Скорость поворота (rad/s).
    pub turn_speed: f32,

    /// Дальность зрения.
    pub vision_range: f32,
    /// Косинус половины угла обзора. cos(60°) = 0.5.
    pub vision_angle_cos: f32,

    /// ИЗМЕНЕНО (sound perception): радиус слышимости.
    ///
    /// Если источник шума (`NoiseEvent`) в этом радиусе — агент
    /// переходит в `Investigate` (или `Chase`, если видит цель).
    /// 0.0 = глухой.
    pub hearing_range: f32,

    /// Дистанция атаки.
    pub attack_range: f32,
    /// Урон за один удар.
    pub damage: f32,
    /// Кулдаун атаки.
    pub attack_cooldown: f32,

    /// Сколько секунд без видимости до потери цели.
    pub lose_target_time: f32,

    // === Runtime (не сериализуется) ===
    /// Текущий путь (пересчитывается через A*).
    pub path: Vec<Vec3>,
    /// Индекс текущей цели в `path`.
    pub path_index: usize,
    /// Таймер до следующего перерасчёта пути.
    pub repath_timer: f32,
    /// Кулдаун атаки (тикает).
    pub attack_timer: f32,
    /// Последняя известная позиция цели (или точки шума).
    pub last_seen_pos: Option<Vec3>,
    /// Сколько секунд цель не видна.
    pub time_since_seen: f32,
    /// Куда агент смотрит (yaw, radians).
    pub yaw: f32,
    /// Время в текущем состоянии (для timeouts).
    pub state_timer: f32,

    /// ИЗМЕНЕНО: агент — источник шума при атаке?
    /// Зарезервировано на будущее (групповое поведение).
    pub emits_noise: bool,
}

impl Default for AiAgent {
    fn default() -> Self {
        Self {
            state: AiState::Idle,
            speed: 3.0,
            turn_speed: 8.0,
            vision_range: 20.0,
            vision_angle_cos: 0.5, // 60° half-angle
            hearing_range: 25.0,
            attack_range: 1.5,
            damage: 10.0,
            attack_cooldown: 1.0,
            lose_target_time: 3.0,

            path: Vec::new(),
            path_index: 0,
            repath_timer: 0.0,
            attack_timer: 0.0,
            last_seen_pos: None,
            time_since_seen: 999.0,
            yaw: 0.0,
            state_timer: 0.0,
            emits_noise: false,
        }
    }
}

impl AiAgent {
    pub fn new() -> Self { Self::default() }

    pub fn with_speed(mut self, s: f32) -> Self { self.speed = s; self }
    pub fn with_vision(mut self, range: f32, half_angle_rad: f32) -> Self {
        self.vision_range = range;
        self.vision_angle_cos = half_angle_rad.cos();
        self
    }
    /// ИЗМЕНЕНО (sound perception): настроить радиус слышимости.
    pub fn with_hearing(mut self, range: f32) -> Self {
        self.hearing_range = range;
        self
    }
    pub fn with_attack(mut self, range: f32, damage: f32, cooldown: f32) -> Self {
        self.attack_range = range;
        self.damage = damage;
        self.attack_cooldown = cooldown;
        self
    }

    pub fn is_alive(&self) -> bool { self.state != AiState::Dead }
}

/// Цикл патруля.
#[derive(Debug, Clone)]
pub struct PatrolPath {
    pub points: Vec<Vec3>,
    /// Индекс текущей цели.
    pub current: usize,
    /// Пауза в каждой точке.
    pub wait_time: f32,
    /// Текущая пауза.
    pub wait_timer: f32,
    /// Идёт ли пауза сейчас.
    pub waiting: bool,
}

impl PatrolPath {
    pub fn new(points: Vec<Vec3>) -> Self {
        Self {
            points,
            current: 0,
            wait_time: 0.5,
            wait_timer: 0.0,
            waiting: false,
        }
    }

    pub fn current_target(&self) -> Option<Vec3> {
        self.points.get(self.current).copied()
    }

    /// Перейти к следующей точке после паузы.
    pub fn advance(&mut self) {
        if self.points.is_empty() { return; }
        self.current = (self.current + 1) % self.points.len();
        self.waiting = false;
        self.wait_timer = 0.0;
    }

    pub fn tick_wait(&mut self, dt: f32) {
        if !self.waiting { return; }
        self.wait_timer -= dt;
        if self.wait_timer <= 0.0 {
            self.advance();
        }
    }

    pub fn start_wait(&mut self) {
        self.waiting = true;
        self.wait_timer = self.wait_time;
    }
}

impl Default for PatrolPath {
    fn default() -> Self {
        Self::new(Vec::new())
    }
}

/// Маркер цели (обычно на игроке или на главной entity, которую
/// AI преследует). Система ищет entity с этим компонентом и берёт
/// первую живую.
#[derive(Debug, Clone, Copy, Default)]
pub struct AiTarget;

/// Маркер "это агент, которого можно убить". Используется, чтобы
/// отличать врагов от декоративных NPC.
#[derive(Debug, Clone, Copy, Default)]
pub struct Enemy;

/// Тег для отладки: показывает путь агента линиями.
#[derive(Debug, Clone, Copy, Default)]
pub struct DebugPath;

/// ИЗМЕНЕНО (sound perception): источник шума в мире.
///
/// Отправляется через `world.send(NoiseEvent { ... })`. Читается
/// `AiSystem::update` (по `read_events_current`) в тот же кадр.
///
/// `radius` — реальный радиус слышимости этого шума. AI воспринимает
/// шум, если его `hearing_range >= radius` **и** расстояние от агента
/// до `position` меньше `min(hearing_range, radius)`.
#[derive(Debug, Clone, Copy)]
pub struct NoiseEvent {
    pub position: Vec3,
    pub radius: f32,
    pub kind: NoiseKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoiseKind {
    /// Выстрел игрока. Громкий, привлекает всех в радиусе.
    Gunshot,
    /// Шаги. Тише, короткий радиус.
    Footstep,
    /// Удар / взрыв. Средний радиус.
    Impact,
    /// Голос NPC / монстра.
    Voice,
}

impl NoiseKind {
    /// Дефолтный радиус для типа шума.
    pub fn default_radius(self) -> f32 {
        match self {
            NoiseKind::Gunshot => 35.0,
            NoiseKind::Footstep => 8.0,
            NoiseKind::Impact => 15.0,
            NoiseKind::Voice => 12.0,
        }
    }
}