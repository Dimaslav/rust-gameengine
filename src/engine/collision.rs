//! Коллизии игрока со сценой.
//!
//! Игрок — вертикальная **капсула** (радиус + высота). Препятствия —
//! точные **AABB** мешей в мире (`Mesh::world_aabb`), а не bounding-сферы.
//!
//! Движение разрешается раздельно по осям X/Z, плюс субшагами по Y.
//! При заблокированном горизонтальном движении пробуется **step-up** —
//! подъём на низкую ступеньку (0.15 / 0.3 / 0.45 м), если игрок на земле.
//!
//! Очень крупные объекты (plane 200×200) пропускаются — их AABB
//! покрыл бы всю сцену. Вместо них землю задаёт явный `floor_y`.

use glam::Vec3;

use crate::ecs::World;
use crate::game::components::{MeshHandle, Transform, Visible};
use crate::render::Renderer;

/// Порог размера AABB, выше которого меш считается «фоном» и не коллизится.
const MAX_COLLIDABLE_EXTENT: f32 = 30.0;

/// Количество сэмплов вдоль оси капсулы для проверки столкновения.
/// 8 — компромисс между точностью и производительностью.
const CAPSULE_SAMPLES: usize = 8;

#[derive(Clone, Copy)]
pub struct PlayerCapsule {
    /// Радиус цилиндра/полусфер.
    pub radius: f32,
    /// Полная высота от ног до макушки.
    pub height: f32,
}

impl Default for PlayerCapsule {
    fn default() -> Self {
        Self {
            radius: 0.35,
            height: 1.8,
        }
    }
}

/// Квадрат расстояния от точки до AABB.
/// Внутри AABB — 0.
#[inline]
fn point_aabb_dist_sq(p: Vec3, min: Vec3, max: Vec3) -> f32 {
    let clamped = p.clamp(min, max);
    (p - clamped).length_squared()
}

/// Проверка: пересекается ли капсула `[a, b] + r` с AABB.
/// Разбиваем сегмент на CAPSULE_SAMPLES+1 точек и проверяем каждую.
#[inline]
fn capsule_hits_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
    let r2 = r * r;

    // Быстрый отсев: если центры далеко — не пересекаются.
    let center = (amin + amax) * 0.5;
    let half = (amax - amin) * 0.5;
    let bs_radius = half.length();
    let capsule_center = (a + b) * 0.5;
    if (center - capsule_center).length() > bs_radius + r + (b - a).length() * 0.5 {
        return false;
    }

    for i in 0..=CAPSULE_SAMPLES {
        let t = i as f32 / CAPSULE_SAMPLES as f32;
        let p = a.lerp(b, t);
        if point_aabb_dist_sq(p, amin, amax) < r2 {
            return true;
        }
    }
    false
}

/// Проверка столкновения капсулы игрока со всеми мешами мира.
fn capsule_hits(
    world: &World,
    renderer: &Renderer,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> bool {
    let r = cap.radius;
    let h = cap.height.max(2.0 * r + 1e-3);

    // Ось капсулы: от центра нижней полусферы до центра верхней.
    let a = feet_pos + Vec3::Y * r;
    let b = feet_pos + Vec3::Y * (h - r);

    for &e in world.entities() {
        // Невидимые — не коллизятся.
        if let Some(v) = world.get::<Visible>(e) {
            if !v.0 {
                continue;
            }
        }

        let (Some(_t), Some(mh)) = (
            world.get::<Transform>(e),
            world.get::<MeshHandle>(e),
        ) else {
            continue;
        };
        let Some(mesh) = renderer.meshes.get(&mh.0) else {
            continue;
        };

        let model = crate::game::world_matrix(world, e);
        let (amin, amax) = mesh.world_aabb(&model);

        // Пропускаем «фоновые» меши (ground, skybox и т.п.).
        let extent = (amax - amin).length();
        if extent > MAX_COLLIDABLE_EXTENT {
            continue;
        }

        if capsule_hits_aabb(a, b, r, amin, amax) {
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
    cap: &PlayerCapsule,
    floor_y: f32,
) -> (Vec3, bool) {
    let stuck = capsule_hits(world, renderer, start_feet, cap);

    // Если уже внутри — двигаемся без проверок, чтобы выбраться.
    if stuck {
        let mut pos = start_feet + delta;
        if pos.y < floor_y {
            pos.y = floor_y;
        }
        let on_ground = (start_feet.y + delta.y) <= floor_y + 1e-4;
        return (pos, on_ground);
    }

    let mut pos = start_feet;
    let mut on_ground = false;

    let can_step_up = start_feet.y <= floor_y + 1e-3;
    // Порядок ступенек: 0.15 → 0.45 м.
    let step_heights = [0.15_f32, 0.30, 0.45];

    // === X ===
    if delta.x.abs() > 1e-6 {
        let try_pos = pos + Vec3::new(delta.x, 0.0, 0.0);
        if !capsule_hits(world, renderer, try_pos, cap) {
            pos = try_pos;
        } else if can_step_up {
            for h in step_heights {
                let lifted = try_pos + Vec3::new(0.0, h, 0.0);
                if !capsule_hits(world, renderer, lifted, cap) {
                    pos = lifted;
                    break;
                }
            }
        }
    }

    // === Z ===
    if delta.z.abs() > 1e-6 {
        let try_pos = pos + Vec3::new(0.0, 0.0, delta.z);
        if !capsule_hits(world, renderer, try_pos, cap) {
            pos = try_pos;
        } else if can_step_up {
            for h in step_heights {
                let lifted = try_pos + Vec3::new(0.0, h, 0.0);
                if !capsule_hits(world, renderer, lifted, cap) {
                    pos = lifted;
                    break;
                }
            }
        }
    }

    // === Y — субшагами ===
    if delta.y.abs() > 1e-6 {
        let max_step = 0.1_f32;
        let steps = (delta.y.abs() / max_step).ceil().max(1.0) as i32;
        let step = delta.y / steps as f32;

        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, step, 0.0);
            if !capsule_hits(world, renderer, try_pos, cap) {
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