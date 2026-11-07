//! `engine-editor` — full engine editor with egui panels.
//!
//! This binary runs a minimal empty scene so the editor can start
//! without depending on any specific game project.

use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;
use rust_engine::engine::Game;
use rust_engine::ecs::World;
use rust_engine::render::{Camera3D, PostFx, Renderer};

// Пустая игра-заглушка для редактора, чтобы он мог запуститься
struct EditorBaseScene {
    camera: Camera3D,
}

impl EditorBaseScene {
    fn new() -> Self {
        Self { camera: Camera3D::new(16.0 / 9.0) }
    }
}

impl Game for EditorBaseScene {
    fn camera(&self) -> &Camera3D { &self.camera }
    fn camera_mut(&mut self) -> &mut Camera3D { &mut self.camera }
    fn postfx(&self) -> PostFx { PostFx::default() }
}

fn main() {
    install_crash_handler();
    run(EditorBaseScene::new());
}