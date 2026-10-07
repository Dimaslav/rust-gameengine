//! Редактор.

pub mod camera_bookmarks;
pub mod content_browser;
pub mod gizmo;
pub mod inspector_audio;
pub mod palette;
pub mod picking;
pub mod placement;
pub mod play;
pub mod settings;
pub mod terrain_tool;   // === TERRAIN ===
pub mod ui;
pub mod undo;

use std::collections::HashSet;
use std::path::PathBuf;

use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::ecs::{Entity, World};
use crate::game::audio::AudioBus;
use crate::game::components::{Parent, Transform};
use crate::render::Material;
use crate::scene::serialize::EntitySnapshot;

use gizmo::GizmoState;
use palette::PaletteState;
use play::PlayState;
use settings::EditorSettings;
use terrain_tool::TerrainTool;   // === TERRAIN ===
use undo::UndoStack;

pub struct Editor {
    pub egui_ctx: egui::Context,
    pub egui_state: EguiWinitState,
    pub egui_renderer: EguiRenderer,
    pub enabled: bool,
    pub state: EditorState,
}

#[derive(Debug, Clone, Copy)]
pub struct BoxSelect {
    pub start: (f32, f32),
    pub current: (f32, f32),
}

pub struct EditorState {
    pub selected: Vec<Entity>,
    pub hierarchy_expanded: HashSet<Entity>,
    pub locked: HashSet<Entity>,
    pub solo: Option<Entity>,
    pub hint: Option<(String, f32)>,

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

    /// === TERRAIN ===
    pub terrain: TerrainTool,

    pub box_select: Option<BoxSelect>,
    pub context_menu_pos: Option<(f32, f32)>,

    pub renaming: Option<Entity>,
    pub rename_buffer: String,

    pub prefabs_dir: String,
    pub prefab_list: Vec<PathBuf>,
    pub prefab_save_name: String,

    pub flying: bool,
    pub fly_speed: f32,
    pub fly_sensitivity: f32,

    pub clipboard_entities: Vec<EntitySnapshot>,

    pub fbx_export_path: String,

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
    LoadTexturesLinear,

    RemoveTexture(String),
    ExportFbxAll,
    ExportFbxSelected,
    ImportFbx,
    LoadPath(String),

    SaveCameraBookmark(usize),
    GotoCameraBookmark(usize),
    CameraPreset(u8),

    DeselectAll,
    InvertSelection,
    SelectAll,
    CleanupEmptyEntities,

    SetBusVolume(AudioBus, f32),
    LoadSound,
    PreviewSound(String),

    BakeNavmesh,

    // === TERRAIN ===
    TerrainRegenerate,
    TerrainSaveFile,   // .r16
    TerrainSavePNG,
    TerrainLoadFile,
}

impl EditorState {
    pub fn new() -> Self {
        let mut s = Self {
            selected: Vec::new(),
            hierarchy_expanded: HashSet::new(),
            locked: HashSet::new(),
            solo: None,
            hint: None,

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

            terrain: TerrainTool::default(),

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

    pub fn collect_settings(
        &self,
        ui_show_renderer: bool,
        ui_show_stats: bool,
        ui_show_hierarchy: bool,
        ui_show_inspector: bool,
        ui_show_audio: bool,
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
        s.show_audio_panel = ui_show_audio;
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
        self.hierarchy_expanded.retain(|&e| world.entities().contains(&e));
        self.locked.retain(|&e| world.entities().contains(&e));
        if let Some(s) = self.solo {
            if !world.entities().contains(&s) {
                self.solo = None;
            }
        }
    }

    pub fn is_locked(&self, world: &World, mut e: Entity) -> bool {
        for _ in 0..64 {
            if self.locked.contains(&e) {
                return true;
            }
            match world.get::<Parent>(e).copied() {
                Some(Parent(p)) => e = p,
                None => return false,
            }
        }
        true
    }

    pub fn hud_hint(&mut self, text: impl Into<String>) {
        self.hint = Some((text.into(), 2.0));
    }

    pub fn tick_hint(&mut self, dt: f32) {
        if let Some((_, t)) = &mut self.hint {
            *t -= dt;
            if *t <= 0.0 {
                self.hint = None;
            }
        }
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
            None,
        );

        let egui_renderer = EguiRenderer::new(device, surface_format, None, 1, false);

        Self {
            egui_ctx,
            egui_state,
            egui_renderer,
            enabled: true,
            state: EditorState::new(),
        }
    }

    pub fn on_window_event(
        &mut self,
        window: &Window,
        event: &winit::event::WindowEvent,
    ) -> bool {
        if !self.enabled {
            return false;
        }
        self.egui_state.on_window_event(window, event).consumed
    }
}