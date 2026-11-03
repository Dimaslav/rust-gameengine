//! Data-driven квесты.
//!
//! Загрузка (`load_from_dir`) терпима к битым `.ron` в папке: файл,
//! который не парсится как `QuestDef`, пропускается с `log::warn`.
//! Раньше `?` валил всю загрузку — один ошибочно положенный файл
//! (например, DialogGraph в `assets/quests/`) ломал ВСЕ квесты в игре.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

// ============================================================
// Определения
// ============================================================

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuestDef {
    pub id: String,
    pub title: String,
    pub description: String,
    pub objectives: Vec<Objective>,
    pub rewards: Vec<Reward>,
    /// `true` — награда выдаётся автоматически при выполнении.
    /// `false` — игрок должен прийти к NPC и сдать квест через диалог.
    #[serde(default = "default_auto_complete")]
    pub auto_complete: bool,
}

fn default_auto_complete() -> bool {
    true
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Objective {
    Kill { target_prefix: String, count: u32 },
    Collect { item_id: String, count: u32 },
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Reward {
    Xp(u32),
    Gold(u32),
    Item { id: String, count: u32 },
}

// ============================================================
// QuestManager
// ============================================================

pub struct QuestManager {
    pub registry: HashMap<String, QuestDef>,
    /// Активные квесты: `id → progress`.
    pub active: HashMap<String, u32>,
    /// Квесты, чьи цели выполнены, но не сданы (`auto_complete == false`).
    pub objectives_done: HashSet<String>,
    /// Сданные квесты (награда уже получена).
    pub completed: HashSet<String>,
}

impl QuestManager {
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
            active: HashMap::new(),
            objectives_done: HashSet::new(),
            completed: HashSet::new(),
        }
    }

    /// Загрузка всех `.ron` из папки.
    ///
    /// ИЗМЕНЕНО: файлы, которые не парсятся как `QuestDef`, **пропускаются**
    /// с `log::warn`. Раньше `?` валил всю загрузку — один ошибочный файл
    /// оставлял `QuestManager` пустым и ломал `start_quest`.
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut manager = Self::new();
        if !dir.exists() {
            log::warn!("Quests dir {} not found, skipping.", dir.display());
            return Ok(manager);
        }

        let entries = std::fs::read_dir(dir).context("read quests dir")?;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("ron") {
                continue;
            }
            let text = match std::fs::read_to_string(&path) {
                Ok(t) => t,
                Err(e) => {
                    log::warn!("quest '{}': read failed: {}", path.display(), e);
                    continue;
                }
            };
            match ron::from_str::<QuestDef>(&text) {
                Ok(quest) => {
                    log::info!(
                        "quest: loaded '{}' ({} objectives, {} rewards) from {}",
                        quest.id, quest.objectives.len(), quest.rewards.len(),
                        path.display()
                    );
                    manager.registry.insert(quest.id.clone(), quest);
                }
                Err(e) => {
                    log::warn!(
                        "quest '{}': parse failed ({}). Пропускаю файл. \
                         Если это DialogGraph, перенесите его в assets/dialogs/.",
                        path.display(), e
                    );
                }
            }
        }
        Ok(manager)
    }

    pub fn start_quest(&mut self, id: &str) {
        if self.registry.contains_key(id)
            && !self.completed.contains(id)
            && !self.active.contains_key(id)
        {
            self.active.insert(id.to_string(), 0);
            log::info!("Quest started: {}", id);
        } else if !self.registry.contains_key(id) {
            log::warn!("start_quest: '{}' not in registry", id);
        }
    }

    /// Возвращает награды **только для auto-complete квестов**.
    /// Для квестов с `auto_complete: false` возвращается пустой Vec;
    /// они переходят в `objectives_done`, и награда выдаётся через `complete_quest`.
    pub fn on_enemy_killed(&mut self, target_name: &str) -> Vec<Reward> {
        let already_done: HashSet<String> = self.objectives_done.clone();
        let mut newly_completed: Vec<String> = Vec::new();
        let mut rewards: Vec<Reward> = Vec::new();

        for (qid, progress) in self.active.iter_mut() {
            if already_done.contains(qid) {
                continue;
            }
            let Some(def) = self.registry.get(qid) else {
                continue;
            };
            for obj in &def.objectives {
                if let Objective::Kill { target_prefix, count } = obj {
                    if target_name.starts_with(target_prefix) {
                        *progress += 1;
                        if *progress >= *count {
                            newly_completed.push(qid.clone());
                        }
                        break;
                    }
                }
            }
        }

        for qid in newly_completed {
            let auto = self.registry.get(&qid).map(|d| d.auto_complete).unwrap_or(true);
            if auto {
                self.active.remove(&qid);
                self.completed.insert(qid.clone());
                if let Some(def) = self.registry.get(&qid) {
                    log::info!("Quest completed (auto): {}", qid);
                    rewards.extend(def.rewards.clone());
                }
            } else {
                self.objectives_done.insert(qid.clone());
                log::info!("Quest objectives done (awaiting turn-in): {}", qid);
            }
        }

        rewards
    }

    /// Сдать квест. Возвращает награды, если квест был готов к сдаче.
    pub fn complete_quest(&mut self, id: &str) -> Vec<Reward> {
        if !self.objectives_done.contains(id) {
            log::warn!("complete_quest: '{}' not ready for turn-in", id);
            return vec![];
        }
        self.active.remove(id);
        self.objectives_done.remove(id);
        self.completed.insert(id.to_string());
        if let Some(def) = self.registry.get(id) {
            log::info!("Quest turned in: {}", id);
            return def.rewards.clone();
        }
        vec![]
    }

    pub fn is_objectives_done(&self, id: &str) -> bool {
        self.objectives_done.contains(id)
    }

    pub fn is_active(&self, id: &str) -> bool {
        self.active.contains_key(id)
    }

    pub fn is_completed(&self, id: &str) -> bool {
        self.completed.contains(id)
    }
}

impl Default for QuestManager {
    fn default() -> Self {
        Self::new()
    }
}