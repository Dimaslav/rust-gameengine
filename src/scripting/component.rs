//! ScriptComponent — скрипт, прикреплённый к entity.
//!
//! Хранится как обычный ECS-компонент и сериализуется в сцену.
//! Сам скрипт живёт в `assets/scripts/`, компонент — только путь.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptComponent {
    /// Путь относительно `assets/scripts/` (например, "enemy_aggro").
    /// Расширение `.rhai` добавляется автоматически.
    pub script: String,

    /// Выключенный скрипт не тикает, но остаётся на entity.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Флаг «on_start уже вызван». Runtime-only.
    #[serde(skip, default)]
    pub started: bool,
}

fn default_true() -> bool { true }

impl ScriptComponent {
    pub fn new(script: impl Into<String>) -> Self {
        Self { script: script.into(), enabled: true, started: false }
    }
}