pub mod camera_bookmarks;
pub mod gizmo;
pub mod hotkeys;
pub mod layout;
pub mod palette;
pub mod panels;
pub mod picking;
pub mod placement;
pub mod play;
pub mod settings;
pub mod terrain_tool;
pub mod theme;
pub mod undo;

use std::collections::{BTreeMap, HashMap, HashSet};
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
use hotkeys::{HotkeyAction, HotkeyMap};
use layout::{LayoutState, OutlinerFilter, Workspace, WorkspaceLayouts};
use palette::PaletteState;
use play::PlayState;
use settings::EditorSettings;
use terrain_tool::TerrainTool;
use theme::Theme;
use undo::UndoStack;

// ============================================================
// ThumbnailCache
// ============================================================

pub enum ThumbnailState {
    Loaded(egui::TextureHandle),
    Failed,
    NoSource,
}

#[derive(Default)]
pub struct ThumbnailCache {
    entries: HashMap<String, ThumbnailState>,
}

impl ThumbnailCache {
    pub fn new() -> Self { Self::default() }

    pub fn get(&self, name: &str) -> Option<&ThumbnailState> {
        self.entries.get(name)
    }

    pub fn insert_loaded(&mut self, name: String, handle: egui::TextureHandle) {
        self.entries.insert(name, ThumbnailState::Loaded(handle));
    }

    pub fn insert_failed(&mut self, name: String) {
        self.entries.insert(name, ThumbnailState::Failed);
    }

    pub fn insert_no_source(&mut self, name: String) {
        self.entries.insert(name, ThumbnailState::NoSource);
    }

    pub fn contains(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    pub fn invalidate(&mut self, name: &str) {
        self.entries.remove(name);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

// ============================================================
// Editor
// ============================================================

pub struct Editor {
    pub egui_ctx: egui::Context,
    pub egui_state: EguiWinitState,
    pub egui_renderer: EguiRenderer,
    pub enabled: bool,
    pub state: EditorState,
}

// ============================================================
// BoxSelect
// ============================================================

#[derive(Debug, Clone, Copy)]
pub struct BoxSelect {
    pub start: (f32, f32),
    pub current: (f32, f32),
}

// ============================================================
// EditorState
// ============================================================

pub struct EditorState {
    pub selected: Vec<Entity>,
    pub hierarchy_expanded: HashSet<Entity>,
    pub locked: HashSet<Entity>,
    pub solo: Option<Entity>,
    pub hint: Option<(String, f32)>,

    pub save_path: String,
    pub pending_action: Option<EditorAction>,

    pub gizmo: GizmoState,
    pub palette: PaletteState,
    pub terrain: TerrainTool,

    pub dirty_materials: Vec<(Entity, String, Material)>,

    pub undo: UndoStack,
    pub undo_requested: bool,
    pub play: PlayState,

    pub search_filter: String,
    pub outliner_filter: OutlinerFilter,
    pub details_filter: String,
    pub clipboard_transform: Option<Transform>,
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

    pub layout: LayoutState,
    pub active_workspace: Workspace,
    pub saved_workspaces: WorkspaceLayouts,

    pub command_palette_open: bool,
    pub command_palette_query: String,
    pub command_palette_selected: usize,

    pub thumbnails: ThumbnailCache,

    pub theme: Theme,
    pub settings_open: bool,

    pub hotkeys: HotkeyMap,
    pub hotkey_capture: Option<HotkeyAction>,
}

// ============================================================
// EditorAction
// ============================================================

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

    TerrainRegenerate,
    TerrainSaveFile,
    TerrainSavePNG,
    TerrainLoadFile,

    ReloadScripts,
    AttachScript { entity: Entity, script: String },
    DetachScript(Entity),
    SelectEntity(Entity),
}

// ============================================================
// EditorState impl
// ============================================================

impl EditorState {
    pub fn new() -> Self {
        let settings = EditorSettings::load("editor.ron");

        let initial_theme = settings.theme;
        let initial_hotkeys = settings.hotkeys.clone();

        let mut saved_workspaces: WorkspaceLayouts = settings.workspace_layouts.clone();

        if saved_workspaces.is_empty() {
            let legacy = LayoutState::from_settings(&settings);
            saved_workspaces.insert(Workspace::Layout, legacy);
        }

        for ws in Workspace::ALL {
            saved_workspaces
                .entry(*ws)
                .or_insert_with(|| LayoutState::preset_for(*ws));
        }

        let active_workspace = settings.active_workspace;
        let layout = saved_workspaces
            .get(&active_workspace)
            .cloned()
            .unwrap_or_else(|| LayoutState::preset_for(active_workspace));

        let mut s = Self {
            selected: Vec::new(),
            hierarchy_expanded: HashSet::new(),
            locked: HashSet::new(),
            solo: None,
            hint: None,

            save_path: "scene.ron".to_string(),
            pending_action: None,

            gizmo: GizmoState::default(),
            palette: PaletteState::default(),
            terrain: TerrainTool::default(),

            dirty_materials: Vec::new(),

            undo: UndoStack::new(),
            undo_requested: false,
            play: PlayState::default(),

            search_filter: String::new(),
            outliner_filter: OutlinerFilter::All,
            details_filter: String::new(),
            clipboard_transform: None,
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

            settings,

            layout,
            active_workspace,
            saved_workspaces,

            command_palette_open: false,
            command_palette_query: String::new(),
            command_palette_selected: 0,

            thumbnails: ThumbnailCache::new(),

            theme: initial_theme,
            settings_open: false,

            hotkeys: initial_hotkeys,
            hotkey_capture: None,
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

    pub fn collect_settings(&self) -> EditorSettings {
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

        let mut layouts: BTreeMap<Workspace, LayoutState> = self.saved_workspaces.clone();
        layouts.insert(self.active_workspace, self.layout.clone());

        s.active_workspace = self.active_workspace;
        s.workspace_layouts = layouts;

        self.layout.write_into_settings(&mut s);

        s.theme = self.theme;
        s.hotkeys = self.hotkeys.clone();

        s
    }

    pub fn switch_workspace(&mut self, new_ws: Workspace) {
        if self.active_workspace == new_ws {
            return;
        }
        self.saved_workspaces
            .insert(self.active_workspace, self.layout.clone());

        let new_layout = self
            .saved_workspaces
            .get(&new_ws)
            .cloned()
            .unwrap_or_else(|| LayoutState::preset_for(new_ws));

        self.layout = new_layout;
        self.active_workspace = new_ws;
    }

    pub fn apply_theme(&self, ctx: &egui::Context) {
        self.theme.apply(ctx);
    }

    // ============================================================
    // Selection
    // ============================================================

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
        if let Some(s) = self.solo
            && !world.entities().contains(&s) {
                self.solo = None;
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

// ============================================================
// Editor impl
// ============================================================

impl Editor {
    pub fn new(
        window: &Window,
        device: &wgpu::Device,
        surface_format: wgpu::TextureFormat,
        viewport_id: egui::ViewportId,
    ) -> Self {
        let egui_ctx = egui::Context::default();
        egui_ctx.set_fonts(egui::FontDefinitions::default());
        egui_ctx.set_visuals(egui::Visuals::dark());

        let egui_state = EguiWinitState::new(
            egui_ctx.clone(),
            viewport_id,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );

        // ИЗМЕНЕНО (egui-wgpu 0.37): `Renderer::new` теперь принимает
        // `RendererOptions` вместо `Option<...> + u32 + bool`.
        let egui_renderer = EguiRenderer::new(
            device,
            surface_format,
            egui_wgpu::RendererOptions::default(),
        );

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