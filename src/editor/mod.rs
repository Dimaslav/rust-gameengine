//! Редактор.

pub mod gizmo;
pub mod palette;
pub mod picking;
pub mod placement;
pub mod play;
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
use undo::UndoStack;

pub struct Editor {
    pub egui_ctx: egui::Context,
    pub egui_state: EguiWinitState,
    pub egui_renderer: EguiRenderer,
    pub enabled: bool,
    pub state: EditorState,
}

/// Активный box-select (ЛКМ-протяжка по viewport).
/// Координаты в физических пикселях (как `Input::mouse_pos`).
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
    /// Правки материалов: (entity-владелец, имя материала, новое значение).
    /// Entity нужен, чтобы сделать материал unique, если он shared.
    pub dirty_materials: Vec<(Entity, String, Material)>,
    pub undo: UndoStack,
    pub undo_requested: bool,
    pub search_filter: String,
    pub clipboard_transform: Option<Transform>,
    pub play: PlayState,
    pub palette: PaletteState,

    /// ЛКМ-протяжка по viewport.
    pub box_select: Option<BoxSelect>,
    /// Позиция открытого контекстного меню (physical pixels).
    pub context_menu_pos: Option<(f32, f32)>,

    /// Entity, для которой сейчас идёт inline-переименование в Hierarchy.
    pub renaming: Option<Entity>,
    /// Буфер для inline-редактирования имени.
    pub rename_buffer: String,

    // === Prefabs ===
    /// Директория с файлами `.prefab.ron`.
    pub prefabs_dir: String,
    /// Кэш-список файлов.
    pub prefab_list: Vec<PathBuf>,
    /// Имя для сохранения текущего выделения.
    pub prefab_save_name: String,

    pub flying: bool,
    pub fly_speed: f32,
    pub fly_sensitivity: f32,

    pub clipboard_entities: Vec<EntitySnapshot>,

    // === FBX ===
    /// Путь/имя по умолчанию для экспорта FBX (кэш последнего выбора).
    pub fbx_export_path: String,
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
    /// Спавн палитра-кистью в конкретной точке.
    PlacePalette,
    /// Сбросить активную кисть.
    ClearPalette,
    /// Новый пустой мир (сброс сцены).
    NewScene,
    /// Сохранить выделение как prefab.
    SavePrefab,
    /// Перечитать список файлов из `prefabs_dir`.
    RefreshPrefabs,
    /// Спавн инстанса префаба по индексу в `prefab_list`.
    InstantiatePrefab(u32),
    /// Открыть диалог выбора файлов и загрузить текстуры.
    LoadTextures,
    /// Удалить текстуру из реестра по имени.
    RemoveTexture(String),

    // === FBX ===
    /// Экспортировать всю сцену в `.fbx`.
    ExportFbxAll,
    /// Экспортировать только выделение.
    ExportFbxSelected,
    /// Импортировать ASCII FBX (Blender/Maya/Unity).
    ImportFbx,
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
        };
        s.prefab_list = crate::scene::prefab::list_prefabs(&s.prefabs_dir);
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