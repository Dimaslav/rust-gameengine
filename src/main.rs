#![allow(dead_code)]
#![allow(unused_imports)]

mod assets;
mod ecs;
mod editor;
mod engine;
mod game;
mod physics;
mod render;
mod scene;

use std::any::Any;
use std::collections::HashMap;

use ecs::{Entity, System, World};
use engine::{run, Game, Input, InputMap, Key, MouseBtn};
use game::ai::{AiAgent, AiState, AiTarget, DebugPath, Enemy, PatrolPath};
use game::animation::{AnimationEvent, AnimationEvents, AnimationRuntime};
use game::audio::AudioSource;
use game::components::{
    AnimationPlayer, Elevator, ElevatorState, Interactable, MaterialHandle, MeshHandle, Name,
    Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint, Transform, Trigger,
    TriggerAction, Velocity, Visible,
};
use game::decals::Decal;
use game::lights::{DirectionalLight, PointLight};
use game::timers::{Timer, TimerFinished, TimerSystem};
use game::AudioBus;
use glam::{Quat, Vec3};
use physics::{BodyType, Collider, RigidBody};
use render::{
    skinning::AnimationClip, AlphaMode, Camera3D, DebugView, GltfInstance, GpuLight,
    GpuPointLight, InstanceData, LineBatch, LineVertex, Material, Mesh, MeshDraw, PostFx,
    Renderer, Skeleton,
};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

// ============================================================
// Константы арены
// ============================================================

const ARENA_HALF: f32 = 20.0;
const WALL_HEIGHT: f32 = 4.0;
const WALL_THICKNESS: f32 = 0.5;
const GATE_WIDTH: f32 = 5.0;
const PLATFORM_HEIGHT: f32 = 1.0;
const PLATFORM_HALF: f32 = 4.0;

// ============================================================
// Хелперы
// ============================================================

fn sphere_in_frustum(center: Vec3, radius: f32, planes: &[glam::Vec4; 6]) -> bool {
    for p in planes {
        let dist = p.x * center.x + p.y * center.y + p.z * center.z + p.w;
        if dist < -radius { return false; }
    }
    true
}

fn compute_uv_scale(mesh: &Mesh, model: &glam::Mat4, tiling_size: f32) -> [f32; 2] {
    let local_extent = (mesh.aabb_max - mesh.aabb_min).abs();
    let (scale, _, _) = model.to_scale_rotation_translation();
    let scale_abs = scale.abs();
    let world_extent = local_extent * scale_abs;

    let tile = tiling_size.max(0.001);
    let mut uv_x = world_extent.x.max(world_extent.z) / tile;
    let mut uv_y = world_extent.y / tile;

    if uv_y < 0.001 { uv_y = uv_x; }
    if uv_x < 0.001 { uv_x = uv_y; }
    [uv_x, uv_y]
}

fn spawn_static_box(
    world: &mut World,
    name: impl Into<String>,
    material: &str,
    center: Vec3,
    size: Vec3,
    collider_half: Vec3,
    tiling: f32,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform {
        position: center,
        rotation: Quat::IDENTITY,
        scale: size,
    });
    world.insert(e, MeshHandle("cube".into()));
    world.insert(e, MaterialHandle(material.to_string()));
    world.insert(e, TextureTiling::new(tiling));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(collider_half));
    e
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
                if axis.length_squared() < 1e-6 { continue; }
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

struct ElevatorSystem;
impl System for ElevatorSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        let entities: Vec<_> = world.query::<Elevator>().map(|(e, _)| e).collect();
        for e in entities {
            let Some(mut el) = world.get::<Elevator>(e).cloned() else { continue };
            let current_y = world.get::<Transform>(e).map(|t| t.position.y).unwrap_or(0.0);

            match el.state {
                ElevatorState::Idle => { el.current_velocity = 0.0; }
                ElevatorState::DoorsOpening => {
                    el.current_velocity = 0.0;
                    el.doors_open = (el.doors_open + el.door_speed * dt).min(1.0);
                    if el.doors_open >= 1.0 {
                        el.state = ElevatorState::DoorsOpen;
                        el.dwell_timer = el.dwell;
                    }
                }
                ElevatorState::DoorsOpen => {
                    el.current_velocity = 0.0;
                    if el.player_inside { el.dwell_timer = el.dwell; }
                    else {
                        el.dwell_timer -= dt;
                        if el.dwell_timer <= 0.0 { el.state = ElevatorState::DoorsClosing; }
                    }
                }
                ElevatorState::DoorsClosing => {
                    el.current_velocity = 0.0;
                    if el.player_inside { el.state = ElevatorState::DoorsOpening; }
                    else {
                        el.doors_open = (el.doors_open - el.door_speed * dt).max(0.0);
                        if el.doors_open <= 0.0 {
                            if el.target_floor != el.current_floor { el.state = ElevatorState::Moving; }
                            else { el.state = ElevatorState::Idle; }
                        }
                    }
                }
                ElevatorState::Moving => {
                    let arrived = el.update_moving(current_y, dt);
                    if arrived { el.state = ElevatorState::DoorsOpening; }
                }
            }

            if let Some(rb) = world.get_mut::<RigidBody>(e) {
                if rb.body_type == BodyType::Kinematic {
                    rb.velocity = Vec3::new(0.0, el.current_velocity, 0.0);
                }
            }

            let doors: Vec<Entity> = world.entities().iter().copied()
                .filter(|&d| {
                    world.get::<SlidingDoor>(d).is_some()
                        && world.get::<Parent>(d).map(|p| p.0 == e).unwrap_or(false)
                }).collect();
            for d in doors {
                if let Some(sd) = world.get_mut::<SlidingDoor>(d) {
                    sd.target = el.doors_open;
                }
            }

            if let Some(slot) = world.get_mut::<Elevator>(e) { *slot = el; }
        }
    }
}

struct SlidingDoorSystem;
impl System for SlidingDoorSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        let entities: Vec<_> = world.query::<SlidingDoor>().map(|(e, _)| e).collect();
        for e in entities {
            let Some(mut sd) = world.get::<SlidingDoor>(e).copied() else { continue };
            let Some(t) = world.get_mut::<Transform>(e) else { continue };

            let diff = sd.target - sd.open_amount;
            if diff.abs() < 1e-4 { sd.open_amount = sd.target; }
            else {
                let step = sd.speed * dt;
                if step >= diff.abs() { sd.open_amount = sd.target; }
                else { sd.open_amount += step * diff.signum(); }
            }

            t.position = sd.closed_position + sd.slide_axis * sd.slide_distance * sd.open_amount;
            if let Some(slot) = world.get_mut::<SlidingDoor>(e) { *slot = sd; }
        }
    }
}

// ============================================================
// Спавн арены
// ============================================================

fn spawn_arena_ground(world: &mut World) {
    let e = world.spawn();
    world.insert(e, Name("Arena_Ground".into()));
    world.insert(e, Transform::at(Vec3::new(0.0, -0.01, 0.0)));
    world.insert(e, MeshHandle("ground".into()));
    world.insert(e, MaterialHandle("arena_floor".into()));
    world.insert(e, TextureTiling::new(2.0));
    world.insert(e, RigidBody::static_body());
    world.insert(e, Collider::aabb(Vec3::new(ARENA_HALF, 0.01, ARENA_HALF)));
}

fn spawn_arena_walls(world: &mut World) {
    let gate_half = GATE_WIDTH * 0.5;
    let wall_len = ARENA_HALF - gate_half;
    let wall_center_offset = gate_half + wall_len * 0.5;

    spawn_static_box(world, "Wall_N_L", "arena_wall",
        Vec3::new(-wall_center_offset, WALL_HEIGHT * 0.5, -ARENA_HALF),
        Vec3::new(wall_len, WALL_HEIGHT, WALL_THICKNESS),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_N_R", "arena_wall",
        Vec3::new(wall_center_offset, WALL_HEIGHT * 0.5, -ARENA_HALF),
        Vec3::new(wall_len, WALL_HEIGHT, WALL_THICKNESS),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_S_L", "arena_wall",
        Vec3::new(-wall_center_offset, WALL_HEIGHT * 0.5, ARENA_HALF),
        Vec3::new(wall_len, WALL_HEIGHT, WALL_THICKNESS),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_S_R", "arena_wall",
        Vec3::new(wall_center_offset, WALL_HEIGHT * 0.5, ARENA_HALF),
        Vec3::new(wall_len, WALL_HEIGHT, WALL_THICKNESS),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_W_N", "arena_wall",
        Vec3::new(-ARENA_HALF, WALL_HEIGHT * 0.5, -wall_center_offset),
        Vec3::new(WALL_THICKNESS, WALL_HEIGHT, wall_len),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_W_S", "arena_wall",
        Vec3::new(-ARENA_HALF, WALL_HEIGHT * 0.5, wall_center_offset),
        Vec3::new(WALL_THICKNESS, WALL_HEIGHT, wall_len),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_E_N", "arena_wall",
        Vec3::new(ARENA_HALF, WALL_HEIGHT * 0.5, -wall_center_offset),
        Vec3::new(WALL_THICKNESS, WALL_HEIGHT, wall_len),
        Vec3::splat(0.5), 2.0);
    spawn_static_box(world, "Wall_E_S", "arena_wall",
        Vec3::new(ARENA_HALF, WALL_HEIGHT * 0.5, wall_center_offset),
        Vec3::new(WALL_THICKNESS, WALL_HEIGHT, wall_len),
        Vec3::splat(0.5), 2.0);
}

fn spawn_arena_center(world: &mut World) {
    spawn_static_box(world, "Arena_Platform", "arena_platform",
        Vec3::new(0.0, PLATFORM_HEIGHT * 0.5, 0.0),
        Vec3::new(PLATFORM_HALF * 2.0, PLATFORM_HEIGHT, PLATFORM_HALF * 2.0),
        Vec3::splat(0.5), 2.0);

    let col_half = PLATFORM_HALF - 0.6;
    let col_height = 3.0;
    let col_y = PLATFORM_HEIGHT + col_height * 0.5;
    for (i, (sx, sz)) in [(-1.0_f32, -1.0_f32), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)]
        .iter().enumerate()
    {
        spawn_static_box(world, format!("Arena_Column_{}", i), "arena_column",
            Vec3::new(sx * col_half, col_y, sz * col_half),
            Vec3::new(0.8, col_height, 0.8),
            Vec3::splat(0.5), 1.5);
    }

    let e = world.spawn();
    world.insert(e, Name("Arena_CenterMarker".into()));
    world.insert(e, Transform::at(Vec3::new(0.0, PLATFORM_HEIGHT + 3.0, 0.0)).with_scale(0.5));
    world.insert(e, MeshHandle("sphere".into()));
    world.insert(e, MaterialHandle("arena_marker".into()));
    world.insert(e, Spinner::new(Vec3::Y, 1.2));
}

fn spawn_arena_props(world: &mut World) {
    let crates: &[(Vec3, f32, &str)] = &[
        (Vec3::new(-12.0, 0.5, -12.0), 1.0, "arena_crate"),
        (Vec3::new(-10.5, 0.75, -12.5), 1.5, "arena_crate"),
        (Vec3::new(-11.5, 1.25, -11.5), 1.0, "arena_crate"),
        (Vec3::new(12.0, 0.5, -12.0), 1.0, "arena_crate"),
        (Vec3::new(11.5, 1.0, -11.0), 1.0, "arena_crate"),
        (Vec3::new(-12.0, 0.5, 12.0), 1.0, "arena_crate"),
        (Vec3::new(12.0, 0.75, 12.0), 1.5, "arena_crate"),
        (Vec3::new(11.0, 0.5, 10.5), 1.0, "arena_crate"),
        (Vec3::new(8.0, 0.5, -14.0), 1.0, "arena_crate"),
        (Vec3::new(-8.0, 0.5, 14.0), 1.0, "arena_crate"),
    ];
    for (i, (pos, size, mat)) in crates.iter().enumerate() {
        spawn_static_box(world, format!("Crate_{}", i), mat,
            *pos, Vec3::splat(*size), Vec3::splat(0.5), 1.0);
    }

    let barrels: &[(Vec3, &str)] = &[
        (Vec3::new(-16.0, 0.6, -3.0), "arena_barrel"),
        (Vec3::new(-16.5, 0.6, -1.5), "arena_barrel"),
        (Vec3::new(16.0, 0.6, 3.0), "arena_barrel"),
        (Vec3::new(15.5, 0.6, 1.5), "arena_barrel"),
        (Vec3::new(-3.0, 0.6, 15.0), "arena_barrel"),
        (Vec3::new(3.0, 0.6, -15.0), "arena_barrel"),
    ];
    for (i, (pos, mat)) in barrels.iter().enumerate() {
        let e = world.spawn();
        world.insert(e, Name(format!("Barrel_{}", i)));
        world.insert(e, Transform {
            position: *pos, rotation: Quat::IDENTITY,
            scale: Vec3::new(0.8, 1.2, 0.8),
        });
        world.insert(e, MeshHandle("cylinder".into()));
        world.insert(e, MaterialHandle(mat.to_string()));
        world.insert(e, TextureTiling::new(1.0));
        world.insert(e, RigidBody::static_body());
        world.insert(e, Collider::aabb(Vec3::splat(0.5)));
    }
}

fn spawn_arena_cover(world: &mut World) {
    let covers: &[(Vec3, Vec3)] = &[
        (Vec3::new(-8.0, 0.5, 0.0), Vec3::new(0.3, 1.0, 6.0)),
        (Vec3::new(8.0, 0.5, 0.0), Vec3::new(0.3, 1.0, 6.0)),
        (Vec3::new(0.0, 0.5, -8.0), Vec3::new(6.0, 1.0, 0.3)),
        (Vec3::new(0.0, 0.5, 8.0), Vec3::new(6.0, 1.0, 0.3)),
    ];
    for (i, (pos, size)) in covers.iter().enumerate() {
        spawn_static_box(world, format!("Cover_{}", i), "arena_cover",
            *pos, *size, Vec3::splat(0.5), 2.0);
    }
}

fn spawn_arena_elevator(world: &mut World) {
    let xz = (-16.0_f32, -16.0_f32);
    let floors_y = vec![0.0, 2.0, 4.0];
    let speed = 2.5;
    let y0 = floors_y[0];
    let plat_scale = Vec3::new(3.0, 0.2, 3.0);

    let plat = world.spawn();
    world.insert(plat, Name("ArenaElevator".into()));
    world.insert(plat, Transform::at(Vec3::new(xz.0, y0, xz.1))
        .with_scale_xyz(plat_scale.x, plat_scale.y, plat_scale.z));
    world.insert(plat, MeshHandle("cube".into()));
    world.insert(plat, MaterialHandle("arena_platform".into()));
    world.insert(plat, TextureTiling::new(1.5));
    world.insert(plat, RigidBody::kinematic());
    world.insert(plat, Collider::aabb(Vec3::splat(0.5)));

    let mut el = Elevator::new(floors_y.clone(), speed);
    el.acceleration = 3.0;
    el.sensor_radius = 2.5;
    world.insert(plat, el);

    for side in [-1.0_f32, 1.0] {
        let d = world.spawn();
        world.insert(d, Name(format!("ArenaElevatorDoor_{}", if side < 0.0 { "L" } else { "R" })));
        let desired_world_size = Vec3::new(1.4, 2.0, 0.1);
        let local_scale = desired_world_size / plat_scale;
        let desired_world_pos = Vec3::new(side * 0.7, 1.0, 1.5);
        let local_pos = desired_world_pos / plat_scale;
        world.insert(d, Transform::at(local_pos)
            .with_scale_xyz(local_scale.x, local_scale.y, local_scale.z));
        world.insert(d, MeshHandle("cube".into()));
        world.insert(d, MaterialHandle("arena_door".into()));
        world.insert(d, Parent(plat));
        world.insert(d, Collider::aabb(Vec3::splat(0.5)));
        world.insert(d, SlidingDoor::new(local_pos, Vec3::X * side, 1.4 / plat_scale.x));
    }

    for (idx, &y) in floors_y.iter().enumerate() {
        let b = world.spawn();
        world.insert(b, Name(format!("ArenaElevatorButton_{}", idx)));
        world.insert(b, Transform::at(Vec3::new(xz.0 + 2.5, y + 1.0, xz.1)).with_scale(0.25));
        world.insert(b, MeshHandle("cube".into()));
        world.insert(b, MaterialHandle("emissive".into()));
        world.insert(b, Spinner::new(Vec3::Y, 1.5));
        world.insert(b, Trigger::repeatable(1.2, TriggerAction::CallElevator {
            elevator: plat,
            floor_idx: idx as u32,
        }));
    }

    log::info!("Arena elevator at ({:.1}, {:.1}), floors {:?}", xz.0, xz.1, floors_y);
}

fn spawn_arena_lighting(world: &mut World) {
    {
        let e = world.spawn();
        world.insert(e, Name("Arena_Sun".into()));
        world.insert(e, Transform::at(Vec3::new(0.0, 20.0, 0.0)));
        let mut sun = DirectionalLight::sun();
        sun.direction = Vec3::new(0.5, 1.0, 0.3);
        sun.intensity = 3.0;
        world.insert(e, sun);
    }
    {
        let e = world.spawn();
        world.insert(e, Name("Arena_Fill".into()));
        world.insert(e, Transform::at(Vec3::new(0.0, 10.0, 0.0)));
        world.insert(e, DirectionalLight::fill());
    }

    let lamps: &[(Vec3, [f32; 3], f32, f32)] = &[
        (Vec3::new(-14.0, 3.0, -14.0), [1.0, 0.65, 0.35], 6.0, 14.0),
        (Vec3::new( 14.0, 3.0, -14.0), [1.0, 0.65, 0.35], 6.0, 14.0),
        (Vec3::new(-14.0, 3.0,  14.0), [1.0, 0.65, 0.35], 6.0, 14.0),
        (Vec3::new( 14.0, 3.0,  14.0), [1.0, 0.65, 0.35], 6.0, 14.0),
    ];
    for (i, (pos, color, intensity, range)) in lamps.iter().enumerate() {
        let e = world.spawn();
        world.insert(e, Name(format!("Arena_Lamp_{}", i)));
        world.insert(e, Transform::at(*pos).with_scale(0.2));
        world.insert(e, MeshHandle("sphere".into()));
        world.insert(e, MaterialHandle("emissive".into()));
        world.insert(e, PointLight::new(*color, *intensity, *range));
    }
}

fn spawn_arena_decals(world: &mut World) {
    let spots = [
        (Vec3::new(-12.0, 0.02, -12.0), 1.8, 0.85),
        (Vec3::new(-11.5, 0.02, -13.0), 1.5, 0.9),
        (Vec3::new(12.0, 0.02, -11.5), 2.0, 0.8),
        (Vec3::new(-8.0, PLATFORM_HEIGHT + 0.02, 0.0), 1.2, 0.7),
        (Vec3::new(8.0, PLATFORM_HEIGHT + 0.02, 0.0), 1.5, 0.75),
        (Vec3::new(0.0, 0.02, 6.0), 2.2, 0.85),
    ];
    for (i, (pos, size, alpha)) in spots.iter().enumerate() {
        let e = world.spawn();
        world.insert(e, Name(format!("Decal_Blood_{}", i)));
        world.insert(e, Transform {
            position: *pos, rotation: Quat::IDENTITY,
            scale: Vec3::new(*size, 0.01, *size),
        });
        world.insert(e, Decal {
            texture: "arena_blood".into(),
            tint: [0.65, 0.05, 0.05, *alpha],
        });
    }
}

fn spawn_arena_bell(world: &mut World) -> Entity {
    let bell = world.spawn();
    world.insert(bell, Name("Arena_Bell".into()));
    world.insert(bell, Transform::at(Vec3::new(0.0, PLATFORM_HEIGHT + 3.0, 0.0)).with_scale(0.3));
    world.insert(bell, MeshHandle("sphere".into()));
    world.insert(bell, MaterialHandle("emissive".into()));
    world.insert(bell, Spinner::new(Vec3::Y, 0.8));
    world.insert(bell, Timer::new(4.0));
    world.insert(bell, AudioSource::new("ding")
        .with_bus(AudioBus::Music)
        .with_range(1.0, 20.0)
        .with_volume(0.8));
    bell
}

fn spawn_arena_ai_target(world: &mut World) -> Entity {
    let e = world.spawn();
    world.insert(e, Name("Arena_AiTarget".into()));
    world.insert(e, Transform::at(Vec3::new(0.0, 1.0, 0.0)));
    world.insert(e, AiTarget);
    world.insert(e, game::Health::new(100_000.0));
    e
}

fn spawn_arena_ai(world: &mut World) {
    let cycles: &[(&str, Vec<Vec3>, f32, &str, f32)] = &[
        (
            "Patrol_Perimeter",
            vec![
                Vec3::new(-15.0, 0.5, -15.0),
                Vec3::new( 15.0, 0.5, -15.0),
                Vec3::new( 15.0, 0.5,  15.0),
                Vec3::new(-15.0, 0.5,  15.0),
            ],
            3.0, "flat_red", 0.6,
        ),
        (
            "Patrol_Center",
            vec![
                Vec3::new(-7.0, PLATFORM_HEIGHT + 0.5, 0.0),
                Vec3::new( 0.0, PLATFORM_HEIGHT + 0.5, -7.0),
                Vec3::new( 7.0, PLATFORM_HEIGHT + 0.5, 0.0),
                Vec3::new( 0.0, PLATFORM_HEIGHT + 0.5, 7.0),
            ],
            4.0, "flat_red", 0.55,
        ),
        (
            "Patrol_Gates",
            vec![
                Vec3::new(-18.0, 0.5, 0.0),
                Vec3::new( 0.0, 0.5, -18.0),
                Vec3::new( 18.0, 0.5, 0.0),
                Vec3::new( 0.0, 0.5,  18.0),
            ],
            3.5, "flat_red", 0.65,
        ),
    ];

    for (i, (name, cycle, speed, mat, scale)) in cycles.iter().enumerate() {
        let start = cycle[0];
        let e = world.spawn();
        world.insert(e, Name(format!("AiEnemy_{}_{}", i, name)));
        world.insert(e, Transform::at(start).with_scale(*scale));
        world.insert(e, MeshHandle("sphere".into()));
        world.insert(e, MaterialHandle(mat.to_string()));
        world.insert(e, game::Health::new(60.0));
        world.insert(e, Enemy);
        world.insert(e, AiAgent::new()
            .with_speed(*speed)
            .with_vision(18.0, 60_f32.to_radians())
            .with_hearing(25.0)
            .with_attack(1.6, 12.0, 0.9));
        world.insert(e, PatrolPath::new(cycle.clone()));
        world.insert(e, DebugPath);
    }

    log::info!("Spawned {} AI enemies with patrol paths", cycles.len());
}

// ============================================================
// Демо-игра
// ============================================================

struct DemoGame {
    camera: Camera3D,
    systems: Vec<Box<dyn System>>,
    postfx: PostFx,
    spawned: bool,
    dragging: bool,
    show_grid: bool,
    show_culling: bool,
    orbit_phase: f32,

    gltf_instances: Vec<GltfInstance>,
    skeletons: HashMap<String, Skeleton>,
    animations: HashMap<String, AnimationClip>,

    animation_runtime: AnimationRuntime,
    animation_events: AnimationEvents,

    bell_entity: Option<Entity>,
    ai_target_entity: Option<Entity>,

    navmesh_bake_requested: bool,
    ai_spawned: bool,

    lod_stats: [usize; 4],
    prev_world_matrices: HashMap<Entity, glam::Mat4>,
}

impl DemoGame {
    fn new() -> Self {
        Self {
            camera: Camera3D::new(16.0 / 9.0),
            systems: vec![
                Box::new(TimerSystem),
                Box::new(RotationSystem),
                Box::new(MovementSystem),
                Box::new(ElevatorSystem),
                Box::new(SlidingDoorSystem),
            ],
            postfx: PostFx {
                bloom_threshold: 1.0,
                bloom_strength: 0.5,
                bloom_knee: 0.5,
                bloom_radius: 1.0,
                exposure: 0.7,
                ssao_strength: 0.9,
                ssao_radius: 0.6,
                ibl_strength: 0.2,
                debug_view: DebugView::Final,
                fxaa_strength: 1.0,
                fog_color: [0.5, 0.55, 0.65],
                fog_density: 0.0,
                fog_height_base: 0.0,
                fog_height_falloff: 0.05,
                vignette_strength: 0.1,
                film_grain: 0.0,
                chromatic_aberration: 0.0,
                shadow_bias: 0.0015,
                shadow_normal_bias: 3.0,
                shadow_fade_start: 150.0,
                shadow_fade_end: 200.0,
                lod_bias: 1.0,
                lod_distances: [30.0, 80.0, 200.0, 500.0],
                taa_strength: 1.0,
                taa_sharpening: 0.1,
                volumetric_density: 0.001,
                volumetric_scattering: 0.4,
                volumetric_phase_g: 0.6,
            },
            spawned: false,
            dragging: false,
            show_grid: false,
            show_culling: true,
            orbit_phase: 0.0,
            gltf_instances: Vec::new(),
            skeletons: HashMap::new(),
            animations: HashMap::new(),
            animation_runtime: AnimationRuntime::new(),
            animation_events: AnimationEvents::new(),
            bell_entity: None,
            ai_target_entity: None,
            navmesh_bake_requested: false,
            ai_spawned: false,
            lod_stats: [0; 4],
            prev_world_matrices: HashMap::new(),
        }
    }
}

impl Game for DemoGame {
    // ИЗМЕНЕНО: используем init_with_assets, чтобы получить доступ
    // к AssetDatabase и залогировать найденные ассеты.
    // Старый init() остаётся в трейте с дефолтной реализацией —
    // существующий код без изменений продолжает работать.
    fn init_with_assets(
        &mut self,
        _world: &mut World,
        renderer: &mut Renderer,
        assets: &crate::assets::AssetDatabase,
    ) {
        log::info!(
            "DemoGame: AssetDatabase reports {} assets under {}",
            assets.len(),
            assets.root().display()
        );

        renderer.add_mesh("cube", Mesh::cube(&renderer.device, 1.0));
        renderer.add_mesh("sphere", Mesh::sphere(&renderer.device, 0.5, 16, 24));
        renderer.add_mesh("ground", Mesh::plane(&renderer.device, 200.0, 1));
        renderer.add_mesh("quad", Mesh::plane(&renderer.device, 2.0, 1));
        renderer.add_mesh("quad_xy", Mesh::plane_xy(&renderer.device, 2.0, 1));
        renderer.add_mesh("cylinder", Mesh::cylinder(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("cone", Mesh::cone(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("capsule", Mesh::capsule(&renderer.device, 0.4, 0.8, 6, 20));

        let mut data = vec![0u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let idx = (y * 64 + x) * 4;
                let c = if ((x / 8) + (y / 8)) % 2 == 0 { 220 } else { 60 };
                data[idx] = c; data[idx + 1] = c; data[idx + 2] = c; data[idx + 3] = 255;
            }
        }
        renderer.load_texture_rgba("checker", &data, 64, 64).expect("checker");

        let mut blood = vec![0u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let idx = (y * 64 + x) * 4;
                let dx = x as f32 - 31.5;
                let dy = y as f32 - 31.5;
                let d = (dx * dx + dy * dy).sqrt() / 32.0;
                let a = (1.0 - d * 1.6).clamp(0.0, 1.0);
                let noise = ((x * 17 + y * 31) % 16) as f32 / 16.0 * 0.3;
                let alpha = (a * (1.0 - noise)).clamp(0.0, 1.0);
                blood[idx] = (140.0 * (1.0 - d * 0.5)) as u8;
                blood[idx + 1] = 20;
                blood[idx + 2] = 15;
                blood[idx + 3] = (alpha * 255.0) as u8;
            }
        }
        renderer.load_texture_rgba("arena_blood", &blood, 64, 64).expect("blood");

        renderer.add_material("arena_floor",
            Material::new([0.48, 0.52, 0.55, 1.0]).with_metallic_roughness(0.0, 0.9));
        renderer.add_material("arena_wall",
            Material::new([0.42, 0.42, 0.46, 1.0]).with_metallic_roughness(0.0, 0.85));
        renderer.add_material("arena_platform",
            Material::new([0.55, 0.58, 0.62, 1.0]).with_metallic_roughness(0.0, 0.8));
        renderer.add_material("arena_column",
            Material::new([0.65, 0.63, 0.58, 1.0]).with_metallic_roughness(0.1, 0.7));
        renderer.add_material("arena_crate",
            Material::new([0.65, 0.45, 0.25, 1.0]).with_metallic_roughness(0.0, 0.85));
        renderer.add_material("arena_barrel",
            Material::new([0.55, 0.30, 0.15, 1.0]).with_metallic_roughness(0.3, 0.6));
        renderer.add_material("arena_cover",
            Material::new([0.50, 0.50, 0.52, 1.0]).with_metallic_roughness(0.0, 0.9));
        renderer.add_material("arena_door",
            Material::new([0.45, 0.30, 0.20, 1.0]).with_metallic_roughness(0.1, 0.7));
        renderer.add_material("arena_marker",
            Material::new([1.0, 0.9, 0.4, 1.0]).with_metallic_roughness(0.0, 0.5).with_emissive([2.5, 2.2, 0.6]));

        renderer.add_material("flat_red",
            Material::new([1.0, 0.35, 0.35, 1.0]).with_metallic_roughness(0.0, 0.5));
        renderer.add_material("flat_blue",
            Material::new([0.35, 0.55, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5));
        renderer.add_material("gold",
            Material::new([1.0, 0.85, 0.3, 1.0]).with_metallic_roughness(1.0, 0.25));
        renderer.add_material("emissive",
            Material::new([1.0, 1.0, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5).with_emissive([2.5, 2.2, 0.6]));
        renderer.add_material("glass",
            Material::new([0.7, 0.85, 1.0, 0.35]).with_metallic_roughness(0.2, 0.05).with_alpha_mode(AlphaMode::Blend));

        match render::load_gltf_into(renderer, "assets/animated.glb", "anim") {
            Ok(loaded) => {
                log::info!("Loaded glTF: {} instances, {} skeletons, {} animations",
                    loaded.instances.len(), loaded.skeletons.len(), loaded.animations.len());
                self.gltf_instances = loaded.instances;
                self.skeletons = loaded.skeletons;
                self.animations = loaded.animations;
            }
            Err(e) => log::info!("glTF not loaded ({}). Using procedural meshes only.", e),
        }

        let sidecar = "assets/animated.anim_events.ron";
        match AnimationEvents::from_file(sidecar) {
            Ok(ev) if !ev.is_empty() => {
                log::info!("Animation events: loaded {} clip(s) from {}",
                    ev.clip_count(), sidecar);
                self.animation_events = ev;
            }
            _ => {
                if let Some(clip_name) = self.animations.keys().next().cloned() {
                    self.animation_events.add(&clip_name,
                        AnimationEvent::new(0.25, "footstep").with_payload("footstep_grass"));
                    self.animation_events.add(&clip_name,
                        AnimationEvent::new(0.75, "footstep").with_payload("footstep_grass"));
                }
            }
        }
    }

    fn configure_input(&mut self, map: &mut InputMap) {
        map
            .bind("toggle_grid", Key::KeyG)
            .bind("toggle_culling", Key::KeyC)
            .bind("debug_final", Key::F1)
            .bind("debug_ssao", Key::F2)
            .bind("debug_gbuffer_normal", Key::F3)
            .bind("debug_gbuffer_depth", Key::F4)
            .bind("debug_hdr", Key::F5)
            .bind("debug_csm", Key::F6)
            .bind("reload_shaders", Key::F12)
            .bind_mouse("primary_fire", MouseBtn::Left);

        for act in [
            "toggle_grid", "toggle_culling",
            "debug_final", "debug_ssao",
            "debug_gbuffer_normal", "debug_gbuffer_depth",
            "debug_hdr", "debug_csm",
        ] {
            map.action_in_context(act, "editor");
        }
        map.action_in_context("primary_fire", "gameplay");
    }

    fn wants_navmesh_bake(&mut self) -> bool {
        std::mem::take(&mut self.navmesh_bake_requested)
    }

    fn update(
        &mut self,
        world: &mut World,
        input: &Input,
        renderer: &mut Renderer,
        dt: f32,
    ) -> bool {
        if input.pressed("reload_shaders") {
            match renderer.reload_shaders() {
                Ok(()) => log::info!("Shaders reloaded"),
                Err(e) => log::error!("Shader reload failed: {}", e),
            }
        }

        let ctrl = input.key_down(KeyCode::ControlLeft) || input.key_down(KeyCode::ControlRight);
        let alt = input.key_down(KeyCode::AltLeft) || input.key_down(KeyCode::AltRight);
        let shift = input.key_down(KeyCode::ShiftLeft) || input.key_down(KeyCode::ShiftRight);
        let plain = !ctrl && !alt && !shift;

        if plain {
            if input.pressed("toggle_grid") { self.show_grid = !self.show_grid; }
            if input.pressed("toggle_culling") { self.show_culling = !self.show_culling; }
        }

        if input.pressed("debug_final") { self.postfx.debug_view = DebugView::Final; }
        if input.pressed("debug_ssao") { self.postfx.debug_view = DebugView::Ssao; }
        if input.pressed("debug_gbuffer_normal") { self.postfx.debug_view = DebugView::GbufferNormal; }
        if input.pressed("debug_gbuffer_depth") { self.postfx.debug_view = DebugView::GbufferDepth; }
        if input.pressed("debug_hdr") { self.postfx.debug_view = DebugView::HdrPreBloom; }
        if input.pressed("debug_csm") { self.postfx.debug_view = DebugView::CsmCascade0; }

        if !input.play_mode && !input.editor_flying {
            let lmb = input.mouse_down(MouseButton::Left) && !input.editor_captured;
            if lmb {
                let (dx, dy) = input.mouse_delta;
                if self.dragging { self.camera.orbit(dx * 0.005, dy * 0.005); }
                self.dragging = true;
            } else { self.dragging = false; }
            if input.scroll_delta.abs() > 0.01 {
                self.camera.zoom(input.scroll_delta * 0.05);
            }
            let speed = 8.0 * dt;
            let mut pan = (0.0, 0.0);
            if input.key_down(KeyCode::KeyW) { pan.1 -= speed; }
            if input.key_down(KeyCode::KeyS) { pan.1 += speed; }
            if input.key_down(KeyCode::KeyA) { pan.0 -= speed; }
            if input.key_down(KeyCode::KeyD) { pan.0 += speed; }
            if pan != (0.0, 0.0) { self.camera.pan(pan.0, pan.1); }
        }

        self.orbit_phase += dt * 0.4;

        if !self.spawned {
            self.spawned = true;

            spawn_arena_ground(world);
            spawn_arena_walls(world);
            spawn_arena_center(world);
            spawn_arena_props(world);
            spawn_arena_cover(world);
            spawn_arena_elevator(world);
            spawn_arena_lighting(world);
            spawn_arena_decals(world);

            self.bell_entity = Some(spawn_arena_bell(world));

            log::info!("Spawned arena: {}×{} m, walls, platform, props",
                ARENA_HALF as u32 * 2, ARENA_HALF as u32 * 2);
        }

        if self.spawned && !self.ai_spawned {
            self.ai_spawned = true;
            self.ai_target_entity = Some(spawn_arena_ai_target(world));
            spawn_arena_ai(world);
            self.navmesh_bake_requested = true;
        }

        if let Some(t) = self.ai_target_entity {
            let p = self.camera.position();
            if let Some(tr) = world.get_mut::<Transform>(t) {
                tr.position = Vec3::new(p.x, p.y - 0.8, p.z);
            }
        }

        let finished: Vec<Entity> = world.read_events::<TimerFinished>()
            .map(|ev| ev.entity).collect();
        for e in finished {
            if Some(e) == self.bell_entity {
                if let Some(src) = world.get_mut::<AudioSource>(e) {
                    src.playing = true;
                }
                if let Some(t) = world.get_mut::<Timer>(e) { t.restart(); }
            }
        }

        if self.spawned {
            let player_pos = self.camera.position();
            let elevators: Vec<Entity> = world.query::<Elevator>().map(|(e, _)| e).collect();
            for e in elevators {
                let (sensor_r, center, half) = match (
                    world.get::<Elevator>(e).map(|el| el.sensor_radius),
                    world.get::<Transform>(e),
                ) {
                    (Some(r), Some(t)) => (r, t.position, t.scale.abs() * 0.5),
                    _ => continue,
                };
                let dx = player_pos.x - center.x;
                let dz = player_pos.z - center.z;
                let dist_xz = (dx * dx + dz * dz).sqrt();
                let in_xz = dist_xz < sensor_r + half.x.max(half.z);
                let dy = player_pos.y - center.y;
                let in_y = dy > -1.5 && dy < 3.5;
                if let Some(el) = world.get_mut::<Elevator>(e) {
                    el.player_inside = in_xz && in_y;
                }
            }
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
            let final_time = if looping && duration > 0.0 { new_time % duration } else { new_time.min(duration) };

            let Some(skel_handle) = world.get::<SkeletonHandle>(e).cloned() else { continue };
            let Some(skel) = self.skeletons.get(&skel_handle.0) else { continue };

            let local_pose = clip.local_pose(final_time, &skel.local_bind);
            let joint_matrices = skel.joint_matrices(&local_pose);
            renderer.update_skeleton(&skel_handle.0, &joint_matrices);
        }

        for sys in self.systems.iter_mut() {
            sys.update(world, dt);
        }

        let anim_events = self.animation_runtime.advance_all(
            world, renderer,
            &self.animations, &self.skeletons, &self.animation_events,
            dt,
        );
        for ev in anim_events {
            log::debug!("Anim event: {} on #{} (clip '{}', payload {:?})",
                ev.name, ev.entity, ev.clip, ev.payload);
            world.send(ev);
        }

        true
    }

    fn apply_postfx(&mut self, postfx: PostFx) { self.postfx = postfx; }

    fn on_play_enter(&mut self, _world: &World) -> Option<Box<dyn Any>> {
        Some(Box::new(()))
    }
    fn on_play_exit(&mut self, _state: Box<dyn Any>) {}

    fn save_game_state(&self) -> Option<String> { None }
    fn load_game_state(&mut self, _ron: &str) {}

    fn collect_draws(&mut self, world: &mut World, renderer: &Renderer) -> Vec<MeshDraw> {
        use std::collections::HashMap;
        type BucketKey = (String, String, [u32; 4], bool, bool, [u32; 2]);
        let mut buckets: HashMap<BucketKey, Vec<InstanceData>> = HashMap::new();
        let planes = self.camera.frustum_planes();
        let cam_pos = self.camera.position();

        let lod_bias = self.postfx.lod_bias.max(0.01);
        let lod_dists = self.postfx.lod_distances;
        let mut lod_counts = [0usize; 4];

        let mut new_prev: HashMap<Entity, glam::Mat4> = HashMap::new();

        let entities: Vec<_> = world.entities().to_vec();
        for e in entities {
            if let Some(v) = world.get::<Visible>(e) { if !v.0 { continue; } }
            if world.has::<Decal>(e) { continue; }

            let (Some(_t), Some(m), Some(mat)) = (
                world.get::<Transform>(e),
                world.get::<MeshHandle>(e),
                world.get::<MaterialHandle>(e),
            ) else { continue };

            let model = crate::game::world_matrix(world, e);
            let prev_model = self.prev_world_matrices.get(&e).copied().unwrap_or(model);
            new_prev.insert(e, model);

            let Some(mesh) = renderer.meshes.get(&m.0) else { continue };

            if self.show_culling {
                let (center, radius) = mesh.world_bounds(&model);
                if !sphere_in_frustum(center, radius, &planes) { continue; }
            }

            let (world_center, world_radius) = mesh.world_bounds(&model);
            let dist = (world_center - cam_pos).length();
            let dist_effective = (dist - world_radius).max(0.0) / lod_bias;

            let lod_level = if mesh.lods.is_empty() { 0 }
                else if dist_effective < lod_dists[0] { 0 }
                else if dist_effective < lod_dists[1] || mesh.lods.len() < 1 { 1 }
                else if dist_effective < lod_dists[2] || mesh.lods.len() < 2 { 2.min(mesh.lods.len()) }
                else { 3.min(mesh.lods.len()) };

            lod_counts[lod_level] += 1;

            let mesh_name = if lod_level == 0 { m.0.clone() }
                else { format!("{}__lod{}", m.0, lod_level - 1) };

            let material = renderer.materials.get(&mat.0).unwrap_or_else(|| renderer.materials_default());
            let blend = material.alpha_mode == AlphaMode::Blend;
            let double_sided = material.double_sided;

            let color = world.get::<Tint>(e).map(|t| t.0).unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let tiling_size = world.get::<TextureTiling>(e).map(|t| t.size).unwrap_or(1.0);
            let uv_scale = compute_uv_scale(mesh, &model, tiling_size);
            let uv_key = [
                (uv_scale[0] * 1000.0).round() as i32 as u32,
                (uv_scale[1] * 1000.0).round() as i32 as u32,
            ];

            let key = (mesh_name, mat.0.clone(), color.map(f32::to_bits), blend, double_sided, uv_key);
            let inst = InstanceData::new_full(model, prev_model, color, uv_scale);
            buckets.entry(key).or_default().push(inst);
        }

        self.prev_world_matrices = new_prev;
        self.lod_stats = lod_counts;

        buckets.into_iter().map(|((mesh, material_name, _, blend, double_sided, _), instances)| MeshDraw {
            mesh,
            instances,
            texture: Some(material_name),
            blend,
            double_sided,
        }).collect()
    }

    fn collect_lines(&mut self, world: &mut World, renderer: &Renderer, selected: &[Entity]) -> Vec<LineVertex> {
        let mut batch = LineBatch::new();
        if self.show_grid {
            batch.grid(40.0, 1.0, [0.15, 0.18, 0.22, 0.6], [0.35, 0.40, 0.48, 0.8], 5);
            batch.axes(3.0);
        }
        for &e in selected {
            if let Some(v) = world.get::<Visible>(e) { if !v.0 { continue; } }
            if let (Some(_t), Some(mh)) = (world.get::<Transform>(e), world.get::<MeshHandle>(e)) {
                if let Some(mesh) = renderer.meshes.get(&mh.0) {
                    let model = crate::game::world_matrix(world, e);
                    let (center, radius) = mesh.world_bounds(&model);
                    batch.sphere_wireframe(center, radius * 1.05, [1.0, 0.85, 0.2, 1.0], 24);
                }
            }
        }

        for (e, agent) in world.query::<AiAgent>() {
            if !world.has::<DebugPath>(e) { continue; }
            if agent.path.len() < 2 { continue; }
            for w in agent.path.windows(2) {
                batch.line(
                    w[0] + Vec3::Y * 0.1,
                    w[1] + Vec3::Y * 0.1,
                    [1.0, 0.9, 0.2, 0.9],
                );
            }
        }

        for (e, agent) in world.query::<AiAgent>() {
            let Some(t) = world.get::<Transform>(e) else { continue };
            let eye = t.position + Vec3::Y * 1.0;
            let fwd = Vec3::new(-agent.yaw.sin(), 0.0, -agent.yaw.cos());
            let color = match agent.state {
                AiState::Idle | AiState::Patrol => [0.4, 0.9, 0.4, 0.7],
                AiState::Investigate => [0.9, 0.7, 0.3, 0.8],
                AiState::Chase => [1.0, 0.8, 0.2, 0.8],
                AiState::Attack => [1.0, 0.3, 0.3, 0.9],
                AiState::Dead => [0.4, 0.4, 0.4, 0.4],
            };
            let range = agent.vision_range.min(8.0);
            let half = agent.vision_angle_cos.acos();
            batch.line(eye, eye + fwd * range, color);
            let left = Quat::from_axis_angle(Vec3::Y, half) * fwd;
            let right = Quat::from_axis_angle(Vec3::Y, -half) * fwd;
            batch.line(eye, eye + left * range, color);
            batch.line(eye, eye + right * range, color);
        }

        batch.vertices().to_vec()
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

    fn ambient(&self) -> [f32; 3] { [0.04, 0.045, 0.06] }
    fn postfx(&self) -> PostFx { self.postfx }
    fn camera(&self) -> &Camera3D { &self.camera }
    fn camera_mut(&mut self) -> &mut Camera3D { &mut self.camera }

    fn rpg_hud(&self) -> Vec<(String, String)> { Vec::new() }
    fn on_kill(&mut self, _world: &mut World, _target: Entity) {}
}

fn main() {
    run(DemoGame::new());
}