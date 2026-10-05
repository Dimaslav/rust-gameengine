//! Сцена: снимок всех сущностей в RON + spawn игрока.
//!
//! Две пары API:
//!
//! * `save_scene_to_*` / `load_scene_from_*` — только World.
//!   Используются там, где Renderer недоступен или не нужен
//!   (undo/redo, Play-in-Editor snapshot).
//!
//! * `save_scene_with_assets_to_*` / `load_scene_with_assets_from_*` —
//!   World + материалы + пути к текстурам. Используются для Save/Load
//!   файла сцены пользователем.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

use crate::ecs::{Entity, World};
use crate::game::components::{
    AnimationPlayer, Chase, Elevator, ElevatorState, Health, Interactable, MaterialHandle,
    MeshHandle, Name, Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint,
    Transform, Trigger, TriggerAction, Velocity, Visible,
};
use crate::game::lights::{DirectionalLight, PointLight};
use crate::physics::{Collider, PhysicsMaterial, RigidBody};
use crate::render::{Material, Renderer};
use glam::Vec3;

#[derive(Serialize, Deserialize, Default)]
pub struct SceneFile {
    #[serde(default)]
    pub entities: Vec<EntitySnapshot>,
    #[serde(default)]
    pub player_spawn: Option<[f32; 3]>,

    /// Материалы, на которые ссылаются entity сцены.
    #[serde(default)]
    pub materials: HashMap<String, Material>,

    /// Пути к файлам текстур для материалов. Ключ — имя текстуры,
    /// значение — путь на диске. Процедурные текстуры (`checker`)
    /// здесь отсутствуют — они создаются при старте движка.
    #[serde(default)]
    pub texture_paths: HashMap<String, String>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct EntitySnapshot {
    #[serde(default)] pub entity_id: Option<u32>,
    #[serde(default)] pub name: Option<String>,
    #[serde(default)] pub transform: Option<TransformSnapshot>,
    #[serde(default)] pub parent: Option<u32>,
    #[serde(default)] pub mesh: Option<String>,
    #[serde(default)] pub material: Option<String>,
    #[serde(default)] pub skeleton: Option<String>,
    #[serde(default)] pub animation: Option<AnimationSnapshot>,
    #[serde(default)] pub spinner: Option<SpinnerSnapshot>,
    #[serde(default)] pub velocity: Option<VelocitySnapshot>,
    #[serde(default)] pub health: Option<HealthSnapshot>,
    #[serde(default)] pub chase: Option<ChaseSnapshot>,
    #[serde(default)] pub trigger: Option<TriggerSnapshot>,
    #[serde(default)] pub interactable: Option<InteractableSnapshot>,
    #[serde(default)] pub tint: Option<[f32; 4]>,
    #[serde(default)] pub visible: Option<bool>,
    #[serde(default)] pub texture_tiling: Option<f32>,
    #[serde(default)] pub elevator: Option<ElevatorSnapshot>,
    #[serde(default)] pub sliding_door: Option<SlidingDoorSnapshot>,

    #[serde(default)] pub rigid_body: Option<RigidBody>,
    #[serde(default)] pub collider: Option<Collider>,
    #[serde(default)] pub physics_material: Option<PhysicsMaterial>,

    #[serde(default)] pub dir_light: Option<DirectionalLightSnapshot>,
    #[serde(default)] pub point_light: Option<PointLightSnapshot>,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct TransformSnapshot {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
}

#[derive(Serialize, Deserialize, Clone)]
pub struct AnimationSnapshot {
    pub clip: String,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct SpinnerSnapshot { pub axis: [f32; 3], pub speed: f32 }

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct VelocitySnapshot { pub value: [f32; 3] }

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct HealthSnapshot { pub current: f32, pub max: f32 }

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct ChaseSnapshot { pub speed: f32, pub stop_distance: f32 }

#[derive(Serialize, Deserialize, Clone)]
pub struct TriggerSnapshot {
    pub radius: f32,
    pub action: String,
    pub param: [f32; 4],
    pub once: bool,
    pub fired: bool,
    #[serde(default)]
    pub sound_name: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InteractableSnapshot {
    pub kind: String,
    pub color: Option<[f32; 4]>,
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ElevatorSnapshot {
    pub floors: Vec<f32>,
    pub current_floor: u32,
    pub target_floor: u32,
    pub speed: f32,
    #[serde(default)] pub acceleration: f32,
    #[serde(default)] pub current_velocity: f32,
    pub state: String,
    pub doors_open: f32,
    pub door_speed: f32,
    pub dwell: f32,
    pub dwell_timer: f32,
    #[serde(default)] pub sensor_radius: f32,
}

impl ElevatorSnapshot {
    fn from_elevator(e: &Elevator) -> Self {
        let state = match e.state {
            ElevatorState::Idle => "idle",
            ElevatorState::Moving => "moving",
            ElevatorState::DoorsOpening => "doors_opening",
            ElevatorState::DoorsOpen => "doors_open",
            ElevatorState::DoorsClosing => "doors_closing",
        };
        Self {
            floors: e.floors.clone(),
            current_floor: e.current_floor as u32,
            target_floor: e.target_floor as u32,
            speed: e.speed,
            acceleration: e.acceleration,
            current_velocity: e.current_velocity,
            state: state.to_string(),
            doors_open: e.doors_open,
            door_speed: e.door_speed,
            dwell: e.dwell,
            dwell_timer: e.dwell_timer,
            sensor_radius: e.sensor_radius,
        }
    }

    fn to_elevator(&self) -> Elevator {
        let state = match self.state.as_str() {
            "moving" => ElevatorState::Moving,
            "doors_opening" => ElevatorState::DoorsOpening,
            "doors_open" => ElevatorState::DoorsOpen,
            "doors_closing" => ElevatorState::DoorsClosing,
            _ => ElevatorState::Idle,
        };
        let mut el = Elevator::new(self.floors.clone(), self.speed);
        el.current_floor = self.current_floor as usize;
        el.target_floor = self.target_floor as usize;
        el.state = state;
        el.doors_open = self.doors_open;
        el.door_speed = self.door_speed;
        el.dwell = self.dwell;
        el.dwell_timer = self.dwell_timer;
        el.acceleration = if self.acceleration > 0.0 { self.acceleration } else { 3.0 };
        el.current_velocity = self.current_velocity;
        el.sensor_radius = if self.sensor_radius > 0.0 { self.sensor_radius } else { 2.0 };
        el.player_inside = false;
        el
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct SlidingDoorSnapshot {
    pub open_amount: f32,
    pub target: f32,
    pub speed: f32,
    pub slide_axis: [f32; 3],
    pub slide_distance: f32,
    pub closed_position: [f32; 3],
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct DirectionalLightSnapshot {
    pub direction: [f32; 3],
    pub color: [f32; 3],
    pub intensity: f32,
}

impl DirectionalLightSnapshot {
    fn from_light(l: &DirectionalLight) -> Self {
        Self {
            direction: l.direction.to_array(),
            color: l.color,
            intensity: l.intensity,
        }
    }
    fn to_light(&self) -> DirectionalLight {
        DirectionalLight {
            direction: Vec3::from_array(self.direction),
            color: self.color,
            intensity: self.intensity,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct PointLightSnapshot {
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

impl PointLightSnapshot {
    fn from_light(l: &PointLight) -> Self {
        Self {
            color: l.color,
            intensity: l.intensity,
            range: l.range,
        }
    }
    fn to_light(&self) -> PointLight {
        PointLight::new(self.color, self.intensity, self.range)
    }
}

// ============================================================
// Save (World only, без материалов)
// ============================================================

pub fn save_scene_to_string(world: &World, player_spawn: Option<Vec3>) -> Result<String> {
    let mut entities = Vec::new();
    for &e in world.entities() {
        if let Some(snap) = snapshot_entity(world, e) {
            entities.push(snap);
        }
    }
    let file = SceneFile {
        entities,
        player_spawn: player_spawn.map(|p| p.to_array()),
        materials: HashMap::new(),
        texture_paths: HashMap::new(),
    };
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .context("serialize scene")?;
    Ok(text)
}

pub fn save_scene_to_file(
    world: &World,
    path: impl AsRef<Path>,
    player_spawn: Option<Vec3>,
) -> Result<()> {
    let text = save_scene_to_string(world, player_spawn)?;
    std::fs::write(path.as_ref(), text)
        .with_context(|| format!("write scene to {}", path.as_ref().display()))?;
    Ok(())
}

// ============================================================
// Save (World + материалы + текстуры из Renderer)
// ============================================================

/// Собрать материалы, на которые ссылаются entity сцены.
fn collect_materials(world: &World, renderer: &Renderer) -> HashMap<String, Material> {
    let mut used: HashSet<String> = HashSet::new();
    for &e in world.entities() {
        if let Some(mh) = world.get::<MaterialHandle>(e) {
            used.insert(mh.0.clone());
        }
    }

    let mut out = HashMap::new();
    for name in used {
        if let Some(mat) = renderer.materials.get(&name) {
            out.insert(name, mat.clone());
        }
    }
    out
}

/// Собрать пути ко всем текстурам, на которые ссылаются материалы.
///
/// Процедурные текстуры (без `source_path`) пропускаются — они
/// создаются при старте движка и не нуждаются в перезагрузке.
fn collect_texture_paths(
    renderer: &Renderer,
    materials: &HashMap<String, Material>,
) -> HashMap<String, String> {
    let mut out = HashMap::new();
    for mat in materials.values() {
        for tex_name in mat.referenced_textures() {
            if out.contains_key(tex_name) {
                continue;
            }
            if let Some(path) = renderer.texture_source_path(tex_name) {
                out.insert(tex_name.to_string(), path.to_string());
            }
        }
    }
    out
}

pub fn save_scene_with_assets_to_string(
    world: &World,
    renderer: &Renderer,
    player_spawn: Option<Vec3>,
) -> Result<String> {
    let mut entities = Vec::new();
    for &e in world.entities() {
        if let Some(snap) = snapshot_entity(world, e) {
            entities.push(snap);
        }
    }
    let materials = collect_materials(world, renderer);
    let texture_paths = collect_texture_paths(renderer, &materials);

    let file = SceneFile {
        entities,
        player_spawn: player_spawn.map(|p| p.to_array()),
        materials,
        texture_paths,
    };
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .context("serialize scene")?;
    Ok(text)
}

pub fn save_scene_with_assets_to_file(
    world: &World,
    renderer: &Renderer,
    path: impl AsRef<Path>,
    player_spawn: Option<Vec3>,
) -> Result<()> {
    let text = save_scene_with_assets_to_string(world, renderer, player_spawn)?;
    std::fs::write(path.as_ref(), text)
        .with_context(|| format!("write scene to {}", path.as_ref().display()))?;
    Ok(())
}

pub fn snapshot_entity(world: &World, e: Entity) -> Option<EntitySnapshot> {
    let mut s = EntitySnapshot::default();
    s.entity_id = Some(e);
    let mut any = false;

    if let Some(n) = world.get::<Name>(e) { s.name = Some(n.0.clone()); any = true; }
    if let Some(t) = world.get::<Transform>(e) {
        s.transform = Some(TransformSnapshot {
            position: t.position.to_array(),
            rotation: t.rotation.to_array(),
            scale: t.scale.to_array(),
        });
        any = true;
    }
    if let Some(Parent(p)) = world.get::<Parent>(e).copied() { s.parent = Some(p); any = true; }
    if let Some(m) = world.get::<MeshHandle>(e) { s.mesh = Some(m.0.clone()); any = true; }
    if let Some(m) = world.get::<MaterialHandle>(e) { s.material = Some(m.0.clone()); any = true; }
    if let Some(sk) = world.get::<SkeletonHandle>(e) { s.skeleton = Some(sk.0.clone()); any = true; }
    if let Some(a) = world.get::<AnimationPlayer>(e) {
        s.animation = Some(AnimationSnapshot {
            clip: a.clip.clone(), time: a.time, speed: a.speed, looping: a.looping,
        });
        any = true;
    }
    if let Some(sp) = world.get::<Spinner>(e) {
        s.spinner = Some(SpinnerSnapshot { axis: sp.axis.to_array(), speed: sp.speed });
        any = true;
    }
    if let Some(v) = world.get::<Velocity>(e) {
        s.velocity = Some(VelocitySnapshot { value: v.value.to_array() });
        any = true;
    }
    if let Some(h) = world.get::<Health>(e) {
        s.health = Some(HealthSnapshot { current: h.current, max: h.max });
        any = true;
    }
    if let Some(c) = world.get::<Chase>(e) {
        s.chase = Some(ChaseSnapshot { speed: c.speed, stop_distance: c.stop_distance });
        any = true;
    }
    if let Some(t) = world.get::<Trigger>(e) {
        let (kind, param, sound_name) = match &t.action {
            TriggerAction::Teleport(p) => ("teleport".to_string(), [p[0], p[1], p[2], 0.0], None),
            TriggerAction::Tint(c) => ("tint".to_string(), *c, None),
            TriggerAction::Despawn => ("despawn".to_string(), [0.0; 4], None),
            TriggerAction::CallElevator { elevator, floor_idx } => (
                "call_elevator".to_string(),
                [*elevator as f32, *floor_idx as f32, 0.0, 0.0],
                None,
            ),
            TriggerAction::PlaySound(name) => (
                "play_sound".to_string(),
                [0.0; 4],
                Some(name.clone()),
            ),
        };
        s.trigger = Some(TriggerSnapshot {
            radius: t.radius, action: kind, param, once: t.once, fired: t.fired, sound_name,
        });
        any = true;
    }
    if let Some(i) = world.get::<Interactable>(e) {
        let (kind, color) = match i {
            Interactable::Pickup => ("pickup".to_string(), None),
            Interactable::Paint(c) => ("paint".to_string(), Some(*c)),
            Interactable::Toggle => ("toggle".to_string(), None),
        };
        s.interactable = Some(InteractableSnapshot { kind, color });
        any = true;
    }
    if let Some(t) = world.get::<Tint>(e) { s.tint = Some(t.0); any = true; }
    if let Some(v) = world.get::<Visible>(e) { s.visible = Some(v.0); any = true; }
    if let Some(t) = world.get::<TextureTiling>(e) {
        s.texture_tiling = Some(t.size);
        any = true;
    }
    if let Some(el) = world.get::<Elevator>(e) {
        s.elevator = Some(ElevatorSnapshot::from_elevator(el));
        any = true;
    }
    if let Some(sd) = world.get::<SlidingDoor>(e) {
        s.sliding_door = Some(SlidingDoorSnapshot {
            open_amount: sd.open_amount,
            target: sd.target,
            speed: sd.speed,
            slide_axis: sd.slide_axis.to_array(),
            slide_distance: sd.slide_distance,
            closed_position: sd.closed_position.to_array(),
        });
        any = true;
    }

    if let Some(rb) = world.get::<RigidBody>(e) { s.rigid_body = Some(*rb); any = true; }
    if let Some(col) = world.get::<Collider>(e) { s.collider = Some(*col); any = true; }
    if let Some(mat) = world.get::<PhysicsMaterial>(e) { s.physics_material = Some(*mat); any = true; }

    if let Some(l) = world.get::<DirectionalLight>(e) {
        s.dir_light = Some(DirectionalLightSnapshot::from_light(l));
        any = true;
    }
    if let Some(l) = world.get::<PointLight>(e) {
        s.point_light = Some(PointLightSnapshot::from_light(l));
        any = true;
    }

    if any { Some(s) } else { None }
}

// ============================================================
// Load (World only)
// ============================================================

pub fn load_scene_from_str(text: &str) -> Result<(World, Option<Vec3>)> {
    let (world, spawn, _) = load_scene_from_str_full(text)?;
    Ok((world, spawn))
}

pub fn load_scene_from_str_full(
    text: &str,
) -> Result<(World, Option<Vec3>, HashMap<u32, Entity>)> {
    let file: SceneFile = ron::from_str(text).context("parse RON scene")?;

    let mut world = World::new();
    let id_map = spawn_all_entities(&mut world, file.entities);
    fix_call_elevator_refs(&mut world, &id_map);

    let spawn = file.player_spawn.map(Vec3::from_array);
    Ok((world, spawn, id_map))
}

pub fn load_scene_from_file(path: impl AsRef<Path>) -> Result<(World, Option<Vec3>)> {
    let text = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("read scene {}", path.as_ref().display()))?;
    load_scene_from_str(&text)
}

// ============================================================
// Load (World + материалы + текстуры в Renderer)
// ============================================================

/// Загрузить сцену вместе с материалами и текстурами.
///
/// Порядок:
///   1. Парсим SceneFile.
///   2. Регистрируем текстуры в Renderer — только те, у которых
///      сохранён `path` (процедурные уже есть в памяти).
///   3. Регистрируем материалы (с уже подгруженными текстурами).
///   4. Спавним entity.
pub fn load_scene_with_assets_from_str(
    text: &str,
    renderer: &mut Renderer,
) -> Result<(World, Option<Vec3>)> {
    let file: SceneFile = ron::from_str(text).context("parse RON scene")?;

    // ШАГ 1. Текстуры. Загружаем с диска те, что сохранили путь.
    // Если текстура уже есть в Renderer (например, встроенная
    // `checker`) — не перезагружаем.
    let mut restored_tex = 0usize;
    let mut failed_tex = 0usize;
    for (name, path) in &file.texture_paths {
        if renderer.textures.contains_key(name) {
            continue;
        }
        match renderer.load_texture(name, path) {
            Ok(()) => restored_tex += 1,
            Err(e) => {
                log::warn!(
                    "Scene load: texture '{}' from '{}' failed: {}",
                    name, path, e
                );
                failed_tex += 1;
            }
        }
    }
    if restored_tex + failed_tex > 0 {
        log::info!(
            "Scene load: {} textures restored, {} failed",
            restored_tex, failed_tex
        );
    }

    // ШАГ 2. Материалы. Ссылаются на уже подгруженные текстуры.
    for (name, material) in &file.materials {
        if renderer.has_material(name) {
            renderer.update_material(name, material.clone());
        } else {
            renderer.add_material(name.clone(), material.clone());
        }
    }
    if !file.materials.is_empty() {
        log::info!(
            "Scene load: {} materials restored",
            file.materials.len()
        );
    }

    // ШАГ 3. Entity.
    let mut world = World::new();
    let id_map = spawn_all_entities(&mut world, file.entities);
    fix_call_elevator_refs(&mut world, &id_map);

    let spawn = file.player_spawn.map(Vec3::from_array);
    Ok((world, spawn))
}

pub fn load_scene_with_assets_from_file(
    path: impl AsRef<Path>,
    renderer: &mut Renderer,
) -> Result<(World, Option<Vec3>)> {
    let text = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("read scene {}", path.as_ref().display()))?;
    load_scene_with_assets_from_str(&text, renderer)
}

// ============================================================
// Общие helper'ы
// ============================================================

fn spawn_all_entities(
    world: &mut World,
    entities: Vec<EntitySnapshot>,
) -> HashMap<u32, Entity> {
    let mut id_map: HashMap<u32, Entity> = HashMap::with_capacity(entities.len());
    let mut pending_parent: Vec<(Entity, Option<u32>)> = Vec::with_capacity(entities.len());

    for snap in entities {
        let old_id = snap.entity_id;
        let old_parent = snap.parent;
        let new_e = spawn_snapshot(world, snap);
        if let Some(old) = old_id {
            id_map.insert(old, new_e);
        }
        pending_parent.push((new_e, old_parent));
    }

    for (new_e, old_parent) in pending_parent {
        if let Some(old_p) = old_parent {
            if let Some(&new_p) = id_map.get(&old_p) {
                world.insert(new_e, Parent(new_p));
            }
        }
    }

    id_map
}

fn fix_call_elevator_refs(world: &mut World, id_map: &HashMap<u32, Entity>) {
    let trigger_entities: Vec<Entity> =
        world.query::<Trigger>().map(|(e, _)| e).collect();
    for e in trigger_entities {
        let Some(mut t) = world.get::<Trigger>(e).cloned() else { continue };
        if let TriggerAction::CallElevator { elevator, floor_idx } = t.action {
            if let Some(&new_el) = id_map.get(&elevator) {
                t.action = TriggerAction::CallElevator { elevator: new_el, floor_idx };
                world.insert(e, t);
            }
        }
    }
}

pub fn spawn_snapshot(world: &mut World, snap: EntitySnapshot) -> Entity {
    let e = world.spawn();

    if let Some(n) = snap.name { world.insert(e, Name(n)); }
    if let Some(t) = snap.transform {
        world.insert(e, Transform {
            position: glam::Vec3::from_array(t.position),
            rotation: glam::Quat::from_array(t.rotation),
            scale: glam::Vec3::from_array(t.scale),
        });
    }
    if let Some(m) = snap.mesh { world.insert(e, MeshHandle(m)); }
    if let Some(m) = snap.material { world.insert(e, MaterialHandle(m)); }
    if let Some(s) = snap.skeleton { world.insert(e, SkeletonHandle(s)); }
    if let Some(a) = snap.animation {
        world.insert(e, AnimationPlayer {
            clip: a.clip, time: a.time, speed: a.speed, looping: a.looping,
        });
    }
    if let Some(sp) = snap.spinner {
        world.insert(e, Spinner { axis: glam::Vec3::from_array(sp.axis), speed: sp.speed });
    }
    if let Some(v) = snap.velocity {
        world.insert(e, Velocity { value: glam::Vec3::from_array(v.value) });
    }
    if let Some(h) = snap.health {
        world.insert(e, Health { current: h.current, max: h.max });
    }
    if let Some(c) = snap.chase {
        world.insert(e, Chase { speed: c.speed, stop_distance: c.stop_distance });
    }
    if let Some(t) = snap.trigger {
        let action = match t.action.as_str() {
            "teleport" => TriggerAction::Teleport([t.param[0], t.param[1], t.param[2]]),
            "tint" => TriggerAction::Tint(t.param),
            "despawn" => TriggerAction::Despawn,
            "call_elevator" => TriggerAction::CallElevator {
                elevator: t.param[0] as u32,
                floor_idx: t.param[1] as u32,
            },
            "play_sound" => TriggerAction::PlaySound(
                t.sound_name.unwrap_or_else(|| "pickup".to_string())
            ),
            _ => TriggerAction::Despawn,
        };
        world.insert(e, Trigger {
            radius: t.radius, action, once: t.once, fired: t.fired,
        });
    }
    if let Some(i) = snap.interactable {
        let kind = match i.kind.as_str() {
            "pickup" => Interactable::Pickup,
            "paint" => Interactable::Paint(i.color.unwrap_or([1.0, 0.0, 0.0, 1.0])),
            "toggle" => Interactable::Toggle,
            _ => Interactable::Pickup,
        };
        world.insert(e, kind);
    }
    if let Some(t) = snap.tint { world.insert(e, Tint(t)); }
    if let Some(v) = snap.visible { world.insert(e, Visible(v)); }
    if let Some(size) = snap.texture_tiling {
        world.insert(e, TextureTiling::new(size));
    }
    if let Some(el) = snap.elevator {
        world.insert(e, el.to_elevator());
    }
    if let Some(sd) = snap.sliding_door {
        world.insert(e, SlidingDoor {
            open_amount: sd.open_amount,
            target: sd.target,
            speed: sd.speed,
            slide_axis: glam::Vec3::from_array(sd.slide_axis),
            slide_distance: sd.slide_distance,
            closed_position: glam::Vec3::from_array(sd.closed_position),
        });
    }

    if let Some(rb) = snap.rigid_body { world.insert(e, rb); }
    if let Some(col) = snap.collider { world.insert(e, col); }
    if let Some(mat) = snap.physics_material { world.insert(e, mat); }

    if let Some(l) = snap.dir_light { world.insert(e, l.to_light()); }
    if let Some(l) = snap.point_light { world.insert(e, l.to_light()); }

    e
}