use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;
use rust_engine::engine::Game;
use rust_engine::render::{Camera3D, PostFx};

struct MyGame {
    camera: Camera3D,
}

impl MyGame {
    fn new() -> Self {
        Self { camera: Camera3D::new(16.0 / 9.0) }
    }
}

impl Game for MyGame {
    fn camera(&self) -> &Camera3D { &self.camera }
    fn camera_mut(&mut self) -> &mut Camera3D { &mut self.camera }
    fn postfx(&self) -> PostFx { PostFx::default() }
}

fn main() {
    install_crash_handler();
    println!("Запуск игры: my_first_rpg");
    run(MyGame::new());
}
