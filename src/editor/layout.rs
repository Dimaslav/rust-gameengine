//! Состояние раскладки редактора.
//!
//! Пока — плоский набор bool'ов, совместимый с `EditorSettings` из
//! `editor.ron`. В Шаге 2 (`egui_tiles`) это поле заменится на
//! dock-tree, но публичный API (`is_visible`, `toggle`) сохранится.
//!
//! ## Зачем отдельный `LayoutState`, а не поля в `EditorState`
//!
//! * Сериализуется в `editor.ron` **отдельно** от логики (selected, undo, …).
//! * В Шаге 2 сюда добавится `TileTree<PanelId>` без правок `EditorState`.
//! * `PanelId` — единый идентификатор для UI-кнопок, хоткеев и (позже)
//!   `egui_tiles`.

use serde::{Deserialize, Serialize};

use super::settings::EditorSettings;

// ============================================================
// PanelId
// ============================================================

/// Все панели редактора. Используется в:
/// * `LayoutState::is_visible` / `toggle`,
/// * `View`-меню для показа/скрытия,
/// * (Шаг 2) `TileTree<PanelId>`.
///
/// `*Bar`-панели (`PaletteBar`, `TerrainBar`) — верхние полосы,
/// которые в Шаге 3 станут floating-тулбарами поверх viewport'а.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    // --- Верхние полосы ---
    TopBar,
    PaletteBar,
    TerrainBar,

    // --- Боковые ---
    Outliner,
    Details,
    Stats,

    // --- Низ ---
    Content,
    Console,

    // --- Окна (плавающие) ---
    Renderer,
    Audio,
    Bookmarks,
    Scripts,
    CommandPalette,

    // --- Центр ---
    Viewport,

    // --- Резерв ---
    Timeline,
}

impl PanelId {
    pub const ALL: &'static [PanelId] = &[
        PanelId::Outliner,
        PanelId::Details,
        PanelId::Stats,
        PanelId::Content,
        PanelId::Console,
        PanelId::Renderer,
        PanelId::Audio,
        PanelId::Bookmarks,
        PanelId::Scripts,
        PanelId::PaletteBar,
        PanelId::TerrainBar,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PanelId::TopBar       => "Top Bar",
            PanelId::PaletteBar   => "Palette",
            PanelId::TerrainBar   => "Terrain",
            PanelId::Outliner     => "Outliner",
            PanelId::Details      => "Details",
            PanelId::Stats        => "Stats",
            PanelId::Content      => "Content Browser",
            PanelId::Console      => "Console",
            PanelId::Renderer     => "Renderer",
            PanelId::Audio        => "Audio",
            PanelId::Bookmarks    => "Bookmarks",
            PanelId::Scripts      => "Scripts",
            PanelId::CommandPalette => "Command Palette",
            PanelId::Viewport     => "Viewport",
            PanelId::Timeline     => "Timeline",
        }
    }

    pub fn default_visible(self) -> bool {
        matches!(
            self,
            PanelId::Outliner
                | PanelId::Details
                | PanelId::Stats
                | PanelId::Content
                | PanelId::Renderer
        )
    }
}

// ============================================================
// LayoutState
// ============================================================

/// Раскладка панелей. Сериализуется в `editor.ron`.
///
/// Все поля `#[serde(default)]` — старые `editor.ron` (без новых полей)
/// загрузятся без ошибок. Это важно: `editor.ron` уже лежит у
/// пользователей, и ломать его нельзя.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutState {
    // --- Видимость панелей ---
    #[serde(default = "default_true")]
    pub show_renderer_panel: bool,
    #[serde(default = "default_true")]
    pub show_stats_panel: bool,
    #[serde(default = "default_true")]
    pub show_hierarchy_panel: bool,
    #[serde(default = "default_true")]
    pub show_inspector_panel: bool,
    #[serde(default)]
    pub show_audio_panel: bool,
    #[serde(default)]
    pub show_bookmarks_panel: bool,
    #[serde(default)]
    pub show_content_browser: bool,
    #[serde(default)]
    pub show_script_panel: bool,
    #[serde(default)]
    pub show_console_panel: bool,
    #[serde(default = "default_true")]
    pub show_palette_bar: bool,
    #[serde(default = "default_true")]
    pub show_terrain_bar: bool,

    // --- Размеры ---
    #[serde(default = "default_left_width")]
    pub left_panel_width: f32,
    #[serde(default = "default_right_width")]
    pub right_panel_width: f32,
    #[serde(default = "default_bottom_height")]
    pub bottom_panel_height: f32,
}

fn default_true() -> bool { true }
fn default_left_width() -> f32 { 260.0 }
fn default_right_width() -> f32 { 340.0 }
fn default_bottom_height() -> f32 { 220.0 }

impl Default for LayoutState {
    fn default() -> Self {
        Self {
            show_renderer_panel: true,
            show_stats_panel: true,
            show_hierarchy_panel: true,
            show_inspector_panel: true,
            show_audio_panel: false,
            show_bookmarks_panel: false,
            show_content_browser: false,
            show_script_panel: false,
            show_console_panel: false,
            show_palette_bar: true,
            show_terrain_bar: true,
            left_panel_width: default_left_width(),
            right_panel_width: default_right_width(),
            bottom_panel_height: default_bottom_height(),
        }
    }
}

impl LayoutState {
    /// Инициализация из сохранённых `EditorSettings` (загружены в
    /// `EditorState::new` из `editor.ron`).
    pub fn from_settings(s: &EditorSettings) -> Self {
        Self {
            show_renderer_panel: s.show_renderer_panel,
            show_stats_panel: s.show_stats_panel,
            show_hierarchy_panel: s.show_hierarchy_panel,
            show_inspector_panel: s.show_inspector_panel,
            show_audio_panel: s.show_audio_panel,
            show_bookmarks_panel: false,
            show_content_browser: false,
            show_script_panel: false,
            show_console_panel: false,
            show_palette_bar: true,
            show_terrain_bar: true,
            left_panel_width: s.left_panel_width,
            right_panel_width: s.right_panel_width,
            bottom_panel_height: default_bottom_height(),
        }
    }

    /// Записать текущее layout обратно в `EditorSettings`.
    pub fn write_into_settings(&self, s: &mut EditorSettings) {
        s.show_renderer_panel = self.show_renderer_panel;
        s.show_stats_panel = self.show_stats_panel;
        s.show_hierarchy_panel = self.show_hierarchy_panel;
        s.show_inspector_panel = self.show_inspector_panel;
        s.show_audio_panel = self.show_audio_panel;
        s.left_panel_width = self.left_panel_width;
        s.right_panel_width = self.right_panel_width;
    }

    // ---------------------------------------------------------
    // Универсальный доступ по PanelId
    // ---------------------------------------------------------

    pub fn is_visible(&self, id: PanelId) -> bool {
        match id {
            PanelId::TopBar        => true, // всегда
            PanelId::PaletteBar    => self.show_palette_bar,
            PanelId::TerrainBar    => self.show_terrain_bar,
            PanelId::Outliner      => self.show_hierarchy_panel,
            PanelId::Details       => self.show_inspector_panel,
            PanelId::Stats         => self.show_stats_panel,
            PanelId::Content       => self.show_content_browser,
            PanelId::Console       => self.show_console_panel,
            PanelId::Renderer      => self.show_renderer_panel,
            PanelId::Audio         => self.show_audio_panel,
            PanelId::Bookmarks     => self.show_bookmarks_panel,
            PanelId::Scripts       => self.show_script_panel,
            PanelId::CommandPalette => false, // transient, не храним
            PanelId::Viewport      => true, // всегда
            PanelId::Timeline      => false,
        }
    }

    pub fn toggle(&mut self, id: PanelId) {
        match id {
            PanelId::PaletteBar    => self.show_palette_bar = !self.show_palette_bar,
            PanelId::TerrainBar    => self.show_terrain_bar = !self.show_terrain_bar,
            PanelId::Outliner      => self.show_hierarchy_panel = !self.show_hierarchy_panel,
            PanelId::Details       => self.show_inspector_panel = !self.show_inspector_panel,
            PanelId::Stats         => self.show_stats_panel = !self.show_stats_panel,
            PanelId::Content       => self.show_content_browser = !self.show_content_browser,
            PanelId::Console       => self.show_console_panel = !self.show_console_panel,
            PanelId::Renderer      => self.show_renderer_panel = !self.show_renderer_panel,
            PanelId::Audio         => self.show_audio_panel = !self.show_audio_panel,
            PanelId::Bookmarks     => self.show_bookmarks_panel = !self.show_bookmarks_panel,
            PanelId::Scripts       => self.show_script_panel = !self.show_script_panel,
            _ => {} // TopBar, Viewport, CommandPalette, Timeline — не toggle'ятся
        }
    }

    pub fn set_visible(&mut self, id: PanelId, visible: bool) {
        if self.is_visible(id) != visible {
            self.toggle(id);
        }
    }
}