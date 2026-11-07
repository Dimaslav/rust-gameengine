//! `engine-editor` — full engine editor with egui panels.

use rust_engine::engine::crash::install_crash_handler;
use rust_engine::engine::run;
use rust_engine::engine::{Game, Input};
use rust_engine::render::{
    Camera3D, PostFx, Renderer, MeshDraw, GpuLight, GpuPointLight, InstanceData, Mesh, Material, LineVertex,
};
use rust_engine::project::ProjectManifest;
use rust_engine::ecs::{World, Entity};
use rust_engine::game::components::{Transform, MeshHandle, MaterialHandle, Tint, Name};
use rust_engine::game::lights::{DirectionalLight, PointLight};
use rust_engine::assets::AssetDatabase;
use std::collections::HashMap;
use std::path::Path;
use glam::Vec3;
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

struct EditorBaseScene {
    camera: Camera3D,
    dragging: bool,
}

impl EditorBaseScene {
    fn new() -> Self {
        let mut camera = Camera3D::new(16.0 / 9.0);
        camera.distance = 15.0;
        camera.yaw = 0.8;
        camera.pitch = 0.5;
        Self { camera, dragging: false }
    }
}

impl Game for EditorBaseScene {
    fn init_with_assets(&mut self, world: &mut World, renderer: &mut Renderer, _assets: &AssetDatabase) {
        renderer.add_mesh("cube", Mesh::cube(&renderer.device, 1.0));
        renderer.add_mesh("sphere", Mesh::sphere(&renderer.device, 0.5, 16, 24));
        renderer.add_mesh("cylinder", Mesh::cylinder(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("plane", Mesh::plane(&renderer.device, 1.0, 1));

        renderer.add_material("arena_floor", Material::new([0.42, 0.46, 0.50, 1.0]).with_metallic_roughness(0.0, 0.92));
        renderer.add_material("gold", Material::new([1.0, 0.85, 0.3, 1.0]).with_metallic_roughness(1.0, 0.25));
        renderer.add_material("flat_blue", Material::new([0.3, 0.5, 1.0, 1.0]).with_metallic_roughness(0.0, 0.8));
        renderer.add_material("emissive_warm", Material::new([1.0, 0.7, 0.4, 1.0]).with_emissive([3.0, 1.5, 0.4]));
        renderer.add_material("default", Material::new([0.8, 0.8, 0.8, 1.0]).with_metallic_roughness(0.0, 0.8));

        if Path::new("scene.ron").exists() {
            if let Ok((loaded_world, _spawn)) = rust_engine::scene::load_scene_with_assets_from_file("scene.ron", renderer) {
                *world = loaded_world;
            }
        } else {
            let sun = world.spawn();
            world.insert(sun, Name("Sun".into()));
            world.insert(sun, Transform::at(Vec3::new(0.0, 40.0, 0.0)));
            world.insert(sun, DirectionalLight::sun());
        }
    }

    fn update(&mut self, _world: &mut World, input: &Input, _renderer: &mut Renderer, dt: f32) -> bool {
        if !input.play_mode && !input.editor_flying && !input.editor_captured {
            if input.mouse_down(MouseButton::Left) {
                let (dx, dy) = input.mouse_delta;
                if self.dragging { self.camera.orbit(dx * 0.005, dy * 0.005); }
                self.dragging = true;
            } else { self.dragging = false; }
            if input.scroll_delta.abs() > 0.01 { self.camera.zoom(input.scroll_delta * 0.05); }
        }

        if input.editor_flying {
            let speed = 15.0 * dt;
            let fwd = self.camera.forward();
            let right = self.camera.right();
            let mut delta = Vec3::ZERO;
            if input.key_down(KeyCode::KeyW) { delta += fwd; }
            if input.key_down(KeyCode::KeyS) { delta -= fwd; }
            if input.key_down(KeyCode::KeyD) { delta += right; }
            if input.key_down(KeyCode::KeyA) { delta -= right; }
            if input.key_down(KeyCode::Space) || input.key_down(KeyCode::KeyE) { delta += Vec3::Y; }
            if input.key_down(KeyCode::KeyQ) { delta -= Vec3::Y; }
            if delta.length_squared() > 1e-6 { self.camera.fly_move(delta.normalize() * speed); }
        }
        true
    }

    fn collect_draws(&mut self, world: &mut World, renderer: &Renderer) -> Vec<MeshDraw> {
        type BucketKey = (String, String, [u32; 4], bool, bool);
        let mut buckets: HashMap<BucketKey, Vec<InstanceData>> = HashMap::new();
        for (e, _) in world.query::<Transform>() {
            let Some(mesh_handle) = world.get::<MeshHandle>(e) else { continue };
            let Some(mat_handle) = world.get::<MaterialHandle>(e) else { continue };
            let model = rust_engine::game::world_matrix(world, e);
            let color = world.get::<Tint>(e).map(|t| t.0).unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let material = renderer.materials.get(&mat_handle.0).unwrap_or_else(|| renderer.materials_default());
            let blend = material.alpha_mode == rust_engine::render::AlphaMode::Blend;
            let double_sided = material.double_sided;
            let inst = InstanceData::new_full(model, model, color, [1.0, 1.0, 1.0]);
            let key = (mesh_handle.0.clone(), mat_handle.0.clone(), color.map(f32::to_bits), blend, double_sided);
            buckets.entry(key).or_default().push(inst);
        }
        buckets.into_iter().map(|((mesh, mat, _, blend, double_sided), instances)| {
            MeshDraw { mesh, instances, texture: Some(mat), blend, double_sided }
        }).collect()
    }

    fn collect_lines(&mut self, _world: &mut World, _renderer: &Renderer, _selected: &[Entity]) -> Vec<LineVertex> {
        Vec::new() // Сетка теперь рисуется движком
    }

    fn dir_lights(&self, world: &World) -> Vec<GpuLight> {
        world.query::<DirectionalLight>().map(|(_, l)| GpuLight {
            direction: [l.direction.x, l.direction.y, l.direction.z, l.intensity],
            color: [l.color[0], l.color[1], l.color[2], 0.0],
        }).take(4).collect()
    }

    fn point_lights(&self, world: &World) -> Vec<GpuPointLight> {
        world.query::<PointLight>().filter_map(|(e, l)| {
            let t = world.get::<Transform>(e)?;
            Some(GpuPointLight {
                position: [t.position.x, t.position.y, t.position.z, l.range],
                color: [l.color[0], l.color[1], l.color[2], l.intensity],
            })
        }).take(16).collect()
    }

    fn camera(&self) -> &Camera3D { &self.camera }
    fn camera_mut(&mut self) -> &mut Camera3D { &mut self.camera }
    fn postfx(&self) -> PostFx { PostFx::default() }
}

fn main() {
    install_crash_handler();
    let args: Vec<String> = std::env::args().collect();
    let mut project_path = None;
    for i in 0..args.len() {
        if args[i] == "--project" && i + 1 < args.len() { project_path = Some(args[i + 1].clone()); }
    }

    if let Some(project_dir) = project_path {
        let path = Path::new(&project_dir);
        let manifest_path = path.join("project.ron");
        if !manifest_path.exists() { std::process::exit(1); }
        if let Ok(manifest) = ProjectManifest::load(&manifest_path) {
            log::info!("Loading project: {}", manifest.name);
            let _ = std::env::set_current_dir(path);
        }
    }
    run(EditorBaseScene::new());
}