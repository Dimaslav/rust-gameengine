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
    #[serde(default = "default_auto_complete")]
    pub auto_complete: bool,
}

fn default_auto_complete() -> bool { true }

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
    pub active: HashMap<String, u32>,
    pub objectives_done: HashSet<String>,
    pub completed: HashSet<String>,
    kills_by_prefix: HashMap<String, u32>,
}

impl QuestManager {
    pub fn new() -> Self {
        Self {
            registry: HashMap::new(),
            active: HashMap::new(),
            objectives_done: HashSet::new(),
            completed: HashSet::new(),
            kills_by_prefix: HashMap::new(),
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
                    if manager.registry.contains_key(&quest.id) {
                        log::error!(
                            "quest '{}': duplicate id in {} — skipped (already loaded)",
                            quest.id, path.display()
                        );
                        continue;
                    }
                    log::info!(
                        "quest: loaded '{}' ({} objectives, {} rewards) from {}",
                        quest.id,
                        quest.objectives.len(),
                        quest.rewards.len(),
                        path.display()
                    );
                    manager.registry.insert(quest.id.clone(), quest);
                }
                Err(e) => {
                    log::warn!(
                        "quest '{}': parse failed ({}). Пропускаю файл. \
                         Если это DialogGraph, перенесите его в assets/dialogs/.",
                        path.display(),
                        e
                    );
                }
            }
        }
        Ok(manager)
    }

    pub fn start_quest(&mut self, id: &str) -> Vec<Reward> {
        if self.completed.contains(id) || self.active.contains_key(id) {
            return Vec::new();
        }
        let Some(def) = self.registry.get(id).cloned() else {
            log::warn!("start_quest: '{}' not in registry", id);
            return Vec::new();
        };

        let mut initial_progress: u32 = 0;
        let mut all_kill_met = true;
        let mut has_collect = false;
        for obj in &def.objectives {
            match obj {
                Objective::Kill { target_prefix, count } => {
                    let have = self
                        .kills_by_prefix
                        .get(target_prefix)
                        .copied()
                        .unwrap_or(0);
                    initial_progress = initial_progress.saturating_add(have);
                    if have < *count {
                        all_kill_met = false;
                    }
                }
                Objective::Collect { .. } => {
                    has_collect = true;
                }
            }
        }

        self.active.insert(id.to_string(), initial_progress);

        if all_kill_met && !has_collect {
            if def.auto_complete {
                self.active.remove(id);
                self.completed.insert(id.to_string());
                log::info!(
                    "Quest '{}' started and immediately completed \
                     (objectives already met)",
                    id
                );
                return def.rewards.clone();
            } else {
                self.objectives_done.insert(id.to_string());
                log::info!(
                    "Quest '{}' started with objectives already met \
                     (awaiting turn-in)",
                    id
                );
            }
        } else {
            log::info!("Quest started: {}", id);
        }
        Vec::new()
    }

    pub fn on_enemy_killed(&mut self, target_name: &str) -> Vec<Reward> {
        let mut matching: HashSet<String> = HashSet::new();
        for def in self.registry.values() {
            for obj in &def.objectives {
                if let Objective::Kill { target_prefix, .. } = obj
                    && target_name.starts_with(target_prefix)
                {
                    matching.insert(target_prefix.clone());
                }
            }
        }
        for prefix in matching {
            *self.kills_by_prefix.entry(prefix).or_insert(0) += 1;
        }

        struct Update {
            qid: String,
            total: u32,
            all_kill_met: bool,
            has_collect: bool,
        }

        let mut updates: Vec<Update> = Vec::new();
        for (qid, _) in self.active.iter() {
            if self.objectives_done.contains(qid) {
                continue;
            }
            let Some(def) = self.registry.get(qid) else {
                continue;
            };
            let mut total: u32 = 0;
            let mut all_kill_met = true;
            let mut has_collect = false;
            for obj in &def.objectives {
                match obj {
                    Objective::Kill {
                        target_prefix,
                        count,
                    } => {
                        let have = self
                            .kills_by_prefix
                            .get(target_prefix)
                            .copied()
                            .unwrap_or(0);
                        total = total.saturating_add(have);
                        if have < *count {
                            all_kill_met = false;
                        }
                    }
                    Objective::Collect { .. } => {
                        has_collect = true;
                    }
                }
            }
            updates.push(Update {
                qid: qid.clone(),
                total,
                all_kill_met,
                has_collect,
            });
        }

        let mut rewards: Vec<Reward> = Vec::new();
        for u in updates {
            if let Some(p) = self.active.get_mut(&u.qid) {
                *p = u.total;
            }
            if u.all_kill_met && !u.has_collect {
                let auto = self
                    .registry
                    .get(&u.qid)
                    .map(|d| d.auto_complete)
                    .unwrap_or(true);
                if auto {
                    self.active.remove(&u.qid);
                    self.completed.insert(u.qid.clone());
                    if let Some(def) = self.registry.get(&u.qid) {
                        log::info!("Quest completed (auto): {}", u.qid);
                        rewards.extend(def.rewards.clone());
                    }
                } else {
                    self.objectives_done.insert(u.qid.clone());
                    log::info!("Quest objectives done (awaiting turn-in): {}", u.qid);
                }
            }
        }
        rewards
    }

    pub fn complete_quest(&mut self, id: &str) -> Vec<Reward> {
        if !self.objectives_done.contains(id) {
            log::warn!("complete_quest: '{}' not ready for turn-in", id);
            return Vec::new();
        }
        self.active.remove(id);
        self.objectives_done.remove(id);
        self.completed.insert(id.to_string());
        if let Some(def) = self.registry.get(id) {
            log::info!("Quest turned in: {}", id);
            return def.rewards.clone();
        }
        Vec::new()
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

    pub fn kills_count(&self, prefix: &str) -> u32 {
        self.kills_by_prefix.get(prefix).copied().unwrap_or(0)
    }

    pub fn progress_snapshot(&self) -> Vec<(String, u32, u32)> {
        let mut out = Vec::with_capacity(self.active.len());
        for (id, progress) in &self.active {
            let required: u32 = self
                .registry
                .get(id)
                .map(|d| {
                    d.objectives
                        .iter()
                        .filter_map(|o| match o {
                            Objective::Kill { count, .. } => Some(*count),
                            _ => None,
                        })
                        .sum()
                })
                .unwrap_or(0);
            out.push((id.clone(), *progress, required));
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }
}

impl Default for QuestManager {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuestManagerState {
    #[serde(default)]
    pub active: HashMap<String, u32>,
    #[serde(default)]
    pub objectives_done: HashSet<String>,
    #[serde(default)]
    pub completed: HashSet<String>,
    #[serde(default)]
    pub kills_by_prefix: HashMap<String, u32>,
}

impl QuestManager {
    pub fn snapshot(&self) -> QuestManagerState {
        QuestManagerState {
            active: self.active.clone(),
            objectives_done: self.objectives_done.clone(),
            completed: self.completed.clone(),
            kills_by_prefix: self.kills_by_prefix.clone(),
        }
    }

    pub fn restore(&mut self, state: QuestManagerState) {
        self.active = state.active;
        self.objectives_done = state.objectives_done;
        self.completed = state.completed;
        self.kills_by_prefix = state.kills_by_prefix;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kill_def(id: &str, prefix: &str, count: u32, auto: bool) -> QuestDef {
        QuestDef {
            id: id.into(),
            title: id.into(),
            description: String::new(),
            objectives: vec![Objective::Kill {
                target_prefix: prefix.into(),
                count,
            }],
            rewards: vec![Reward::Xp(10), Reward::Gold(100)],
            auto_complete: auto,
        }
    }

    fn manager_with(defs: Vec<QuestDef>) -> QuestManager {
        let mut m = QuestManager::new();
        for d in defs {
            m.registry.insert(d.id.clone(), d);
        }
        m
    }

    #[test]
    fn kill_completes_auto_quest() {
        let mut m = manager_with(vec![kill_def("q1", "Enemy_", 2, true)]);
        m.start_quest("q1");
        assert!(m.is_active("q1"));

        let r = m.on_enemy_killed("Enemy_A");
        assert!(r.is_empty());
        assert!(m.is_active("q1"));

        let r = m.on_enemy_killed("Enemy_B");
        assert_eq!(r.len(), 2);
        assert!(!m.is_active("q1"));
        assert!(m.is_completed("q1"));
    }

    #[test]
    fn multi_objective_requires_all() {
        let mut m = QuestManager::new();
        m.registry.insert(
            "multi".into(),
            QuestDef {
                id: "multi".into(),
                title: "multi".into(),
                description: String::new(),
                objectives: vec![
                    Objective::Kill {
                        target_prefix: "A_".into(),
                        count: 1,
                    },
                    Objective::Kill {
                        target_prefix: "B_".into(),
                        count: 1,
                    },
                ],
                rewards: vec![Reward::Xp(10)],
                auto_complete: true,
            },
        );
        m.start_quest("multi");

        m.on_enemy_killed("A_1");
        assert!(m.is_active("multi"));
        assert!(!m.is_completed("multi"));

        let r = m.on_enemy_killed("B_1");
        assert_eq!(r.len(), 1);
        assert!(m.is_completed("multi"));
    }

    #[test]
    fn retroactive_kill_marks_objectives_done_on_start() {
        let mut m = manager_with(vec![kill_def("hunt", "A_", 2, false)]);
        m.on_enemy_killed("A_1");
        m.on_enemy_killed("A_2");

        assert!(!m.is_active("hunt"));
        assert!(!m.is_objectives_done("hunt"));
        assert!(!m.is_completed("hunt"));

        m.start_quest("hunt");
        assert!(m.is_active("hunt"));
        assert!(m.is_objectives_done("hunt"));

        let r = m.complete_quest("hunt");
        assert_eq!(r.len(), 2);
        assert!(m.is_completed("hunt"));
    }

    #[test]
    fn retroactive_auto_quest_returns_rewards_immediately() {
        let mut m = manager_with(vec![kill_def("auto", "X_", 1, true)]);
        m.on_enemy_killed("X_1");
        assert!(!m.is_completed("auto"));

        let rewards = m.start_quest("auto");
        assert_eq!(rewards.len(), 2);
        assert!(m.is_completed("auto"));
        assert!(!m.is_active("auto"));
    }

    #[test]
    fn duplicate_prefix_in_quest_counts_once_per_kill() {
        let mut m = QuestManager::new();
        m.registry.insert(
            "dup".into(),
            QuestDef {
                id: "dup".into(),
                title: "dup".into(),
                description: String::new(),
                objectives: vec![
                    Objective::Kill {
                        target_prefix: "X_".into(),
                        count: 2,
                    },
                    Objective::Kill {
                        target_prefix: "X_".into(),
                        count: 2,
                    },
                ],
                rewards: vec![Reward::Xp(10)],
                auto_complete: true,
            },
        );
        m.start_quest("dup");

        m.on_enemy_killed("X_1");
        assert!(m.is_active("dup"));
        assert_eq!(m.kills_count("X_"), 1);

        m.on_enemy_killed("X_2");
        assert_eq!(m.kills_count("X_"), 2);
        assert!(m.is_completed("dup"));
    }

    #[test]
    fn same_prefix_across_quests_counts_once_per_kill() {
        let mut m = manager_with(vec![
            kill_def("a", "Z_", 1, true),
            kill_def("b", "Z_", 1, true),
        ]);
        m.start_quest("a");
        m.start_quest("b");

        m.on_enemy_killed("Z_1");

        assert!(m.is_completed("a"));
        assert!(m.is_completed("b"));
        assert_eq!(m.kills_count("Z_"), 1);
    }

    #[test]
    fn collect_objective_blocks_auto_completion() {
        let mut m = QuestManager::new();
        m.registry.insert(
            "col".into(),
            QuestDef {
                id: "col".into(),
                title: "col".into(),
                description: String::new(),
                objectives: vec![Objective::Collect {
                    item_id: "herb".into(),
                    count: 5,
                }],
                rewards: vec![Reward::Xp(10)],
                auto_complete: true,
            },
        );
        m.start_quest("col");

        m.on_enemy_killed("Anything");
        assert!(m.is_active("col"));
        assert!(!m.is_completed("col"));
        assert!(!m.is_objectives_done("col"));
    }

    #[test]
    fn start_quest_is_idempotent() {
        let mut m = manager_with(vec![kill_def("q", "E_", 1, false)]);
        m.start_quest("q");
        let rewards = m.start_quest("q");
        assert!(rewards.is_empty());
        assert!(m.is_active("q"));
    }

    #[test]
    fn complete_quest_requires_objectives_done() {
        let mut m = manager_with(vec![kill_def("q", "E_", 1, false)]);
        m.start_quest("q");
        let rewards = m.complete_quest("q");
        assert!(rewards.is_empty());
        assert!(m.is_active("q"));
        assert!(!m.is_completed("q"));
    }

    #[test]
    fn progress_snapshot_reports_active() {
        let mut m = manager_with(vec![
            kill_def("a", "E_", 3, true),
            kill_def("b", "F_", 1, true),
        ]);
        m.start_quest("a");
        m.start_quest("b");
        m.on_enemy_killed("E_1");

        let snap = m.progress_snapshot();
        assert_eq!(snap.len(), 2);
        let a = snap.iter().find(|(id, _, _)| id == "a").unwrap();
        assert_eq!(a.1, 1);
        assert_eq!(a.2, 3);
    }
}