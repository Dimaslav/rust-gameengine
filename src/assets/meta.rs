//! Метаданные ассета: `AssetMeta`, тип ассета, настройки импорта.
//! Хранится рядом с файлом в `<name>.<ext>.meta` (RON).

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::id::AssetId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AssetKind {
    Mesh,
    Texture,
    Material,
    Scene,
    Prefab,
    Audio,
    Animation,
    Shader,
    Script,

    // === Data-driven gameplay (.ron) ===
    /// Предмет (`assets/items/*.ron`).
    Item,
    /// Оружие (`assets/weapons/*.ron`).
    Weapon,
    /// Квест (`assets/quests/*.ron`).
    Quest,
    /// Диалоговый граф (`assets/dialogs/*.ron`).
    Dialog,
    /// Вендор (`assets/vendors/*.ron`).
    Vendor,
    /// Крафт-рецепт (`assets/recipes/*.ron`).
    Recipe,
    /// Лут-таблица (`assets/loot/*.ron`).
    Loot,
    /// Локализация (`assets/locale/*.ron`).
    Locale,
    /// Общий `.ron` вне известных папок (generic).
    Data,

    /// Что-то, что мы не умеем импортировать, но хотим видеть в браузере.
    Other,
}

impl AssetKind {
    pub fn icon(&self) -> &'static str {
        match self {
            AssetKind::Mesh => "🧊",
            AssetKind::Texture => "🖼",
            AssetKind::Material => "🎨",
            AssetKind::Scene => "🎬",
            AssetKind::Prefab => "📦",
            AssetKind::Audio => "🔊",
            AssetKind::Animation => "🎞",
            AssetKind::Shader => "✨",
            AssetKind::Script => "📜",
            AssetKind::Item => "🎒",
            AssetKind::Weapon => "🔫",
            AssetKind::Quest => "📋",
            AssetKind::Dialog => "💬",
            AssetKind::Vendor => "🏪",
            AssetKind::Recipe => "🔨",
            AssetKind::Loot => "🎲",
            AssetKind::Locale => "🌐",
            AssetKind::Data => "📊",
            AssetKind::Other => "📄",
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            AssetKind::Mesh => "Mesh",
            AssetKind::Texture => "Texture",
            AssetKind::Material => "Material",
            AssetKind::Scene => "Scene",
            AssetKind::Prefab => "Prefab",
            AssetKind::Audio => "Audio",
            AssetKind::Animation => "Animation",
            AssetKind::Shader => "Shader",
            AssetKind::Script => "Script",
            AssetKind::Item => "Item",
            AssetKind::Weapon => "Weapon",
            AssetKind::Quest => "Quest",
            AssetKind::Dialog => "Dialog",
            AssetKind::Vendor => "Vendor",
            AssetKind::Recipe => "Recipe",
            AssetKind::Loot => "Loot",
            AssetKind::Locale => "Locale",
            AssetKind::Data => "Data",
            AssetKind::Other => "Other",
        }
    }
}

/// Настройки импорта — зависят от типа ассета.
/// Разработчик игры может их менять в Content Browser.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportSettings {
    /// sRGB (albedo, emissive) или Linear (normal, MR, AO, height).
    #[serde(default = "default_true")]
    pub srgb: bool,

    /// Генерировать мипмапы. Только для текстур.
    #[serde(default = "default_true")]
    pub generate_mips: bool,

    /// Множитель масштаба при импорте меша.
    #[serde(default = "default_one")]
    pub import_scale: f32,

    /// Отбрасывать ли ассет при сборке игры.
    #[serde(default)]
    pub excluded_from_build: bool,
}

fn default_true() -> bool { true }
fn default_one() -> f32 { 1.0 }

impl Default for ImportSettings {
    fn default() -> Self {
        Self {
            srgb: true,
            generate_mips: true,
            import_scale: 1.0,
            excluded_from_build: false,
        }
    }
}

#[derive(Debug, Clone)]
pub struct AssetMeta {
    pub id: AssetId,
    pub path: PathBuf,
    pub kind: AssetKind,
    /// Хэш содержимого файла. Используется для детекта изменений.
    pub content_hash: u64,
    pub import_settings: ImportSettings,
    /// Ассеты, от которых зависит этот (для packaging).
    pub dependencies: Vec<AssetId>,
}

impl AssetMeta {
    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    }

    pub fn stem(&self) -> String {
        self.path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string()
    }
}

/// Формат `.meta`-файла.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AssetMetaFile {
    pub id: AssetId,
    pub kind: AssetKind,
    pub import_settings: ImportSettings,
    #[serde(default)]
    pub dependencies: Vec<AssetId>,
}