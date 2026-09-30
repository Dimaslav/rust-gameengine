//! Простые AABB-коллизии между игроком и мешами сцены.
//!
//! Каждый меш представляется кубом вокруг его bounding-сферы
//! (`Mesh::world_bounds` возвращает центр и радиус). Игрок — AABB
//! `radius × height` от точки ног.
//!
//! Движение разрешается по осям: сначала X, потом Z, потом Y.
//! Если по оси коллизия — эта компонента отменяется, остальные едут.
//! По Y используется субшаг ≤ 0.1, чтобы не проскочить сквозь пол.

use glam::Vec3;

use crate::ecs::World;
use crate::game::components::{MeshHandle, Transform};
use crate::render::Renderer;

/// Параметры AABB игрока.
#[derive(Clone, Copy)]
pub struct PlayerBox {
    pub radius: f32,
    pub height: f32,
}

impl Default for PlayerBox {
    fn default() -> Self {
        Self {
            radius: 0.35,
            height: 1.8,
        }
    }
}

/// Проверка пересечения AABB игрока (в позиции `feet_pos` — низ бокса)
/// с AABB любого меша в мире.
fn collides(
    world: &World,
    renderer: &Renderer,
    feet_pos: Vec3,
    pbox: &PlayerBox,
) -> bool {
    let pmin = feet_pos + Vec3::new(-pbox.radius, 0.0, -pbox.radius);
    let pmax = feet_pos + Vec3::new(pbox.radius, pbox.height, pbox.radius);

    for &e in world.entities() {
        let (Some(t), Some(mh)) = (
            world.get::<Transform>(e),
            world.get::<MeshHandle>(e),
        ) else {
            continue;
        };
        let Some(mesh) = renderer.meshes.get(&mh.0) else {
            continue;
        };
        let model = t.matrix();
        let (center, radius) = mesh.world_bounds(&model);

        // Грубо: AABB меша = куб вокруг bounding-сферы.
        // Для ground plane радиус огромный — плоскость тоже станет кубом,
        // но это работает: игрок стоит на ней.
        let mm = Vec3::splat(radius);
        let mmin = center - mm;
        let mmax = center + mm;

        if pmax.x > mmin.x
            && pmin.x < mmax.x
            && pmax.y > mmin.y
            && pmin.y < mmax.y
            && pmax.z > mmin.z
            && pmin.z < mmax.z
        {
            return true;
        }
    }

    false
}

/// Разрешает движение `delta` из `start_feet` с учётом коллизий.
/// Возвращает `(new_feet, on_ground)`.
pub fn resolve_movement(
    world: &World,
    renderer: &Renderer,
    start_feet: Vec3,
    delta: Vec3,
    pbox: &PlayerBox,
) -> (Vec3, bool) {
    let mut pos = start_feet;

    // X
    if delta.x.abs() > 1e-6 {
        let try_pos = pos + Vec3::new(delta.x, 0.0, 0.0);
        if !collides(world, renderer, try_pos, pbox) {
            pos = try_pos;
        }
    }

    // Z
    if delta.z.abs() > 1e-6 {
        let try_pos = pos + Vec3::new(0.0, 0.0, delta.z);
        if !collides(world, renderer, try_pos, pbox) {
            pos = try_pos;
        }
    }

    // Y — субшагами, чтобы не провалиться сквозь пол на высокой скорости.
    let mut on_ground = false;
    if delta.y.abs() > 1e-6 {
        let max_step = 0.1_f32;
        let steps = (delta.y.abs() / max_step).ceil().max(1.0) as i32;
        let step = delta.y / steps as f32;

        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, step, 0.0);
            if !collides(world, renderer, try_pos, pbox) {
                pos = try_pos;
            } else {
                if step < 0.0 {
                    on_ground = true;
                }
                break;
            }
        }
    }

    (pos, on_ground)
}