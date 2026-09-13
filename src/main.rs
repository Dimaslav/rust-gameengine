#![allow(dead_code, unused_imports)]

mod ecs;
mod engine;
mod game;
mod render;

use ecs::{System, World};
use engine::{run, Game, Input};
use game::components::{MaterialHandle, MeshHandle, Spinner, Transform, Velocity};
use glam::{Quat, Vec3};
use render::{
    Camera3D, DebugView, GltfInstance, GpuLight, GpuPointLight, InstanceData, LineBatch,
    LineVertex, Material, Mesh, MeshDraw, PostFx, Renderer,
};

// ============================================================
// Frustum culling
// ============================================================

fn sphere_in_frustum(center: Vec3, radius: f32, planes: &[glam::Vec4; 6]) -> bool {
    for p in planes {
        let dist = p.x * center.x + p.y * center.y + p.z * center.z + p.w;
        if dist < -radius {
            return false;
        }
    }
    true
}

// ============================================================
// Системы
// ============================================================

struct RotationSystem;

impl System for RotationSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        let entities: Vec<_> = world.query::<Spinner>().map(|(e, _)| e).collect();
        for e in entities {
            let spinner = world.get::<Spinner>(e).copied();
            if let (Some(s), Some(t)) = (spinner, world.get_mut::<Transform>(e)) {
                let dq = Quat::from_axis_angle(s.axis.normalize_or_zero(), s.speed * dt);
                t.rotation = dq * t.rotation;
            }
        }
    }
}

struct MovementSystem;

impl System for MovementSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        world.for_each_pair::<Transform, Velocity, _>(|_e, t, v| {
            t.position += v.value * dt;
        });
    }
}

// ============================================================
// Игра
// ============================================================

struct DemoGame {
    camera: Camera3D,
    systems: Vec<Box<dyn System>>,
    dir_lights: Vec<GpuLight>,
    point_lights: Vec<GpuPointLight>,
    postfx: PostFx,
    spawned: bool,
    dragging: bool,
    show_grid: bool,
    show_culling: bool,
    orbit_phase: f32,
    gltf_instances: Vec<GltfInstance>,
}

impl DemoGame {
    fn new() -> Self {
        // Directional: intensity в .w (2.5 — усилено, чтобы конкурировать с IBL).
        let dir_lights = vec![
            GpuLight {
                direction: [0.4, 1.0, 0.3, 2.5],
                color: [1.0, 0.98, 0.9, 0.0],
            },
            GpuLight {
                direction: [-0.6, 0.3, -0.7, 0.4],
                color: [1.0, 0.5, 0.3, 0.0],
            },
        ];

        // Point-lights: intensity в .a (12/10/10 — усилено).
        let point_lights = vec![
            GpuPointLight {
                position: [0.0, 3.0, 0.0, 18.0],
                color: [1.0, 0.4, 0.2, 12.0],
            },
            GpuPointLight {
                position: [10.0, 4.0, 10.0, 14.0],
                color: [0.2, 0.6, 1.0, 10.0],
            },
            GpuPointLight {
                position: [-10.0, 4.0, -10.0, 14.0],
                color: [0.4, 1.0, 0.4, 10.0],
            },
        ];

        Self {
            camera: Camera3D::new(16.0 / 9.0),
            systems: vec![Box::new(RotationSystem), Box::new(MovementSystem)],
            dir_lights,
            point_lights,
            postfx: PostFx {
                bloom_threshold: 1.5,
                bloom_strength: 0.4,
                exposure: 1.1,
                ssao_strength: 0.8,
                ssao_radius: 0.6,
                debug_view: DebugView::Final,
            },
            spawned: false,
            dragging: false,
            show_grid: true,
            show_culling: true,
            orbit_phase: 0.0,
            gltf_instances: Vec::new(),
        }
    }
}

impl Game for DemoGame {
    fn init(&mut self, _world: &mut World, renderer: &mut Renderer) {
        renderer.add_mesh("cube", Mesh::cube(&renderer.device, 1.0));
        renderer.add_mesh("sphere", Mesh::sphere(&renderer.device, 0.5, 16, 24));
        renderer.add_mesh("ground", Mesh::plane(&renderer.device, 200.0, 1));

        // Шахматка
        let mut data = vec![0u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let idx = (y * 64 + x) * 4;
                let c = if ((x / 8) + (y / 8)) % 2 == 0 { 220 } else { 60 };
                data[idx] = c;
                data[idx + 1] = c;
                data[idx + 2] = c;
                data[idx + 3] = 255;
            }
        }
        renderer
            .load_texture_rgba("checker", &data, 64, 64)
            .expect("checker");

        // PBR-материалы
        renderer.add_material(
            "ground",
            Material::new([0.55, 0.60, 0.55, 1.0]).with_metallic_roughness(0.0, 0.85),
        );
        renderer.add_material(
            "checker_red",
            Material::new([1.0, 0.35, 0.35, 1.0])
                .with_texture("checker")
                .with_metallic_roughness(0.0, 0.5),
        );
        renderer.add_material(
            "checker_blue",
            Material::new([0.35, 0.55, 1.0, 1.0])
                .with_texture("checker")
                .with_metallic_roughness(0.0, 0.5),
        );
        renderer.add_material(
            "gold",
            Material::new([1.0, 0.85, 0.3, 1.0]).with_metallic_roughness(1.0, 0.25),
        );
        renderer.add_material(
            "emissive",
            Material::new([1.0, 1.0, 1.0, 1.0])
                .with_metallic_roughness(0.0, 0.5)
                .with_emissive([2.5, 2.2, 0.6]),
        );

        // glTF (опционально)
        match render::load_gltf_into(renderer, "assets/model.glb", "model") {
            Ok(list) => {
                println!("Loaded glTF: {} primitives", list.len());
                self.gltf_instances = list;
            }
            Err(e) => {
                eprintln!("glTF not loaded ({}). Using only procedural meshes.", e);
            }
        }
    }

    fn update(&mut self, world: &mut World, input: &Input, dt: f32) {
        use winit::keyboard::KeyCode;

        if input.key_pressed(KeyCode::Escape) {
            std::process::exit(0);
        }
        if input.key_pressed(KeyCode::KeyG) {
            self.show_grid = !self.show_grid;
        }
        if input.key_pressed(KeyCode::KeyC) {
            self.show_culling = !self.show_culling;
        }

        // Debug views: F1–F6
        if input.key_pressed(KeyCode::F1) {
            self.postfx.debug_view = DebugView::Final;
        }
        if input.key_pressed(KeyCode::F2) {
            self.postfx.debug_view = DebugView::Ssao;
        }
        if input.key_pressed(KeyCode::F3) {
            self.postfx.debug_view = DebugView::GbufferNormal;
        }
        if input.key_pressed(KeyCode::F4) {
            self.postfx.debug_view = DebugView::GbufferDepth;
        }
        if input.key_pressed(KeyCode::F5) {
            self.postfx.debug_view = DebugView::HdrPreBloom;
        }
        if input.key_pressed(KeyCode::F6) {
            self.postfx.debug_view = DebugView::CsmCascade0;
        }

        // Post-FX
        if input.key_down(KeyCode::BracketLeft) {
            self.postfx.bloom_threshold = (self.postfx.bloom_threshold - dt * 0.5).max(0.1);
        }
        if input.key_down(KeyCode::BracketRight) {
            self.postfx.bloom_threshold += dt * 0.5;
        }
        if input.key_down(KeyCode::Comma) {
            self.postfx.bloom_strength = (self.postfx.bloom_strength - dt * 0.5).max(0.0);
        }
        if input.key_down(KeyCode::Period) {
            self.postfx.bloom_strength = (self.postfx.bloom_strength + dt * 0.5).min(3.0);
        }
        if input.key_down(KeyCode::Semicolon) {
            self.postfx.exposure = (self.postfx.exposure - dt * 0.5).max(0.1);
        }
        if input.key_down(KeyCode::Quote) {
            self.postfx.exposure = (self.postfx.exposure + dt * 0.5).min(3.0);
        }
        if input.key_down(KeyCode::KeyZ) {
            self.postfx.ssao_strength = (self.postfx.ssao_strength - dt * 0.5).max(0.0);
        }
        if input.key_down(KeyCode::KeyX) {
            self.postfx.ssao_strength = (self.postfx.ssao_strength + dt * 0.5).min(2.0);
        }

        // Орбита
        let lmb = input.mouse_down(winit::event::MouseButton::Left);
        if lmb {
            let (dx, dy) = input.mouse_delta;
            if self.dragging {
                self.camera.orbit(dx * 0.005, dy * 0.005);
            }
            self.dragging = true;
        } else {
            self.dragging = false;
        }

        if input.scroll_delta.abs() > 0.01 {
            self.camera.zoom(input.scroll_delta * 0.05);
        }

        // Панорама
        let speed = 8.0 * dt;
        let mut pan = (0.0, 0.0);
        if input.key_down(KeyCode::KeyW) { pan.1 -= speed; }
        if input.key_down(KeyCode::KeyS) { pan.1 += speed; }
        if input.key_down(KeyCode::KeyA) { pan.0 -= speed; }
        if input.key_down(KeyCode::KeyD) { pan.0 += speed; }
        if pan != (0.0, 0.0) {
            self.camera.pan(pan.0, pan.1);
        }

        // Анимация point-lights
        self.orbit_phase += dt * 0.5;
        let (sp, cp) = self.orbit_phase.sin_cos();
        self.point_lights[1].position[0] = cp * 12.0;
        self.point_lights[1].position[2] = sp * 12.0;
        self.point_lights[2].position[0] = -cp * 12.0;
        self.point_lights[2].position[2] = -sp * 12.0;

        // Спавн сцены один раз
        if !self.spawned {
            self.spawned = true;

            let ground = world.spawn();
            world.insert(ground, Transform::new(0.0, -1.0, 0.0));
            world.insert(ground, MeshHandle("ground".into()));
            world.insert(ground, MaterialHandle("ground".into()));

            for i in 0..2000 {
                let t = i as f32 / 2000.0;
                let angle = t * std::f32::consts::TAU * 20.0;
                let radius = 3.0 + t * 30.0;
                let y = (t * 8.0).sin() * 2.0;
                let e = world.spawn();
                world.insert(
                    e,
                    Transform::new(angle.cos() * radius, y + 1.0, angle.sin() * radius)
                        .with_rotation(Quat::from_axis_angle(Vec3::Y, angle))
                        .with_scale(0.6 + t * 0.4),
                );
                world.insert(e, MeshHandle("cube".into()));
                let mat = match i % 3 {
                    0 => "checker_red",
                    1 => "checker_blue",
                    _ => "gold",
                };
                world.insert(e, MaterialHandle(mat.into()));
                if i % 10 == 0 {
                    world.insert(e, Spinner::new(Vec3::new(0.2, 1.0, 0.3), 1.0 + t * 3.0));
                }
            }

            for i in 0..20 {
                let angle = i as f32 / 20.0 * std::f32::consts::TAU;
                let e = world.spawn();
                world.insert(
                    e,
                    Transform::new(
                        angle.cos() * 12.0,
                        6.0 + (i as f32 * 0.3).sin(),
                        angle.sin() * 12.0,
                    ),
                );
                world.insert(e, MeshHandle("sphere".into()));
                let mat = if i % 2 == 0 { "emissive" } else { "gold" };
                world.insert(e, MaterialHandle(mat.into()));
                world.insert(e, Velocity::new(angle.cos() * 0.4, 0.2, angle.sin() * 0.4));
            }

            for i in 0..3 {
                let pos = [
                    self.point_lights[i].position[0],
                    self.point_lights[i].position[1],
                    self.point_lights[i].position[2],
                ];
                let e = world.spawn();
                world.insert(e, Transform::new(pos[0], pos[1], pos[2]).with_scale(0.3));
                world.insert(e, MeshHandle("sphere".into()));
                world.insert(e, MaterialHandle("emissive".into()));
            }

            for inst in &self.gltf_instances {
                let e = world.spawn();
                let (scale, rot, trans) = inst.model.to_scale_rotation_translation();
                world.insert(
                    e,
                    Transform::at(trans).with_rotation(rot).with_scale(scale.x),
                );
                world.insert(e, MeshHandle(inst.mesh_name.clone()));
                world.insert(e, MaterialHandle(inst.material_name.clone()));
            }

            println!("Spawned demo scene: 2000 cubes + 20 spheres + 3 bulbs + ground");
        }

        for sys in self.systems.iter_mut() {
            sys.update(world, dt);
        }
    }

    fn collect_draws(&mut self, world: &mut World, renderer: &Renderer) -> Vec<MeshDraw> {
        use std::collections::HashMap;
        let mut buckets: HashMap<(String, String, [u32; 4]), Vec<InstanceData>> = HashMap::new();

        let planes = self.camera.frustum_planes();

        let entities: Vec<_> = world.entities().to_vec();
        for e in entities {
            let (Some(t), Some(m), Some(mat)) = (
                world.get::<Transform>(e),
                world.get::<MeshHandle>(e),
                world.get::<MaterialHandle>(e),
            ) else {
                continue;
            };

            if self.show_culling {
                if let Some(mesh) = renderer.meshes.get(&m.0) {
                    let model = t.matrix();
                    let (center, radius) = mesh.world_bounds(&model);
                    if !sphere_in_frustum(center, radius, &planes) {
                        continue;
                    }
                }
            }

            let material = renderer
                .materials
                .get(&mat.0)
                .unwrap_or_else(|| renderer.materials_default());

            let key = (
                m.0.clone(),
                mat.0.clone(),
                material.base_color.map(f32::to_bits),
            );
            let inst = InstanceData::new(t.matrix(), material.base_color);
            buckets.entry(key).or_default().push(inst);
        }

        buckets
            .into_iter()
            .map(|((mesh, material_name, _), instances)| MeshDraw {
                mesh,
                instances,
                texture: Some(material_name),
            })
            .collect()
    }

    fn collect_lines(&mut self, _world: &mut World, _renderer: &Renderer) -> Vec<LineVertex> {
        if !self.show_grid {
            return Vec::new();
        }
        let mut batch = LineBatch::new();
        batch.grid(
            100.0,
            2.0,
            [0.15, 0.18, 0.22, 1.0],
            [0.35, 0.40, 0.48, 1.0],
            5,
        );
        batch.axes(5.0);
        batch.vertices().to_vec()
    }

    fn dir_lights(&self) -> Vec<GpuLight> {
        self.dir_lights.clone()
    }

    fn point_lights(&self) -> Vec<GpuPointLight> {
        self.point_lights.clone()
    }

    fn ambient(&self) -> [f32; 3] {
        // Не используется — IBL заменяет ambient.
        [0.15, 0.17, 0.22]
    }

    fn postfx(&self) -> PostFx {
        self.postfx
    }

    fn camera(&self) -> &Camera3D {
        &self.camera
    }
    fn camera_mut(&mut self) -> &mut Camera3D {
        &mut self.camera
    }
}

fn main() {
    run(DemoGame::new());
}