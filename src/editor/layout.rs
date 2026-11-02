//! Состояние раскладки редактора + Workspace presets + OutlinerFilter.
//!
//! ## Что такое Workspace
//!
//! Именованный набор «какие панели видимы и какой ширины». Пользователь
//! кликает таб в top bar → редактор сохраняет текущий `LayoutState` в
//! слот старого workspace'а, загружает `LayoutState` нового.
//!
//! | Workspace      | Когда использовать                          |
//! |----------------|---------------------------------------------|
//! | Layout         | Общая работа (всё видно)                    |
//! | Modeling       | Работа с мешами (большой viewport)          |
//! | Level Design   | Расстановка объектов (Content + Palette)    |
//! | Animation      | Работа с анимациями (Outliner + Details)    |
//! | Scripting      | Работа со скриптами (Scripts panel)         |
//!
//! ## OutlinerFilter
//!
//! Фильтр по типу компонента в Outliner'е. Хранится в
//! `EditorState::outliner_filter`, применяется в
//! `HierarchySnapshot::build` вместе с текстовым фильтром.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use super::settings::EditorSettings;

// ============================================================
// Workspace
// ============================================================

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash,
    Serialize, Deserialize,
)]
#[derive(Default)]
pub enum Workspace {
    #[default]
    Layout,
    Modeling,
    LevelDesign,
    Animation,
    Scripting,
}

impl Workspace {
    pub const ALL: &'static [Workspace] = &[
        Workspace::Layout,
        Workspace::Modeling,
        Workspace::LevelDesign,
        Workspace::Animation,
        Workspace::Scripting,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Workspace::Layout => "Layout",
            Workspace::Modeling => "Modeling",
            Workspace::LevelDesign => "Level Design",
            Workspace::Animation => "Animation",
            Workspace::Scripting => "Scripting",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            Workspace::Layout => "General-purpose editing (all panels visible)",
            Workspace::Modeling => "Focus on viewport; hide stats/renderer",
            Workspace::LevelDesign => "Large Content Browser; palette visible",
            Workspace::Animation => "Animation-focused layout",
            Workspace::Scripting => "Scripts panel visible; hide details",
        }
    }
}


// ============================================================
// PanelId
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PanelId {
    TopBar,
    PaletteBar,
    TerrainBar,
    Outliner,
    Details,
    Stats,
    Content,
    Console,
    Renderer,
    Audio,
    Bookmarks,
    Scripts,
    CommandPalette,
    Viewport,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayoutState {
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
    // Workspace presets
    // ---------------------------------------------------------

    pub fn preset_for(ws: Workspace) -> Self {
        let mut s = Self::default();
        match ws {
            Workspace::Layout => {
                s.show_hierarchy_panel = true;
                s.show_inspector_panel = true;
                s.show_stats_panel = true;
                s.show_renderer_panel = true;
                s.show_palette_bar = true;
                s.show_terrain_bar = true;
            }
            Workspace::Modeling => {
                s.show_hierarchy_panel = true;
                s.show_inspector_panel = true;
                s.show_stats_panel = false;
                s.show_renderer_panel = false;
                s.show_palette_bar = false;
                s.show_terrain_bar = false;
                s.left_panel_width = 240.0;
                s.right_panel_width = 320.0;
            }
            Workspace::LevelDesign => {
                s.show_hierarchy_panel = true;
                s.show_inspector_panel = false;
                s.show_stats_panel = true;
                s.show_renderer_panel = false;
                s.show_palette_bar = true;
                s.show_terrain_bar = true;
                s.show_content_browser = true;
                s.right_panel_width = 260.0;
            }
            Workspace::Animation => {
                s.show_hierarchy_panel = true;
                s.show_inspector_panel = true;
                s.show_stats_panel = false;
                s.show_renderer_panel = false;
                s.show_palette_bar = false;
                s.show_terrain_bar = false;
                s.left_panel_width = 240.0;
                s.right_panel_width = 340.0;
            }
            Workspace::Scripting => {
                s.show_hierarchy_panel = true;
                s.show_inspector_panel = false;
                s.show_stats_panel = false;
                s.show_renderer_panel = false;
                s.show_palette_bar = false;
                s.show_terrain_bar = false;
                s.show_script_panel = true;
            }
        }
        s
    }

    // ---------------------------------------------------------
    // Универсальный доступ по PanelId
    // ---------------------------------------------------------

    pub fn is_visible(&self, id: PanelId) -> bool {
        match id {
            PanelId::TopBar        => true,
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
            PanelId::CommandPalette => false,
            PanelId::Viewport      => true,
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
            _ => {}
        }
    }

    pub fn set_visible(&mut self, id: PanelId, visible: bool) {
        if self.is_visible(id) != visible {
            self.toggle(id);
        }
    }
}

// ============================================================
// WorkspaceLayouts
// ============================================================

pub type WorkspaceLayouts = BTreeMap<Workspace, LayoutState>;

// ============================================================
// OutlinerFilter
// ============================================================

/// Фильтр по типу entity в Outliner'е.
///
/// Хранится в `EditorState::outliner_filter`. Применяется в
/// `HierarchySnapshot::build` **вместе** с текстовым фильтром.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum OutlinerFilter {
    #[default]
    All,
    Meshes,
    Lights,
    Audio,
    Ai,
    Triggers,
}

impl OutlinerFilter {
    pub const ALL: &'static [OutlinerFilter] = &[
        OutlinerFilter::All,
        OutlinerFilter::Meshes,
        OutlinerFilter::Lights,
        OutlinerFilter::Audio,
        OutlinerFilter::Ai,
        OutlinerFilter::Triggers,
    ];

    pub fn label(self) -> &'static str {
        match self {
            OutlinerFilter::All      => "All",
            OutlinerFilter::Meshes   => "🖼",
            OutlinerFilter::Lights   => "💡",
            OutlinerFilter::Audio    => "🔊",
            OutlinerFilter::Ai       => "🤖",
            OutlinerFilter::Triggers => "⚡",
        }
    }

    pub fn tooltip(self) -> &'static str {
        match self {
            OutlinerFilter::All      => "Show all entities",
            OutlinerFilter::Meshes   => "Only entities with Mesh",
            OutlinerFilter::Lights   => "Only lights",
            OutlinerFilter::Audio    => "Only audio sources",
            OutlinerFilter::Ai       => "Only AI agents / targets",
            OutlinerFilter::Triggers => "Only triggers",
        }
    }

    /// Подходит ли entity под фильтр по компонентам.
    pub fn matches(self, world: &crate::ecs::World, e: crate::ecs::Entity) -> bool {
        use crate::game::ai::{AiAgent, AiTarget, Enemy};
        use crate::game::audio::AudioSource;
        use crate::game::components::{MeshHandle, Trigger};
        use crate::game::lights::{DirectionalLight, PointLight};

        match self {
            OutlinerFilter::All => true,
            OutlinerFilter::Meshes => world.has::<MeshHandle>(e),
            OutlinerFilter::Lights => {
                world.has::<DirectionalLight>(e) || world.has::<PointLight>(e)
            }
            OutlinerFilter::Audio => world.has::<AudioSource>(e),
            OutlinerFilter::Ai => {
                world.has::<AiAgent>(e)
                    || world.has::<AiTarget>(e)
                    || world.has::<Enemy>(e)
            }
            OutlinerFilter::Triggers => world.has::<Trigger>(e),
        }
    }
}