//! ScriptEvent — событие, которое раздаётся всем скриптам через `on_event`.
//!
//! Это ECS-событие: игра и скрипты шлют его через `world.send(...)`,
//! `ScriptSystem` читает `read_events::<ScriptEvent>()` в начале
//! своего тика и вызывает `on_event(host)` у каждой scripted entity.
//!
//! # Доставка
//!
//! Событие, отправленное в кадре N, доставляется скриптам в кадре N+1
//! (через `previous`-буфер `Events`). Это **exactly-once** — не
//! «сколько отправили, столько получили», а «раз за кадр после отправки».
//! Такая задержка в один кадр — стандарт для событийных шин и позволяет
//! избежать циклических зависимостей внутри одного кадра.

use std::collections::HashMap;

use crate::ecs::Entity;

#[derive(Debug, Clone)]
pub struct ScriptEvent {
    /// Имя события. Соглашение — snake_case: "all_enemies_dead",
    /// "door_opened", "player_died".
    pub name: String,

    /// Entity-источник (опционально). Полезно, чтобы скрипт-получатель
    /// мог отреагировать «на того, кто прислал».
    pub source: Option<Entity>,

    /// Произвольные числовые параметры. Скрипт читает их через
    /// `event_get("key")`.
    pub payload: HashMap<String, f64>,
}

impl ScriptEvent {
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            source: None,
            payload: HashMap::new(),
        }
    }

    pub fn with_source(mut self, e: Entity) -> Self {
        self.source = Some(e);
        self
    }

    pub fn with_payload(mut self, key: impl Into<String>, value: f64) -> Self {
        self.payload.insert(key.into(), value);
        self
    }
}