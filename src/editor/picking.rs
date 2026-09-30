//! Picking: луч из камеры → ближайшая сущность под курсором.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::{MeshHandle, Transform};
use crate::render::{Camera3D, Renderer};

/// Возвращает ближайшую сущность под пикселем `(screen_x, screen_y)`.
/// Пересечение — со сферой `Mesh::world_bounds` каждой сущности.
pub fn pick_entity(
    world: &World,
    renderer: &Renderer,
    camera: &Camera3D,
    screen_x: f32,
    screen_y: f32,
) -> Option<Entity> {
    let (origin, dir) = camera.ray_from_screen(
        screen_x,
        screen_y,
        renderer.size.width as f32,
        renderer.size.height as f32,
    );

    let mut best: Option<(Entity, f32)> = None;

    for &e in world.entities() {
        let Some(t) = world.get::<Transform>(e) else { continue };
        let Some(mh) = world.get::<MeshHandle>(e) else { continue };
        let Some(mesh) = renderer.meshes.get(&mh.0) else { continue };

        let model = t.matrix();
        let (center, radius) = mesh.world_bounds(&model);

        if let Some(t_hit) = ray_sphere_hit(origin, dir, center, radius) {
            if best.map_or(true, |(_, bt)| t_hit < bt) {
                best = Some((e, t_hit));
            }
        }
    }

    best.map(|(e, _)| e)
}

/// Возвращает параметр `t` вдоль луча, если луч пересекает сферу.
/// `dir` должен быть нормализован.
fn ray_sphere_hit(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<f32> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 {
        return None;
    }
    let t = -b - disc.sqrt();
    if t < 0.0 {
        // Сфера позади луча (например, камера внутри) — берём дальнюю точку.
        let t2 = -b + disc.sqrt();
        if t2 < 0.0 {
            return None;
        }
        Some(t2)
    } else {
        Some(t)
    }
}