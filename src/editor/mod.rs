//! Редактор.

pub mod camera_bookmarks;
pub mod gizmo;
pub mod palette;
pub mod picking;
pub mod placement;
pub mod play;
pub mod settings;
pub mod ui;
pub mod undo;

use std::path::PathBuf;

use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::ecs::{Entity, World};
use crate::game::components::Transform;
use crate::render::Material;
use crate::scene::serialize::EntitySnapshot;

use gizmo::GizmoState;
use palette::PaletteState;
use play::PlayState;
use settings::EditorSettings;
use undo::UndoStack;

pub struct Editor {
    pub egui_ctx: egui::Context,
    pub egui_state: EguiWinitState,
    pub egui_renderer: EguiRenderer,
    pub enabled: bool,
    pub state: EditorState,
}

/// Активный box-select (ЛКМ-протяжка по viewport).
#[derive(Debug, Clone, Copy)]
pub struct BoxSelect {
    pub start: (f32, f32),
    pub current: (f32, f32),
}

pub struct EditorState {
    pub selected: Vec<Entity>,
    pub save_path: String,
    pub pending_action: Option<EditorAction>,
    pub gizmo: GizmoState,
    pub dirty_materials: Vec<(Entity, String, Material)>,
    pub undo: UndoStack,
    pub undo_requested: bool,
    pub search_filter: String,
    pub clipboard_transform: Option<Transform>,
    pub play: PlayState,
    pub palette: PaletteState,

    pub box_select: Option<BoxSelect>,
    pub context_menu_pos: Option<(f32, f32)>,

    pub renaming: Option<Entity>,
    pub rename_buffer: String,

    // === Prefabs ===
    pub prefabs_dir: String,
    pub prefab_list: Vec<PathBuf>,
    pub prefab_save_name: String,

    pub flying: bool,
    pub fly_speed: f32,
    pub fly_sensitivity: f32,

    pub clipboard_entities: Vec<EntitySnapshot>,

    // === FBX ===
    pub fbx_export_path: String,

    /// Настройки редактора (сохраняются при выходе в `editor.ron`).
    pub settings: EditorSettings,
}

#[derive(Debug, Clone)]
pub enum EditorAction {
    Save,
    Load,
    AddCube,
    AddSphere,
    DeleteSelected,
    Duplicate,
    Undo,
    Redo,
    FocusSelected,
    TogglePlay,
    SpawnPlayerHere,
    CopyEntity,
    PasteEntity,
    MakeMaterialUnique,
    PlacePalette,
    ClearPalette,
    NewScene,
    SavePrefab,
    RefreshPrefabs,
    InstantiatePrefab(u32),
    LoadTextures,
    RemoveTexture(String),
    ExportFbxAll,
    ExportFbxSelected,
    ImportFbx,
    /// Загрузить конкретный путь (из Recent Files).
    LoadPath(String),

    // === Команды из Command Palette ===
    SaveCameraBookmark(usize),
    GotoCameraBookmark(usize),
    /// 0=front, 1=back, 2=right, 3=left, 4=top, 5=bottom, 6=iso.
    CameraPreset(u8),

    // === Дополнительные действия ===
    DeselectAll,
    InvertSelection,
    SelectAll,
    CleanupEmptyEntities,
}

impl EditorState {
    pub fn new() -> Self {
        let mut s = Self {
            selected: Vec::new(),
            save_path: "scene.ron".to_string(),
            pending_action: None,
            gizmo: GizmoState::default(),
            dirty_materials: Vec::new(),
            undo: UndoStack::new(),
            undo_requested: false,
            search_filter: String::new(),
            clipboard_transform: None,
            play: PlayState::default(),
            palette: PaletteState::default(),
            box_select: None,
            context_menu_pos: None,
            renaming: None,
            rename_buffer: String::new(),
            prefabs_dir: "prefabs".to_string(),
            prefab_list: Vec::new(),
            prefab_save_name: String::new(),
            flying: false,
            fly_speed: 15.0,
            fly_sensitivity: 0.0025,
            clipboard_entities: Vec::new(),
            fbx_export_path: "scene.fbx".to_string(),
            settings: EditorSettings::load("editor.ron"),
        };

        // Синхронизируем поля из настроек.
        s.save_path = s.settings.save_path.clone();
        s.prefabs_dir = s.settings.prefabs_dir.clone();
        s.fbx_export_path = s.settings.fbx_export_path.clone();
        s.fly_speed = s.settings.fly_speed;
        s.fly_sensitivity = s.settings.fly_sensitivity;
        s.gizmo.snap_enabled = s.settings.gizmo_snap;
        s.palette.keep_active = s.settings.palette_keep_active;
        s.palette.snap_to_grid = s.settings.palette_snap_to_grid;
        s.palette.grid_step = s.settings.palette_grid_step;

        s.prefab_list = crate::scene::prefab::list_prefabs(&s.prefabs_dir);
        s
    }

    /// Собрать актуальные настройки из текущего состояния.
    pub fn collect_settings(
        &self,
        ui_show_renderer: bool,
        ui_show_stats: bool,
        ui_show_hierarchy: bool,
        ui_show_inspector: bool,
        left_panel_width: f32,
        right_panel_width: f32,
    ) -> EditorSettings {
        let mut s = self.settings.clone();
        s.save_path = self.save_path.clone();
        s.prefabs_dir = self.prefabs_dir.clone();
        s.fbx_export_path = self.fbx_export_path.clone();
        s.fly_speed = self.fly_speed;
        s.fly_sensitivity = self.fly_sensitivity;
        s.gizmo_snap = self.gizmo.snap_enabled;
        s.palette_keep_active = self.palette.keep_active;
        s.palette_snap_to_grid = self.palette.snap_to_grid;
        s.palette_grid_step = self.palette.grid_step;

        s.show_renderer_panel = ui_show_renderer;
        s.show_stats_panel = ui_show_stats;
        s.show_hierarchy_panel = ui_show_hierarchy;
        s.show_inspector_panel = ui_show_inspector;
        s.left_panel_width = left_panel_width;
        s.right_panel_width = right_panel_width;
        s
    }

    pub fn is_selected(&self, e: Entity) -> bool {
        self.selected.contains(&e)
    }

    pub fn select_single(&mut self, e: Entity) {
        self.selected.clear();
        self.selected.push(e);
    }

    pub fn toggle_select(&mut self, e: Entity) {
        if let Some(i) = self.selected.iter().position(|&x| x == e) {
            self.selected.remove(i);
        } else {
            self.selected.push(e);
        }
    }

    pub fn select_range(&mut self, list: &[Entity], anchor: Entity, e: Entity) {
        let (Some(a), Some(b)) = (
            list.iter().position(|&x| x == anchor),
            list.iter().position(|&x| x == e),
        ) else {
            self.select_single(e);
            return;
        };
        let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
        self.selected.clear();
        for &ent in &list[lo..=hi] {
            self.selected.push(ent);
        }
    }

    pub fn primary(&self) -> Option<Entity> {
        self.selected.last().copied()
    }

    pub fn prune_selection(&mut self, world: &World) {
        self.selected.retain(|&e| world.entities().contains(&e));
    }
}

impl Default for EditorState {
    fn default() -> Self {
        Self::new()
    }
}

impl Editor {
    pub fn new(
        window: &Window,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        viewport_id: egui::ViewportId,
    ) -> Self {
        let egui_ctx = egui::Context::default();
        egui_ctx.set_visuals(egui::Visuals::dark());

        let egui_state = EguiWinitState::new(
            egui_ctx.clone(),
            viewport_id,
            window,
            Some(window.scale_factor() as f32),
            None,
        );
        // egui-wgpu 0.28.1: new(device, format, depth_format, msaa_samples).
        let egui_renderer = EguiRenderer::new(device, surface_format, None, 1);

        Self {
            egui_ctx,
            egui_state,
            egui_renderer,
            enabled: true,
            state: EditorState::new(),
        }
    }

    pub fn on_window_event(&mut self, window: &Window, event: &winit::event::WindowEvent) -> bool {
        if !self.enabled {
            return false;
        }
        self.egui_state.on_window_event(window, event).consumed
    }
}