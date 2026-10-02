//! Сцена: снимок всех сущностей в RON + spawn игрока.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::ecs::{Entity, World};
use crate::game::components::{
    AnimationPlayer, Chase, Health, Interactable, MaterialHandle, MeshHandle, Name, Parent,
    SkeletonHandle, Spinner, Tint, Transform, Trigger, TriggerAction, Velocity, Visible,
};
use glam::Vec3;

#[derive(Serialize, Deserialize, Default)]
pub struct SceneFile {
    #[serde(default)]
    pub entities: Vec<EntitySnapshot>,
    #[serde(default)]
    pub player_spawn: Option<[f32; 3]>,
}

#[derive(Serialize, Deserialize, Default, Clone)]
pub struct EntitySnapshot {
    /// Оригинальный entity id. Нужен для ремапа `Parent` при загрузке.
    /// У старых файлов отсутствует — тогда Parent просто отбрасывается.
    #[serde(default)]
    pub entity_id: Option<u32>,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub transform: Option<TransformSnapshot>,
    /// Ссылка на оригинальный entity id родителя (в терминах `entity_id`).
    #[serde(default)]
    pub parent: Option<u32>,
    #[serde(default)]
    pub mesh: Option<String>,
    #[serde(default)]
    pub material: Option<String>,
    #[serde(default)]
    pub skeleton: Option<String>,
    #[serde(default)]
    pub animation: Option<AnimationSnapshot>,
    #[serde(default)]
    pub spinner: Option<SpinnerSnapshot>,
    #[serde(default)]
    pub velocity: Option<VelocitySnapshot>,
    #[serde(default)]
    pub health: Option<HealthSnapshot>,
    #[serde(default)]
    pub chase: Option<ChaseSnapshot>,
    #[serde(default)]
    pub trigger: Option<TriggerSnapshot>,
    #[serde(default)]
    pub interactable: Option<InteractableSnapshot>,
    #[serde(default)]
    pub tint: Option<[f32; 4]>,
    #[serde(default)]
    pub visible: Option<bool>,
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
}

#[derive(Serialize, Deserialize, Clone)]
pub struct InteractableSnapshot {
    pub kind: String,
    pub color: Option<[f32; 4]>,
}

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
        let (kind, param) = match &t.action {
            TriggerAction::Teleport(p) => ("teleport".to_string(), [p[0], p[1], p[2], 0.0]),
            TriggerAction::Tint(c) => ("tint".to_string(), *c),
            TriggerAction::Despawn => ("despawn".to_string(), [0.0; 4]),
        };
        s.trigger = Some(TriggerSnapshot {
            radius: t.radius, action: kind, param, once: t.once, fired: t.fired,
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

    // Важно: entity_id устанавливается ВСЕГДА, но он не считается
    // «компонентом» — сущности без компонентов в снимок не попадают.
    if any { Some(s) } else { None }
}

/// Загрузка сцены: two-pass, чтобы корректно перепривязать `Parent`.
pub fn load_scene_from_str(text: &str) -> Result<(World, Option<Vec3>)> {
    let file: SceneFile = ron::from_str(text).context("parse RON scene")?;
    let mut world = World::new();

    // Pass 1: spawn всех, построить old_id → new_id.
    let mut id_map: HashMap<u32, Entity> = HashMap::with_capacity(file.entities.len());
    let mut pending_parent: Vec<(Entity, Option<u32>)> = Vec::with_capacity(file.entities.len());

    for snap in file.entities {
        let old_id = snap.entity_id;
        let old_parent = snap.parent;
        let new_e = spawn_snapshot(&mut world, snap);
        if let Some(old) = old_id {
            id_map.insert(old, new_e);
        }
        pending_parent.push((new_e, old_parent));
    }

    // Pass 2: перепривязать Parent через маппинг. Если old_id отсутствует
    // (старый файл) — Parent отбрасывается: указывать было бы некуда.
    for (new_e, old_parent) in pending_parent {
        if let Some(old_p) = old_parent {
            if let Some(&new_p) = id_map.get(&old_p) {
                world.insert(new_e, Parent(new_p));
            }
        }
    }

    let spawn = file.player_spawn.map(Vec3::from_array);
    Ok((world, spawn))
}

pub fn load_scene_from_file(path: impl AsRef<Path>) -> Result<(World, Option<Vec3>)> {
    let text = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("read scene {}", path.as_ref().display()))?;
    load_scene_from_str(&text)
}

/// Спавнит entity из снимка. `Parent` НЕ ставится — им управляет вызывающий
/// (через two-pass, где есть маппинг old→new).
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
    // Parent — НЕ здесь. См. load_scene_from_str / prefab::instantiate_prefab.
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

    e
}