#![allow(dead_code, unused_imports)]

mod ecs;
mod editor;
mod engine;
mod game;
mod physics;
mod render;
mod scene;

use std::collections::HashMap;

use ecs::{Entity, System, World};
use engine::{run, Game, Input};
use game::components::{
    AnimationPlayer, Chase, Health, Interactable, MaterialHandle, MeshHandle, Name, Parent,
    SkeletonHandle, Spinner, Tint, Transform, Trigger, TriggerAction, Velocity, Visible,
};
use glam::{Quat, Vec3};
use physics::{Collider, PhysicsMaterial, RigidBody};
use render::{
    skinning::AnimationClip, AlphaMode, Camera3D, DebugView, GltfInstance, GpuLight,
    GpuPointLight, InstanceData, LineBatch, LineVertex, Material, Mesh, MeshDraw, PostFx,
    Renderer, Skeleton,
};

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
                let axis = s.axis.normalize_or_zero();
                if axis.length_squared() < 1e-6 {
                    continue;
                }
                let dq = Quat::from_axis_angle(axis, s.speed * dt);
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
    skeletons: HashMap<String, Skeleton>,
    animations: HashMap<String, AnimationClip>,

    /// Сколько объектов на каждом LOD-уровне в последнем кадре.
    lod_stats: [usize; 4],
}

impl DemoGame {
    fn new() -> Self {
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
                bloom_threshold: 1.2,
                bloom_strength: 0.6,
                bloom_knee: 0.5,
                bloom_radius: 1.0,
                exposure: 1.0,
                ssao_strength: 0.8,
                ssao_radius: 0.6,
                ibl_strength: 0.35,
                debug_view: DebugView::Final,
                fxaa_strength: 1.0,
                fog_color: [0.55, 0.62, 0.72],
                fog_density: 0.0,
                fog_height_base: 0.0,
                fog_height_falloff: 0.05,
                vignette_strength: 0.0,
                film_grain: 0.0,
                chromatic_aberration: 0.0,
                shadow_bias: 0.0015,
                shadow_normal_bias: 3.0,
                shadow_fade_start: 150.0,
                shadow_fade_end: 200.0,
                lod_bias: 1.0,
                lod_distances: [30.0, 80.0, 200.0, 500.0],
            },
            spawned: false,
            dragging: false,
            show_grid: true,
            show_culling: true,
            orbit_phase: 0.0,
            gltf_instances: Vec::new(),
            skeletons: HashMap::new(),
            animations: HashMap::new(),
            lod_stats: [0; 4],
        }
    }
}

impl Game for DemoGame {
    fn init(&mut self, _world: &mut World, renderer: &mut Renderer) {
        renderer.add_mesh("cube", Mesh::cube(&renderer.device, 1.0));
        renderer.add_mesh("sphere", Mesh::sphere(&renderer.device, 0.5, 16, 24));
        renderer.add_mesh("ground", Mesh::plane(&renderer.device, 200.0, 1));
        renderer.add_mesh("quad", Mesh::plane(&renderer.device, 2.0, 1));

        renderer.add_mesh("cylinder", Mesh::cylinder(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("cone",     Mesh::cone(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("capsule",  Mesh::capsule(&renderer.device, 0.4, 0.8, 6, 20));

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
            "flat_red",
            Material::new([1.0, 0.35, 0.35, 1.0]).with_metallic_roughness(0.0, 0.5),
        );
        renderer.add_material(
            "flat_blue",
            Material::new([0.35, 0.55, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5),
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
        renderer.add_material(
            "glass",
            Material::new([0.7, 0.85, 1.0, 0.35])
                .with_metallic_roughness(0.2, 0.05)
                .with_alpha_mode(AlphaMode::Blend),
        );
        renderer.add_material(
            "foliage",
            Material::new([0.25, 0.75, 0.3, 1.0])
                .with_metallic_roughness(0.0, 0.7)
                .with_double_sided(true),
        );

        renderer.add_material(
            "emissive_warm",
            Material::new([1.0, 0.85, 0.4, 1.0])
                .with_metallic_roughness(0.0, 0.4)
                .with_emissive([3.5, 1.8, 0.4]),
        );
        renderer.add_material(
            "emissive_cyan",
            Material::new([0.5, 0.9, 1.0, 1.0])
                .with_metallic_roughness(0.0, 0.4)
                .with_emissive([0.5, 2.5, 3.2]),
        );
        renderer.add_material(
            "emissive_red",
            Material::new([1.0, 0.4, 0.4, 1.0])
                .with_metallic_roughness(0.0, 0.4)
                .with_emissive([3.0, 0.6, 0.6]),
        );
        renderer.add_material(
            "silver",
            Material::new([0.9, 0.9, 0.92, 1.0]).with_metallic_roughness(1.0, 0.15),
        );
        renderer.add_material(
            "chocolate",
            Material::new([0.28, 0.16, 0.10, 1.0]).with_metallic_roughness(0.0, 0.7),
        );
        renderer.add_material(
            "brick",
            Material::new([0.65, 0.28, 0.20, 1.0])
                .with_texture("checker")
                .with_metallic_roughness(0.0, 0.85),
        );

        match render::load_gltf_into(renderer, "assets/animated.glb", "anim") {
            Ok(loaded) => {
                println!(
                    "Loaded glTF: {} instances, {} skeletons, {} animations",
                    loaded.instances.len(),
                    loaded.skeletons.len(),
                    loaded.animations.len()
                );
                self.gltf_instances = loaded.instances;
                self.skeletons = loaded.skeletons;
                self.animations = loaded.animations;
            }
            Err(e) => {
                eprintln!("glTF not loaded ({}). Using only procedural meshes.", e);
            }
        }
    }

    fn update(
        &mut self,
        world: &mut World,
        input: &Input,
        renderer: &mut Renderer,
        dt: f32,
    ) -> bool {
        use winit::keyboard::KeyCode;

        if input.key_pressed(KeyCode::F12) {
            match renderer.reload_shaders() {
                Ok(()) => log::info!("Shaders reloaded"),
                Err(e) => log::error!("Shader reload failed: {}", e),
            }
        }

        if !input.play_mode {
            if input.key_pressed(KeyCode::KeyG) {
                self.show_grid = !self.show_grid;
            }
            if input.key_pressed(KeyCode::KeyC) {
                self.show_culling = !self.show_culling;
            }

            if input.key_pressed(KeyCode::F1) { self.postfx.debug_view = DebugView::Final; }
            if input.key_pressed(KeyCode::F2) { self.postfx.debug_view = DebugView::Ssao; }
            if input.key_pressed(KeyCode::F3) { self.postfx.debug_view = DebugView::GbufferNormal; }
            if input.key_pressed(KeyCode::F4) { self.postfx.debug_view = DebugView::GbufferDepth; }
            if input.key_pressed(KeyCode::F5) { self.postfx.debug_view = DebugView::HdrPreBloom; }
            if input.key_pressed(KeyCode::F6) { self.postfx.debug_view = DebugView::CsmCascade0; }
        }

        if !input.play_mode && !input.editor_flying {
            let lmb = input.mouse_down(winit::event::MouseButton::Left)
                && !input.editor_captured;
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

            let speed = 8.0 * dt;
            let mut pan = (0.0, 0.0);
            if input.key_down(KeyCode::KeyW) { pan.1 -= speed; }
            if input.key_down(KeyCode::KeyS) { pan.1 += speed; }
            if input.key_down(KeyCode::KeyA) { pan.0 -= speed; }
            if input.key_down(KeyCode::KeyD) { pan.0 += speed; }
            if pan != (0.0, 0.0) {
                self.camera.pan(pan.0, pan.1);
            }
        }

        self.orbit_phase += dt * 0.5;
        let (sp, cp) = self.orbit_phase.sin_cos();
        self.point_lights[1].position[0] = cp * 12.0;
        self.point_lights[1].position[2] = sp * 12.0;
        self.point_lights[2].position[0] = -cp * 12.0;
        self.point_lights[2].position[2] = -sp * 12.0;

        if !self.spawned {
            self.spawned = true;

            // === 1. ПОЛ ===
            let ground = world.spawn();
            world.insert(ground, Name("Ground".into()));
            world.insert(ground, Transform::new(0.0, 0.0, 0.0));
            world.insert(ground, MeshHandle("ground".into()));
            world.insert(ground, MaterialHandle("ground".into()));
            world.insert(ground, RigidBody::static_body());
            world.insert(ground, Collider::aabb(Vec3::new(60.0, 0.01, 60.0)));
            world.insert(ground, PhysicsMaterial::concrete());

            // === 2. КОЛОННЫ ===
            for (i, (x, z)) in [(-20.0, -20.0), (20.0, -20.0), (20.0, 20.0), (-20.0, 20.0)]
                .into_iter()
                .enumerate()
            {
                let col = world.spawn();
                world.insert(col, Name(format!("Column_{}", i)));
                world.insert(col, Transform::new(x, 4.0, z).with_scale_xyz(1.5, 8.0, 1.5));
                world.insert(col, MeshHandle("cube".into()));
                world.insert(col, MaterialHandle("gold".into()));
                world.insert(col, RigidBody::static_body());
                world.insert(col, Collider::aabb(Vec3::splat(0.5)));
                world.insert(col, PhysicsMaterial::metal());
            }

            // === 3. АЛТАРЬ + 3 ЭМИССИВНЫХ ШАРА ===
            let altar = world.spawn();
            world.insert(altar, Name("Altar".into()));
            world.insert(altar, Transform::new(0.0, 1.5, 0.0).with_scale_xyz(3.0, 3.0, 3.0));
            world.insert(altar, MeshHandle("cube".into()));
            world.insert(altar, MaterialHandle("silver".into()));
            world.insert(altar, Spinner::new(Vec3::Y, 0.4));
            world.insert(altar, RigidBody::static_body());
            world.insert(altar, Collider::aabb(Vec3::splat(0.5)));

            let emissive_colors = [
                ("emissive_warm", Vec3::new(0.0, 1.0, 0.0)),
                ("emissive_cyan", Vec3::new(0.9, 0.0, 0.5)),
                ("emissive_red",  Vec3::new(-0.9, 0.0, -0.5)),
            ];
            for (i, (mat, axis)) in emissive_colors.into_iter().enumerate() {
                let s = world.spawn();
                world.insert(s, Name(format!("AltarOrb_{}", i)));
                let angle = i as f32 * std::f32::consts::TAU / 3.0;
                world.insert(
                    s,
                    Transform::new(angle.cos() * 2.0, 6.0 + (i as f32) * 0.6, angle.sin() * 2.0)
                        .with_scale(0.8),
                );
                world.insert(s, MeshHandle("sphere".into()));
                world.insert(s, MaterialHandle(mat.into()));
                world.insert(s, Spinner::new(axis.normalize_or_zero(), 1.5));
            }

            // === 4. КОЛЬЦО КУБОВ ===
            let ring_materials = [
                "checker_red", "checker_blue", "gold", "silver",
                "flat_red", "flat_blue", "chocolate", "brick",
            ];
            for i in 0..16 {
                let angle = i as f32 / 16.0 * std::f32::consts::TAU;
                let r = 14.0;
                let e = world.spawn();
                world.insert(e, Name(format!("RingCube_{:02}", i)));
                world.insert(
                    e,
                    Transform::new(angle.cos() * r, 2.0, angle.sin() * r)
                        .with_rotation(Quat::from_axis_angle(Vec3::Y, angle))
                        .with_scale(0.9),
                );
                world.insert(e, MeshHandle("cube".into()));
                world.insert(e, MaterialHandle(ring_materials[i % ring_materials.len()].into()));
                let axis = if i % 2 == 0 { Vec3::Y } else { Vec3::new(0.3, 1.0, 0.4).normalize() };
                world.insert(e, Spinner::new(axis, 0.7 + (i as f32) * 0.15));
            }

            // === 5. СТЕКЛО ===
            for i in 0..8 {
                let angle = i as f32 / 8.0 * std::f32::consts::TAU;
                let e = world.spawn();
                world.insert(e, Name(format!("Glass_{}", i)));
                world.insert(
                    e,
                    Transform::new(angle.cos() * 6.0, 3.5, angle.sin() * 6.0).with_scale(1.6),
                );
                world.insert(e, MeshHandle("sphere".into()));
                world.insert(e, MaterialHandle("glass".into()));
                world.insert(e, Spinner::new(Vec3::Y, 0.3 + i as f32 * 0.1));
            }

            // === 6. ФИЗИКА: ПАДАЮЩИЕ КУБЫ ===
            for i in 0..12 {
                let e = world.spawn();
                world.insert(e, Name(format!("FallingCube_{:02}", i)));
                let x = -8.0 + (i % 3) as f32 * 1.1;
                let z = -8.0 + (i / 3) as f32 * 1.1;
                let y = 6.0 + (i / 3) as f32 * 1.4;
                world.insert(e, Transform::at(Vec3::new(x, y, z)).with_scale(1.0));
                world.insert(e, MeshHandle("cube".into()));
                world.insert(e, MaterialHandle(["flat_red", "flat_blue", "gold"][i % 3].into()));
                world.insert(e, RigidBody::dynamic(1.0));
                world.insert(e, Collider::aabb(Vec3::splat(0.5)));
                world.insert(e, PhysicsMaterial::wood());
            }

            // === 7. ФИЗИКА: ШАРЫ ===
            for i in 0..6 {
                let e = world.spawn();
                world.insert(e, Name(format!("BouncingBall_{}", i)));
                world.insert(
                    e,
                    Transform::at(Vec3::new(8.0 + i as f32 * 0.9, 5.0 + (i as f32) * 0.7, 8.0)),
                );
                world.insert(e, MeshHandle("sphere".into()));
                world.insert(
                    e,
                    MaterialHandle(["checker_red", "gold", "emissive_red"][i % 3].into()),
                );
                world.insert(e, RigidBody::dynamic(0.5));
                world.insert(e, Collider::sphere(0.5));
                world.insert(e, PhysicsMaterial::rubber());
            }

            // === 8. ЛИФТЫ ===
            for i in 0..2 {
                let e = world.spawn();
                world.insert(e, Name(format!("Lift_{}", i)));
                world.insert(
                    e,
                    Transform::new(-12.0 + i as f32 * 24.0, 2.0, 0.0)
                        .with_scale_xyz(2.5, 0.3, 2.5),
                );
                world.insert(e, MeshHandle("cube".into()));
                world.insert(e, MaterialHandle("chocolate".into()));
                world.insert(e, RigidBody::kinematic());
                world.insert(e, Collider::aabb(Vec3::splat(0.5)));
                world.insert(e, PhysicsMaterial::concrete());
            }

            // === 9. ВРАГИ ===
            for i in 0..8 {
                let angle = i as f32 / 8.0 * std::f32::consts::TAU;
                let e = world.spawn();
                world.insert(e, Name(format!("Enemy_{}", i)));
                world.insert(
                    e,
                    Transform::new(angle.cos() * 18.0, 0.8, angle.sin() * 18.0).with_scale(0.8),
                );
                world.insert(e, MeshHandle("sphere".into()));
                world.insert(e, MaterialHandle("brick".into()));
                world.insert(e, Health::new(50.0));
                world.insert(e, Chase::new(3.0, 1.5));
            }

            // === 10. PICKUPS + SWITCH ===
            for i in 0..5 {
                let angle = i as f32 / 5.0 * std::f32::consts::TAU;
                let e = world.spawn();
                world.insert(e, Name(format!("Pickup_{}", i)));
                world.insert(
                    e,
                    Transform::new(angle.cos() * 5.0, 1.2, angle.sin() * 5.0).with_scale(0.4),
                );
                world.insert(e, MeshHandle("sphere".into()));
                world.insert(e, MaterialHandle("emissive_warm".into()));
                world.insert(e, Interactable::Pickup);
                world.insert(e, Spinner::new(Vec3::Y, 2.0));
            }
            for i in 0..3 {
                let angle = i as f32 / 3.0 * std::f32::consts::TAU;
                let e = world.spawn();
                world.insert(e, Name(format!("Switch_{}", i)));
                world.insert(e, Transform::new(angle.cos() * 10.0, 1.2, angle.sin() * 10.0));
                world.insert(e, MeshHandle("cylinder".into()));
                world.insert(e, MaterialHandle("emissive_cyan".into()));
                world.insert(e, Interactable::Toggle);
                world.insert(e, Spinner::new(Vec3::Y, 1.5));
            }

            // === 11. ПОРТАЛ ===
            let portal = world.spawn();
            world.insert(portal, Name("Portal".into()));
            world.insert(
                portal,
                Transform::new(-16.0, 2.0, -16.0).with_scale_xyz(2.0, 4.0, 2.0),
            );
            world.insert(portal, MeshHandle("cube".into()));
            world.insert(portal, MaterialHandle("glass".into()));
            world.insert(portal, Trigger::new(3.0, TriggerAction::Teleport([0.0, 2.0, 0.0])));

            // === 12. ЛЕС ===
            for i in 0..12 {
                let angle = i as f32 / 12.0 * std::f32::consts::TAU;
                let r = 26.0 + (i % 3) as f32 * 1.5;
                let e = world.spawn();
                world.insert(e, Name(format!("Tree_{:02}", i)));
                world.insert(
                    e,
                    Transform::new(angle.cos() * r, 3.0, angle.sin() * r)
                        .with_rotation(Quat::from_axis_angle(Vec3::Y, angle))
                        .with_scale_xyz(3.0, 6.0, 1.0),
                );
                world.insert(e, MeshHandle("quad".into()));
                world.insert(e, MaterialHandle("foliage".into()));
            }

            // === 13. glTF ===
            let mut index = 0;
            for inst in &self.gltf_instances {
                let e = world.spawn();
                let node_label = inst.node_name.clone().unwrap_or_else(|| format!("Gltf_{}", index));
                world.insert(e, Name(node_label));
                let (scale, rot, trans) = inst.model.to_scale_rotation_translation();
                let offset = Vec3::new((index as f32) * 3.0 - 3.0, 0.5, 12.0);
                world.insert(
                    e,
                    Transform::at(trans + offset).with_rotation(rot).with_scale(scale.x),
                );
                world.insert(e, MeshHandle(inst.mesh_name.clone()));
                world.insert(e, MaterialHandle(inst.material_name.clone()));
                if let Some(skel_name) = &inst.skeleton_name {
                    world.insert(e, SkeletonHandle(skel_name.clone()));
                }
                if let Some(clip) = &inst.default_animation {
                    world.insert(e, AnimationPlayer::new(clip.clone()));
                }
                index += 1;
            }

            log::info!("Spawned demo arena with LOD-enabled meshes");
        }

        // Лифты.
        if self.spawned {
            let t = self.orbit_phase * 2.0;
            let entities: Vec<Entity> = world.entities().to_vec();
            for e in entities {
                if let Some(Name(n)) = world.get::<Name>(e) {
                    let base_y = if n == "Lift_0" { Some((2.0_f32, 0.0_f32)) }
                        else if n == "Lift_1" { Some((3.5_f32, 1.5_f32)) }
                        else { None };
                    if let Some((base, phase)) = base_y {
                        if let Some(tf) = world.get_mut::<Transform>(e) {
                            tf.position.y = base + ((t + phase) * 1.2).sin() * 2.0;
                        }
                        if let Some(rb) = world.get_mut::<RigidBody>(e) {
                            rb.wake();
                        }
                    }
                }
            }
        }

        for sys in self.systems.iter_mut() {
            sys.update(world, dt);
        }

        let anim_entities: Vec<_> = world.query::<AnimationPlayer>().map(|(e, _)| e).collect();
        for e in anim_entities {
            let Some(player) = world.get::<AnimationPlayer>(e) else { continue };
            let clip_name = player.clip.clone();
            let speed = player.speed;
            let looping = player.looping;

            let new_time = if let Some(player) = world.get_mut::<AnimationPlayer>(e) {
                player.time += dt * speed;
                player.time
            } else { continue };

            let Some(clip) = self.animations.get(&clip_name) else { continue };
            let duration = clip.duration;
            let final_time = if looping && duration > 0.0 { new_time % duration }
                else { new_time.min(duration) };

            let Some(skel_handle) = world.get::<SkeletonHandle>(e).cloned() else { continue };
            let Some(skel) = self.skeletons.get(&skel_handle.0) else { continue };

            let local_pose = clip.local_pose(final_time, &skel.local_bind);
            let joint_matrices = skel.joint_matrices(&local_pose);

            renderer.update_skeleton(&skel_handle.0, &joint_matrices);
        }

        true
    }

    fn apply_postfx(&mut self, postfx: PostFx) {
        self.postfx = postfx;
    }

    fn collect_draws(&mut self, world: &mut World, renderer: &Renderer) -> Vec<MeshDraw> {
        use std::collections::HashMap;

        type BucketKey = (String, String, [u32; 4], bool, bool);
        let mut buckets: HashMap<BucketKey, Vec<InstanceData>> = HashMap::new();
        let planes = self.camera.frustum_planes();
        let cam_pos = self.camera.position();

        let lod_bias = self.postfx.lod_bias.max(0.01);
        let lod_dists = self.postfx.lod_distances;

        // Сколько LOD было выбрано за этот кадр.
        let mut lod_counts = [0usize; 4];

        let entities: Vec<_> = world.entities().to_vec();
        for e in entities {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }

            let (Some(_t), Some(m), Some(mat)) = (
                world.get::<Transform>(e),
                world.get::<MeshHandle>(e),
                world.get::<MaterialHandle>(e),
            ) else { continue };

            let model = crate::game::world_matrix(world, e);

            let Some(mesh) = renderer.meshes.get(&m.0) else { continue };

            if self.show_culling {
                let (center, radius) = mesh.world_bounds(&model);
                if !sphere_in_frustum(center, radius, &planes) {
                    continue;
                }
            }

            // === LOD выбор ===
            let (world_center, world_radius) = mesh.world_bounds(&model);
            let dist = (world_center - cam_pos).length();

            // Порог LOD скорректирован на радиус: крупные объекты
            // переключаются позже.
            let dist_effective = (dist - world_radius).max(0.0) / lod_bias;

            let lod_level = if mesh.lods.is_empty() {
                0
            } else if dist_effective < lod_dists[0] {
                0
            } else if dist_effective < lod_dists[1] || mesh.lods.len() < 1 {
                1
            } else if dist_effective < lod_dists[2] || mesh.lods.len() < 2 {
                2.min(mesh.lods.len())
            } else {
                3.min(mesh.lods.len())
            };

            lod_counts[lod_level] += 1;

            let mesh_name = if lod_level == 0 {
                m.0.clone()
            } else {
                format!("{}__lod{}", m.0, lod_level - 1)
            };

            let material = renderer
                .materials
                .get(&mat.0)
                .unwrap_or_else(|| renderer.materials_default());

            let blend = material.alpha_mode == AlphaMode::Blend;
            let double_sided = material.double_sided;

            let color = world
                .get::<Tint>(e)
                .map(|t| t.0)
                .unwrap_or(material.base_color);

            let key = (
                mesh_name,
                mat.0.clone(),
                color.map(f32::to_bits),
                blend,
                double_sided,
            );
            let inst = InstanceData::new(model, color);
            buckets.entry(key).or_default().push(inst);
        }

        // Сохраняем статистику LOD.
        self.lod_stats = lod_counts;

        buckets
            .into_iter()
            .map(|((mesh, material_name, _, blend, double_sided), instances)| MeshDraw {
                mesh,
                instances,
                texture: Some(material_name),
                blend,
                double_sided,
            })
            .collect()
    }

    fn collect_lines(
        &mut self,
        world: &mut World,
        renderer: &Renderer,
        selected: &[Entity],
    ) -> Vec<LineVertex> {
        let mut batch = LineBatch::new();

        if self.show_grid {
            batch.grid(100.0, 2.0, [0.15, 0.18, 0.22, 1.0], [0.35, 0.40, 0.48, 1.0], 5);
            batch.axes(5.0);
        }

        for &e in selected {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }

            if let (Some(_t), Some(mh)) = (
                world.get::<Transform>(e),
                world.get::<MeshHandle>(e),
            ) {
                if let Some(mesh) = renderer.meshes.get(&mh.0) {
                    let model = crate::game::world_matrix(world, e);
                    let (center, radius) = mesh.world_bounds(&model);
                    batch.sphere_wireframe(center, radius * 1.05, [1.0, 0.85, 0.2, 1.0], 24);
                }
            }
        }

        batch.vertices().to_vec()
    }

    fn dir_lights(&self) -> Vec<GpuLight> { self.dir_lights.clone() }
    fn point_lights(&self) -> Vec<GpuPointLight> { self.point_lights.clone() }
    fn ambient(&self) -> [f32; 3] { [0.15, 0.17, 0.22] }
    fn postfx(&self) -> PostFx { self.postfx }
    fn camera(&self) -> &Camera3D { &self.camera }
    fn camera_mut(&mut self) -> &mut Camera3D { &mut self.camera }
}

fn main() {
    run(DemoGame::new());
}