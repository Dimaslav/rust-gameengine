//! Глобальное состояние приложения: режим (меню / игра), настройки.
//! Настройки сериализуются в `settings.ron` рядом с exe.

use serde::{Deserialize, Serialize};

// ============================================================
// AppMode
// ============================================================

#[derive(Debug, Clone)]
pub enum AppMode {
    /// Главное меню. Сцена НЕ обновляется, но рендерится (фон).
    MainMenu,
    /// Обычная игра/редактор.
    InGame,
    /// Настройки. `return_to` — куда вернуться по «Back».
    Settings { return_to: Box<AppMode> },
}

impl Default for AppMode {
    fn default() -> Self {
        // Движок стартует сразу в редакторе (без главного меню).
        // Меню доступно через SKIP_MAIN_MENU=0 или в standalone-билде.
        AppMode::InGame
    }
}

impl AppMode {
    pub fn is_menu(&self) -> bool {
        matches!(self, AppMode::MainMenu | AppMode::Settings { .. })
    }
}

// ============================================================
// AppSettings
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default)] pub window: WindowSettings,
    #[serde(default)] pub graphics: GraphicsSettings,
    #[serde(default)] pub audio: AudioSettings,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            window: WindowSettings::default(),
            graphics: GraphicsSettings::default(),
            audio: AudioSettings::default(),
        }
    }
}

impl AppSettings {
    pub fn load_or_default(path: &str) -> Self {
        let Ok(text) = std::fs::read_to_string(path) else {
            return Self::default();
        };
        ron::from_str(&text).unwrap_or_default()
    }

    pub fn save(&self, path: &str) -> std::io::Result<()> {
        let text = ron::ser::to_string_pretty(self, ron::ser::PrettyConfig::default())
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(path, text)
    }
}

// ============================================================
// Window
// ============================================================

pub const RESOLUTIONS: &[(u32, u32, &str)] = &[
    (1280, 720, "1280 × 720"),
    (1366, 768, "1366 × 768"),
    (1600, 900, "1600 × 900"),
    (1920, 1080, "1920 × 1080"),
    (2560, 1440, "2560 × 1440"),
    (3840, 2160, "3840 × 2160"),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WindowSettings {
    pub vsync: bool,
    pub fullscreen: bool,
    pub resolution_index: usize,
}

impl Default for WindowSettings {
    fn default() -> Self {
        Self { vsync: true, fullscreen: false, resolution_index: 3 }
    }
}

// ============================================================
// Graphics
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum QualityPreset {
    Low,
    Medium,
    High,
    Ultra,
}

impl QualityPreset {
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::Ultra => "Ultra",
        }
    }
    pub fn all() -> [Self; 4] {
        [Self::Low, Self::Medium, Self::High, Self::Ultra]
    }
    pub fn description(self) -> &'static str {
        match self {
            Self::Low => "Без TAA, SSAO, bloom. Максимум FPS.",
            Self::Medium => "Мягкое SSAO, лёгкий bloom, TAA.",
            Self::High => "Полное SSAO, заметный bloom, TAA + sharpen.",
            Self::Ultra => "Всё + volumetric + SSR + DoF + motion blur.",
        }
    }
}

impl Default for QualityPreset {
    fn default() -> Self { Self::High }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphicsSettings {
    pub quality_preset: QualityPreset,
    pub render_scale: f32,
    pub fov: f32,
    pub show_fps: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            quality_preset: QualityPreset::High,
            render_scale: 1.0,
            fov: 75.0,
            show_fps: true,
        }
    }
}

// ============================================================
// Audio
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSettings {
    pub master: f32,
    pub sfx: f32,
    pub music: f32,
    pub voice: f32,
    pub ui: f32,
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self { master: 1.0, sfx: 1.0, music: 0.7, voice: 1.0, ui: 0.9 }
    }
}