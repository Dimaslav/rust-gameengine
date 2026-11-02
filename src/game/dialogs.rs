//! Диалоговый граф: структуры данных, загрузка из RON, состояние сессии.
//!
//! # Формат
//!
//! Диалоги хранятся в `assets/dialogs/*.ron`. Каждый — `DialogGraph`
//! с узлами (`DialogNode`). Узел содержит реплику NPC и список опций.
//!
//! ```ron
//! (
//!     id: "elder",
//!     speaker_name: "Elder",
//!     start: "intro",
//!     nodes: {
//!         "intro": (
//!             speaker: "Elder",
//!             text: "Приветствую, путник.",
//!             options: [
//!                 (text: "Что мне сделать?", next: Some("quest_offer"), effect: []),
//!                 (text: "Просто прохожу мимо.", next: None, effect: []),
//!             ],
//!         ),
//!         "quest_offer": (
//!             speaker: "Elder",
//!             text: "Убей 5 волков.",
//!             options: [
//!                 (text: "Согласен.", next: Some("thanks"),
//!                     effect: [StartQuest("act1_guards")]),
//!             ],
//!         ),
//!     },
//! )
//! ```

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::ecs::Entity;

// ============================================================
// Компонент: NPC, у которого есть диалог
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogNpc {
    pub dialog_id: String,
}

// ============================================================
// DialogGraph
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogGraph {
    pub id: String,
    pub speaker_name: String,
    pub start: String,
    #[serde(default)]
    pub nodes: HashMap<String, DialogNode>,
}

impl DialogGraph {
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        let text = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("read dialog {}", path.as_ref().display()))?;
        let graph: Self = ron::from_str(&text)
            .with_context(|| format!("parse dialog {}", path.as_ref().display()))?;
        Ok(graph)
    }

    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<HashMap<String, DialogGraph>> {
        let dir = dir.as_ref();
        let mut out = HashMap::new();
        if !dir.exists() {
            log::warn!("dialogs: dir '{}' не существует, диалоги отключены", dir.display());
            return Ok(out);
        }
        for entry in std::fs::read_dir(dir)?.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) == Some("ron") {
                match Self::load_from_file(&path) {
                    Ok(g) => {
                        log::info!("dialog loaded: '{}' ({} nodes)", g.id, g.nodes.len());
                        out.insert(g.id.clone(), g);
                    }
                    Err(e) => log::warn!("dialog load failed {}: {:#}", path.display(), e),
                }
            }
        }
        Ok(out)
    }

    pub fn node(&self, id: &str) -> Option<&DialogNode> {
        self.nodes.get(id)
    }
}

// ============================================================
// DialogNode
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogNode {
    pub speaker: String,
    pub text: String,
    #[serde(default)]
    pub options: Vec<DialogOption>,
    #[serde(default)]
    pub on_enter: Vec<DialogEffect>,
}

// ============================================================
// DialogOption
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogOption {
    pub text: String,
    /// `None` → конец диалога.
    #[serde(default)]
    pub next: Option<String>,
    /// Условие показа (сейчас — reserved).
    #[serde(default)]
    pub condition: Option<String>,
    #[serde(default)]
    pub effect: Vec<DialogEffect>,
}

// ============================================================
// DialogEffect
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DialogEffect {
    StartQuest(String),
    GiveItem { id: String, count: u32 },
    GiveGold(u32),
    GiveXp(u32),
}

// ============================================================
// DialogSession — runtime
// ============================================================

pub struct DialogSession {
    pub graph: DialogGraph,
    pub current_node: String,
    pub npc_entity: Entity,
}

impl DialogSession {
    pub fn new(graph: DialogGraph, npc_entity: Entity) -> Option<Self> {
        let start = graph.start.clone();
        if !graph.nodes.contains_key(&start) {
            log::error!("dialog '{}': start node '{}' not found", graph.id, start);
            return None;
        }
        Some(Self {
            graph,
            current_node: start,
            npc_entity,
        })
    }

    pub fn node(&self) -> Option<&DialogNode> {
        self.graph.node(&self.current_node)
    }

    /// Выбрать опцию. Возвращает `(effect, ended)`. `ended == true` → закрыть диалог.
    pub fn choose(&mut self, idx: usize) -> (Vec<DialogEffect>, bool) {
        let Some(node) = self.node().cloned() else {
            return (Vec::new(), true);
        };
        let Some(opt) = node.options.get(idx) else {
            return (Vec::new(), false);
        };

        let mut effects = opt.effect.clone();
        let ended = match &opt.next {
            Some(next_id) if self.graph.nodes.contains_key(next_id) => {
                self.current_node = next_id.clone();
                if let Some(next_node) = self.node() {
                    effects.extend(next_node.on_enter.clone());
                }
                false
            }
            _ => true,
        };
        (effects, ended)
    }
}

// ============================================================
// Поиск NPC перед игроком
// ============================================================

/// Простой поиск ближайшего NPC в направлении взгляда.
/// Не проверяет стены (для демо — приемлемо).
pub fn find_npc_in_front(
    world: &crate::ecs::World,
    cam_pos: glam::Vec3,
    cam_dir: glam::Vec3,
    max_dist: f32,
    max_angle_cos: f32,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (e, _) in world.query::<DialogNpc>() {
        let Some(t) = world.get::<crate::game::components::Transform>(e) else { continue };
        let to = t.position - cam_pos;
        let dist = to.length();
        if dist > max_dist || dist < 1e-4 { continue; }
        let dot = to.normalize_or_zero().dot(cam_dir);
        if dot < max_angle_cos { continue; }
        if best.is_none_or(|(_, d)| dist < d) {
            best = Some((e, dist));
        }
    }
    best.map(|(e, _)| e)
}