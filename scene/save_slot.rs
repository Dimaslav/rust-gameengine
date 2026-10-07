//! Система сохранений: слоты, версионирование, миграции.
//!
//! Каждый слот — один файл `saves/slot_N.ron` со структурой `SaveFile`:
//!   * `header` — метаданные (версия, таймстемп, playtime, stage);
//!   * `scene_ron` — RON-снапшот мира (та же схема, что Save Scene);
//!   * `game_state_ron` — opaque-строка от игры (CampaignState и т.п.).
//!
//! Движок не парсит `game_state_ron` — это дело игры. Движок только
//! хранит и отдаёт его обратно.
//!
//! ## Версионирование
//!
//! `SaveHeader::format_version` инкрементируется при любом изменении
//! схемы сохранения (добавление поля, переименование, смена типа).
//! При загрузке слотов со старой версией запускаются миграции
//! (`migrate_v1_to_v2` и т.д.). Пока версия одна — `CURRENT_FORMAT = 1`,
//! миграций нет, но каркас готов.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Текущая версия формата сохранения.
pub const CURRENT_FORMAT: u32 = 1;

/// Количество слотов для UI.
pub const MAX_SLOTS: usize = 4;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveHeader {
    pub format_version: u32,
    pub game_version: String,
    /// Unix timestamp (секунды).
    pub timestamp_unix: u64,
    pub playtime_secs: f32,
    /// Название текущей стадии (для отображения в UI).
    pub stage_title: String,
    /// Общее число убийств (или другой прогресс).
    pub kills: u32,
}

impl SaveHeader {
    pub fn empty() -> Self {
        Self {
            format_version: CURRENT_FORMAT,
            game_version: "0.0.0".into(),
            timestamp_unix: 0,
            playtime_secs: 0.0,
            stage_title: "—".into(),
            kills: 0,
        }
    }

    /// Сколько секунд назад сохранено. Используется в UI.
    pub fn ago_secs(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.timestamp_unix)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaveFile {
    pub header: SaveHeader,
    /// RON-снапшот сцены. Формат совпадает с обычным Save Scene.
    pub scene_ron: String,
    /// Opaque game state от игры (RON-строка).
    #[serde(default)]
    pub game_state_ron: Option<String>,
}

pub struct SaveManager {
    dir: PathBuf,
}

impl SaveManager {
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        let dir = dir.into();
        if !dir.exists() {
            if let Err(e) = std::fs::create_dir_all(&dir) {
                log::warn!("SaveManager: cannot create {}: {}", dir.display(), e);
            }
        }
        Self { dir }
    }

    pub fn dir(&self) -> &Path { &self.dir }

    fn slot_path(&self, slot: u32) -> PathBuf {
        self.dir.join(format!("slot_{}.ron", slot))
    }

    pub fn exists(&self, slot: u32) -> bool {
        self.slot_path(slot).exists()
    }

    /// Сохранить слот. Атомарная запись через `.tmp` + rename,
    /// чтобы не оставить полусочинённый файл при падении.
    pub fn save(
        &self,
        slot: u32,
        scene_ron: String,
        game_state_ron: Option<String>,
        header: SaveHeader,
    ) -> Result<()> {
        let file = SaveFile {
            header,
            scene_ron,
            game_state_ron,
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
            .context("serialize save file")?;

        let final_path = self.slot_path(slot);
        let tmp_path = final_path.with_extension("ron.tmp");
        std::fs::write(&tmp_path, text)
            .with_context(|| format!("write {}", tmp_path.display()))?;
        std::fs::rename(&tmp_path, &final_path)
            .with_context(|| format!("rename to {}", final_path.display()))?;
        Ok(())
    }

    pub fn load(&self, slot: u32) -> Result<SaveFile> {
        let path = self.slot_path(slot);
        let text = std::fs::read_to_string(&path)
            .with_context(|| format!("read slot {}", slot))?;
        let file: SaveFile = ron::from_str(&text)
            .with_context(|| format!("parse slot {}", slot))?;

        // Проверка версии.
        if file.header.format_version > CURRENT_FORMAT {
            anyhow::bail!(
                "Save slot {} has format v{} (newer than supported v{}). \
                 Update the game to load this save.",
                slot,
                file.header.format_version,
                CURRENT_FORMAT,
            );
        }

        // Здесь могут быть миграции:
        //   if file.header.format_version < 2 { file = migrate_1_to_2(file)?; }
        //   if file.header.format_version < 3 { file = migrate_2_to_3(file)?; }
        // Пока только v1 — миграции не нужны.

        Ok(file)
    }

    /// Быстрое чтение только header (без парсинга scene_ron).
    /// Используется в UI для отображения списка слотов.
    pub fn header(&self, slot: u32) -> Option<SaveHeader> {
        self.load(slot).ok().map(|f| f.header)
    }

    pub fn delete(&self, slot: u32) -> Result<()> {
        let path = self.slot_path(slot);
        if path.exists() {
            std::fs::remove_file(&path)
                .with_context(|| format!("delete {}", path.display()))?;
        }
        Ok(())
    }

    /// Список всех слотов: `(slot, Option<SaveHeader>)`.
    pub fn list(&self) -> Vec<(u32, Option<SaveHeader>)> {
        (0..MAX_SLOTS as u32)
            .map(|i| (i, self.header(i)))
            .collect()
    }
}