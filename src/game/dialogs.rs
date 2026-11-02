//! Диалоговый граф: структуры данных, загрузка из RON, состояние сессии.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::ecs::Entity;
use crate::game::items::Inventory;
use crate::game::quests::QuestManager;

// ============================================================
// DialogNpc
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

    /// Загружает все `.ron` из папки с учётом языка.
    ///
    /// Приоритет: `<name>.<lang>.ron` > `<name>.ron`.
    /// Если английского файла нет — используется базовый (русский).
    pub fn load_from_dir(
        dir: impl AsRef<Path>,
        lang: &str,
    ) -> Result<HashMap<String, DialogGraph>> {
        let dir = dir.as_ref();
        let mut out = HashMap::new();
        if !dir.exists() {
            log::warn!("dialogs: dir '{}' не существует, диалоги отключены", dir.display());
            return Ok(out);
        }

        let mut files: Vec<PathBuf> = std::fs::read_dir(dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
            .collect();
        files.sort();

        // Сначала грузим локализованные версии (приоритет)
        let suffix = format!(".{}.ron", lang);
        let mut loaded_ids: HashSet<String> = HashSet::new();

        for path in &files {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if filename.ends_with(&suffix) {
                match Self::load_from_file(path) {
                    Ok(g) => {
                        log::info!("dialog loaded: '{}' [{}] ({} nodes)", g.id, lang, g.nodes.len());
                        loaded_ids.insert(g.id.clone());
                        out.insert(g.id.clone(), g);
                    }
                    Err(e) => log::warn!("dialog load failed {}: {:#}", path.display(), e),
                }
            }
        }

        // Потом грузим базовые (fallback)
        for path in &files {
            let filename = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !filename.ends_with(&suffix) {
                match Self::load_from_file(path) {
                    Ok(g) => {
                        if !loaded_ids.contains(&g.id) {
                            log::info!("dialog loaded: '{}' [fallback] ({} nodes)", g.id, g.nodes.len());
                            out.insert(g.id.clone(), g);
                        }
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
// DialogNode / DialogOption
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DialogOption {
    pub text: String,
    #[serde(default)]
    pub next: Option<String>,
    /// Условие показа. Синтаксис:
    /// - `quest_active:<id>` — квест в процессе
    /// - `quest_done:<id>` — квест завершён
    /// - `quest_not_started:<id>` — ещё не начат
    /// - `quest_objectives_done:<id>` — цели выполнены, ждёт сдачи
    /// - `has_item:<id>` или `has_item:<id>:<count>` — предмет в инвентаре
    /// - `has_key_red` — есть красный ключ
    /// - `gold >= N`, `level >= N` — числовые проверки
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
    CompleteQuest(String),
    GiveItem { id: String, count: u32 },
    GiveGold(u32),
    GiveXp(u32),
}

// ============================================================
// DialogContext — состояние для проверки условий
// ============================================================

pub struct DialogContext<'a> {
    pub quest_manager: &'a QuestManager,
    pub inventory: &'a Inventory,
    pub gold: u32,
    pub player_level: u32,
    pub has_key_red: bool,
}

impl<'a> DialogContext<'a> {
    pub fn eval_condition(&self, cond: &str) -> bool {
        let cond = cond.trim();
        if cond.is_empty() {
            return true;
        }

        if let Some((kind, arg)) = cond.split_once(':') {
            let arg = arg.trim();
            return match kind.trim() {
                "quest_active" => self.quest_manager.active.contains_key(arg),
                "quest_done" => self.quest_manager.completed.contains(arg),
                "quest_not_started" => {
                    !self.quest_manager.active.contains_key(arg)
                        && !self.quest_manager.completed.contains(arg)
                }
                "quest_objectives_done" => self.quest_manager.is_objectives_done(arg),
                "has_item" => {
                    let (id, count) = arg
                        .split_once(':')
                        .map(|(a, b)| (a.trim(), b.trim().parse::<u32>().unwrap_or(1)))
                        .unwrap_or((arg, 1));
                    self.inventory.count_of(id) >= count
                }
                _ => {
                    log::warn!("unknown condition: {}", cond);
                    false
                }
            };
        }

        if let Some((field, val)) = cond.split_once(">=") {
            let v: i64 = val.trim().parse().unwrap_or(0);
            return match field.trim() {
                "gold" => self.gold as i64 >= v,
                "level" => self.player_level as i64 >= v,
                _ => false,
            };
        }

        if let Some((field, val)) = cond.split_once("<=") {
            let v: i64 = val.trim().parse().unwrap_or(0);
            return match field.trim() {
                "gold" => self.gold as i64 <= v,
                "level" => self.player_level as i64 <= v,
                _ => false,
            };
        }

        match cond {
            "has_key_red" => self.has_key_red,
            _ => {
                log::warn!("unknown condition: {}", cond);
                false
            }
        }
    }
}

// ============================================================
// DialogSession
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

    /// Список доступных опций: `(original_idx, &DialogOption)`.
    pub fn available_options<'b>(&'b self, ctx: &DialogContext) -> Vec<(usize, &'b DialogOption)> {
        let Some(node) = self.node() else {
            return vec![];
        };
        node.options
            .iter()
            .enumerate()
            .filter(|(_, opt)| {
                opt.condition
                    .as_deref()
                    .map_or(true, |c| ctx.eval_condition(c))
            })
            .collect()
    }

    /// Выбор опции по **оригинальному** индексу (не по индексу в фильтрованном списке).
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

pub fn find_npc_in_front(
    world: &crate::ecs::World,
    cam_pos: glam::Vec3,
    cam_dir: glam::Vec3,
    max_dist: f32,
    max_angle_cos: f32,
) -> Option<Entity> {
    let mut best: Option<(Entity, f32)> = None;
    for (e, _) in world.query::<DialogNpc>() {
        let Some(t) = world.get::<crate::game::components::Transform>(e) else {
            continue;
        };
        let to = t.position - cam_pos;
        let dist = to.length();
        if dist > max_dist || dist < 1e-4 {
            continue;
        }
        let dot = to.normalize_or_zero().dot(cam_dir);
        if dot < max_angle_cos {
            continue;
        }
        if best.is_none_or(|(_, d)| dist < d) {
            best = Some((e, dist));
        }
    }
    best.map(|(e, _)| e)
}