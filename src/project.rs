//! Система проектов: чтение `project.ron`.
//!
//! Позволяет редактору и рантайму понимать, где лежат ассеты,
//! и какую сцену загружать по умолчанию.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    /// Название игры.
    pub name: String,
    /// Версия движка, под которую собирался проект.
    #[serde(default)]
    pub engine_version: String,
    /// Папка с ассетами (относительно корня проекта).
    #[serde(default = "default_assets")]
    pub assets_root: String,
    /// Сцена, которая загружается при старте (относительно корня).
    #[serde(default)]
    pub entry_scene: Option<String>,
}

fn default_assets() -> String {
    "assets".to_string()
}

impl ProjectManifest {
    /// Загрузить манифест из файла `project.ron`.
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read project manifest: {}", path.display()))?;
        let manifest: Self = ron::from_str(&text)
            .with_context(|| format!("failed to parse project manifest: {}", path.display()))?;
        Ok(manifest)
    }

    /// Корень проекта (папка, где лежит project.ron).
    pub fn root(&self) -> PathBuf {
        // Так как манифест загружается из <root>/project.ron,
        // мы просто используем текущую директорию (она будет изменена).
        PathBuf::from(".")
    }

    /// Полный путь до папки с ассетами.
    pub fn assets_path(&self) -> PathBuf {
        self.root().join(&self.assets_root)
    }
}