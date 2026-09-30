//! Редактор: egui-оверлей + панели + gizmo + undo/redo + Play + Fly.

pub mod gizmo;
pub mod picking;
pub mod play;
pub mod ui;
pub mod undo;

use egui_wgpu::Renderer as EguiRenderer;
use egui_winit::State as EguiWinitState;
use winit::window::Window;

use crate::ecs::{Entity, World};
use crate::game::components::Transform;
use crate::render::Material;

use gizmo::GizmoState;
use play::PlayState;
use undo::UndoStack;

pub struct Editor {
    pub egui_ctx: egui::Context,
    pub egui_state: EguiWinitState,
    pub egui_renderer: EguiRenderer,
    pub enabled: bool,
    pub state: EditorState,
}

pub struct EditorState {
    pub selected: Vec<Entity>,
    pub save_path: String,
    pub pending_action: Option<EditorAction>,
    pub gizmo: GizmoState,
    pub dirty_materials: Vec<(String, Material)>,
    pub undo: UndoStack,
    pub undo_requested: bool,
    pub search_filter: String,
    pub clipboard_transform: Option<Transform>,
    pub play: PlayState,

    /// Зажат RMB — летим по миру (UE5-подобно).
    pub flying: bool,
    /// Скорость полёта в м/с (с учётом Shift/Ctrl).
    pub fly_speed: f32,
    /// Чувствительность мыши в fly-режиме.
    pub fly_sensitivity: f32,
}

#[derive(Debug, Clone, Copy)]
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
}

impl EditorState {
    pub fn new() -> Self {
        Self {
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
            flying: false,
            fly_speed: 15.0,
            fly_sensitivity: 0.0025,
        }
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

        let egui_renderer = EguiRenderer::new(device, surface_format, None, 1);

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
        let response = self.egui_state.on_window_event(window, event);
        response.consumed
    }
}