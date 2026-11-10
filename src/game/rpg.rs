//! RPG-компоненты, которые используются сценой "The Fallen Citadel"
//! и сериализацией (`scene/serialize.rs`).
//!
//! Ранее в этом файле была полноценная RPG-демка (`RpgState`,
//! `try_interact`, `on_kill`, `tick`, `spawn_scene`, …), но её
//! заменил `FortressDemo`. Здесь остались только data-компоненты,
//! на которые ссылаются другие модули.

use serde::{Deserialize, Serialize};

// ============================================================
// Компоненты
// ============================================================

/// Количество золота в подбираемом предмете.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GoldValue(pub u32);

/// Идентификатор ключа, который подбирается.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct KeyItem(pub u32);

/// Сундук: сколько золота внутри и открыт ли он.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Chest {
    pub gold: u32,
    pub opened: bool,
}

/// Дверь: какой ключ нужен и открыта ли.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Door {
    pub needs_key: u32,
    pub open: bool,
}

/// NPC с репликами и опциональным квестом.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Npc {
    pub name: String,
    pub lines: Vec<String>,
    pub quest_id: Option<u32>,
    pub spoken_to: u32,
}

/// Маркер: убийство этой сущности продвигает квест `quest_id`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct QuestTarget {
    pub quest_id: u32,
}