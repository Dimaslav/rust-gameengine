//! Простые AABB-коллизии между игроком и мешами сцены.
//!
//! Каждый меш представляется кубом вокруг его bounding-сферы.
//! Очень большие объекты (plane 200×200) пропускаются — их куб
//! покрыл бы всю сцену, и игрок всегда был бы «внутри».
//! Вместо них землю задаёт явный пол (`floor_y`).
//!
//! Если игрок по какой-то причине оказался внутри коллайдера
//! (неудачный спавн, телепорт, undo) — движение разрешается
//! без проверки. Это даёт возможность выбраться.

use glam::Vec3;

use crate::ecs::World;
use crate::game::components::{MeshHandle, Transform};
use crate::render::Renderer;

/// Порог радиуса, выше которого меш считается «фоном» и не коллизится.
const MAX_COLLIDABLE_RADIUS: f32 = 20.0;

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

        // Пропускаем «фоновые» меши (ground, skybox и т.п.).
        if radius > MAX_COLLIDABLE_RADIUS {
            continue;
        }

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
///
/// `floor_y` — минимальный Y для ног игрока.
///
/// Если в стартовой позиции уже коллизия (застряли) — движение
/// разрешается без проверок, чтобы игрок мог выбраться. Это
/// стандартный приём; иначе при неудачном спавне игрок навсегда
/// остаётся в стене.
pub fn resolve_movement(
    world: &World,
    renderer: &Renderer,
    start_feet: Vec3,
    delta: Vec3,
    pbox: &PlayerBox,
    floor_y: f32,
) -> (Vec3, bool) {
    let stuck = collides(world, renderer, start_feet, pbox);

    // Если уже внутри — просто двигаемся, не проверяя коллизии.
    // Так игрок постепенно выберется наружу.
    if stuck {
        let mut pos = start_feet + delta;
        if pos.y < floor_y {
            pos.y = floor_y;
        }
        let on_ground = (start_feet.y + delta.y) <= floor_y + 1e-4;
        return (pos, on_ground);
    }

    // Обычный случай: разделяем движение по осям.
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

    // Y — субшагами.
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

    // Пол — отдельная проверка после коллизий.
    if pos.y <= floor_y {
        pos.y = floor_y;
        on_ground = true;
    }

    (pos, on_ground)
}