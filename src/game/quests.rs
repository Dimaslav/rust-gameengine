use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct QuestDef {
    pub id: String,
    pub title: String,
    pub description: String,
    pub objectives: Vec<Objective>,
    pub rewards: Vec<Reward>,
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

pub struct QuestManager {
    pub registry: HashMap<String, QuestDef>,
    pub active: HashMap<String, u32>, // quest_id -> progress
    pub completed: HashSet<String>,
}

impl QuestManager {
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
            active: HashMap::new(),
            completed: HashSet::new(),
        }
    }

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
            if path.extension().and_then(|s| s.to_str()) == Some("ron") {
                let text = std::fs::read_to_string(&path)?;
                let quest: QuestDef = ron::from_str(&text)
                    .with_context(|| format!("parse quest {}", path.display()))?;
                manager.registry.insert(quest.id.clone(), quest);
            }
        }
        Ok(manager)
    }

    pub fn start_quest(&mut self, id: &str) {
        if self.registry.contains_key(id) && !self.completed.contains(id) && !self.active.contains_key(id) {
            self.active.insert(id.to_string(), 0);
            log::info!("Quest started: {}", id);
        }
    }

    /// Возвращает награды, если квест был завершен этим действием.
    pub fn on_enemy_killed(&mut self, target_name: &str) -> Vec<Reward> {
        let mut rewards = Vec::new();
        let mut completed_quests = Vec::new();

        for (qid, progress) in self.active.iter_mut() {
            let def = &self.registry[qid];
            for obj in &def.objectives {
                if let Objective::Kill { target_prefix, count } = obj {
                    if target_name.starts_with(target_prefix) {
                        *progress += 1;
                        if *progress >= *count {
                            completed_quests.push(qid.clone());
                        }
                        break; // Only count once per kill per quest
                    }
                }
            }
        }

        for qid in completed_quests {
            self.active.remove(&qid);
            self.completed.insert(qid.clone());
            if let Some(def) = self.registry.get(&qid) {
                log::info!("Quest completed: {}", qid);
                rewards.extend(def.rewards.clone());
            }
        }

        rewards
    }
}