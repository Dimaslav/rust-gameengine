//! Сцена: снимок всех сущностей в RON.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use crate::ecs::{Entity, World};
use crate::game::components::{
    AnimationPlayer, MaterialHandle, MeshHandle, Name, SkeletonHandle, Spinner, Transform,
    Velocity,
};

#[derive(Serialize, Deserialize, Default)]
pub struct SceneFile {
    #[serde(default)]
    pub entities: Vec<EntitySnapshot>,
}

#[derive(Serialize, Deserialize, Default)]
pub struct EntitySnapshot {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub transform: Option<TransformSnapshot>,
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
pub struct SpinnerSnapshot {
    pub axis: [f32; 3],
    pub speed: f32,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
pub struct VelocitySnapshot {
    pub value: [f32; 3],
}

pub fn save_scene_to_string(world: &World) -> Result<String> {
    let mut entities = Vec::new();
    for &e in world.entities() {
        if let Some(snap) = snapshot_entity(world, e) {
            entities.push(snap);
        }
    }
    let file = SceneFile { entities };
    let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())
        .context("serialize scene")?;
    Ok(text)
}

pub fn save_scene_to_file(world: &World, path: impl AsRef<Path>) -> Result<()> {
    let text = save_scene_to_string(world)?;
    std::fs::write(path.as_ref(), text)
        .with_context(|| format!("write scene to {}", path.as_ref().display()))?;
    Ok(())
}

fn snapshot_entity(world: &World, e: Entity) -> Option<EntitySnapshot> {
    let mut s = EntitySnapshot::default();
    let mut any = false;

    if let Some(n) = world.get::<Name>(e) {
        s.name = Some(n.0.clone());
        any = true;
    }
    if let Some(t) = world.get::<Transform>(e) {
        s.transform = Some(TransformSnapshot {
            position: t.position.to_array(),
            rotation: t.rotation.to_array(),
            scale: t.scale.to_array(),
        });
        any = true;
    }
    if let Some(m) = world.get::<MeshHandle>(e) {
        s.mesh = Some(m.0.clone());
        any = true;
    }
    if let Some(m) = world.get::<MaterialHandle>(e) {
        s.material = Some(m.0.clone());
        any = true;
    }
    if let Some(sk) = world.get::<SkeletonHandle>(e) {
        s.skeleton = Some(sk.0.clone());
        any = true;
    }
    if let Some(a) = world.get::<AnimationPlayer>(e) {
        s.animation = Some(AnimationSnapshot {
            clip: a.clip.clone(),
            time: a.time,
            speed: a.speed,
            looping: a.looping,
        });
        any = true;
    }
    if let Some(sp) = world.get::<Spinner>(e) {
        s.spinner = Some(SpinnerSnapshot {
            axis: sp.axis.to_array(),
            speed: sp.speed,
        });
        any = true;
    }
    if let Some(v) = world.get::<Velocity>(e) {
        s.velocity = Some(VelocitySnapshot {
            value: v.value.to_array(),
        });
        any = true;
    }

    if any {
        Some(s)
    } else {
        None
    }
}

pub fn load_scene_from_str(text: &str) -> Result<World> {
    let file: SceneFile = ron::from_str(text).context("parse RON scene")?;
    let mut world = World::new();
    for snap in file.entities {
        spawn_snapshot(&mut world, snap);
    }
    Ok(world)
}

pub fn load_scene_from_file(path: impl AsRef<Path>) -> Result<World> {
    let text = std::fs::read_to_string(path.as_ref())
        .with_context(|| format!("read scene {}", path.as_ref().display()))?;
    load_scene_from_str(&text)
}

fn spawn_snapshot(world: &mut World, snap: EntitySnapshot) {
    let e = world.spawn();

    if let Some(n) = snap.name {
        world.insert(e, Name(n));
    }
    if let Some(t) = snap.transform {
        world.insert(
            e,
            Transform {
                position: glam::Vec3::from_array(t.position),
                rotation: glam::Quat::from_array(t.rotation),
                scale: glam::Vec3::from_array(t.scale),
            },
        );
    }
    if let Some(m) = snap.mesh {
        world.insert(e, MeshHandle(m));
    }
    if let Some(m) = snap.material {
        world.insert(e, MaterialHandle(m));
    }
    if let Some(s) = snap.skeleton {
        world.insert(e, SkeletonHandle(s));
    }
    if let Some(a) = snap.animation {
        world.insert(
            e,
            AnimationPlayer {
                clip: a.clip,
                time: a.time,
                speed: a.speed,
                looping: a.looping,
            },
        );
    }
    if let Some(sp) = snap.spinner {
        world.insert(
            e,
            Spinner {
                axis: glam::Vec3::from_array(sp.axis),
                speed: sp.speed,
            },
        );
    }
    if let Some(v) = snap.velocity {
        world.insert(
            e,
            Velocity {
                value: glam::Vec3::from_array(v.value),
            },
        );
    }
}