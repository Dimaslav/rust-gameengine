use anyhow::Result;
use glam::Vec2;
use std::path::Path;

use crate::ecs::World;
use crate::game::components::{Sprite, Spinner, Transform, Velocity};
use super::prefab::SceneDef;

pub fn load_scene_from_str(world: &mut World, text: &str) -> Result<usize> {
    let scene: SceneDef = ron::from_str(text)?;
    Ok(spawn_scene(world, &scene))
}

pub fn load_scene_from_file(world: &mut World, path: impl AsRef<Path>) -> Result<usize> {
    let text = std::fs::read_to_string(path)?;
    load_scene_from_str(world, &text)
}

pub fn spawn_scene(world: &mut World, scene: &SceneDef) -> usize {
    let mut count = 0;
    for def in &scene.entities {
        let e = world.spawn();

        if let Some(t) = &def.transform {
            let mut tr = Transform::new(t.position[0], t.position[1]);
            tr.rotation = t.rotation;
            tr.scale = Vec2::new(t.scale[0], t.scale[1]);
            world.insert(e, tr);
        }

        if let Some(s) = &def.sprite {
            let mut sp = Sprite::new(s.texture.clone(), s.width, s.height);
            if let Some(c) = s.color { sp.color = c; }
            if let Some(z) = s.z { sp.z = z; }
            if let Some(uv) = s.uv_rect { sp.uv_rect = uv; }
            world.insert(e, sp);
        }

        if let Some(v) = &def.velocity {
            world.insert(e, Velocity::new(v.x, v.y));
        }

        if let Some(sp) = &def.spinner {
            world.insert(e, Spinner::new(sp.speed));
        }

        count += 1;
    }
    count
}