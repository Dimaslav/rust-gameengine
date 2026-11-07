use std::fs;
use std::path::Path;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    
    if args.len() < 3 || args[1] != "new" {
        println!("Использование: cargo run --bin engine-cli -- new <имя_проекта>");
        return;
    }

    let project_name = &args[2];
    let project_dir = Path::new("projects").join(project_name);

    if project_dir.exists() {
        println!("❌ Ошибка: Проект '{}' уже существует!", project_dir.display());
        return;
    }

    println!("🚀 Создание нового проекта: {}", project_name);

    // 1. Создаем структуру папок
    fs::create_dir_all(project_dir.join("src")).unwrap();
    fs::create_dir_all(project_dir.join("assets")).unwrap();
    println!("  ✓ Папки созданы");

    // 2. Создаем project.ron
    let project_ron = format!(
r#"(
    name: "{}",
    engine_version: "0.3.0",
    assets_root: "assets",
    entry_scene: Some("scene.ron"),
)"#, project_name);
    fs::write(project_dir.join("project.ron"), project_ron).unwrap();
    println!("  ✓ project.ron создан");

    // 3. Создаем пустую сцену (scene.ron)
    let scene_ron = 
r#"(
    entities: [
        (
            name: Some("Sun"),
            transform: Some((position: (0.0, 20.0, 0.0), rotation: (0.0, 0.0, 0.0, 1.0), scale: (1.0, 1.0, 1.0))),
            dir_light: Some((direction: (0.5, 1.0, 0.3), color: (1.0, 1.0, 1.0), intensity: 1.2))
        ),
    ],
    player_spawn: Some((0.0, 2.0, 5.0)),
    materials: {},
    texture_paths: {},
    game_state_ron: None,
    terrain: None,
)"#;
    fs::write(project_dir.join("scene.ron"), scene_ron).unwrap();
    println!("  ✓ scene.ron создан");

    // 4. Создаем Cargo.toml для игры
    let cargo_toml = format!(
r#"[package]
name = "{}"
version = "0.1.0"
edition = "2024"

[dependencies]
rust-engine = {{ path = "../../" }}
"#, project_name);
    fs::write(project_dir.join("Cargo.toml"), cargo_toml).unwrap();
    println!("  ✓ Cargo.toml создан");

    // 5. Создаем точку входа main.rs
    let main_rs = format!(
r#"use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;
use rust_engine::engine::Game;
use rust_engine::render::{{Camera3D, PostFx}};

struct MyGame {{
    camera: Camera3D,
}}

impl MyGame {{
    fn new() -> Self {{
        Self {{ camera: Camera3D::new(16.0 / 9.0) }}
    }}
}}

impl Game for MyGame {{
    fn camera(&self) -> &Camera3D {{ &self.camera }}
    fn camera_mut(&mut self) -> &mut Camera3D {{ &mut self.camera }}
    fn postfx(&self) -> PostFx {{ PostFx::default() }}
}}

fn main() {{
    install_crash_handler();
    println!("Запуск игры: {}");
    run(MyGame::new());
}}
"#, project_name);
    fs::write(project_dir.join("src/main.rs"), main_rs).unwrap();
    println!("  ✓ src/main.rs создан");

    println!("\n✅ Готово! Чтобы запустить игру, выполните:");
    println!("   cd projects/{}", project_name);
    println!("   cargo run");
}