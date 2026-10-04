//! Коллизии игрока со сценой.
//!
//! **Dynamic-тела не блокируют игрока по XZ** — он проходит сквозь их
//! footprint, а `character::push_dynamic_bodies` после этого толкает
//! пересекающиеся тела. По Y динамические тела по-прежнему блокируют —
//! игрок может стоять на ящике.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::{MeshHandle, Transform, Visible};
use crate::physics::{BodyType, RigidBody};
use crate::render::Renderer;

const MAX_COLLIDABLE_EXTENT: f32 = 30.0;
const CAPSULE_SAMPLES: usize = 8;
/// Допуск по Y для определения «стоит на поверхности».
/// Увеличен с 0.10 до 0.15 — быстрый лифт (5+ м/с) при 60 FPS проходит
/// 8+ см за кадр и мог проскочить поддержку.
const SUPPORT_TOLERANCE: f32 = 0.15;

#[derive(Clone, Copy)]
pub struct PlayerCapsule {
    pub radius: f32,
    pub height: f32,
}

impl Default for PlayerCapsule {
    fn default() -> Self {
        Self { radius: 0.35, height: 1.8 }
    }
}

#[inline]
fn point_aabb_dist_sq(p: Vec3, min: Vec3, max: Vec3) -> f32 {
    let clamped = p.clamp(min, max);
    (p - clamped).length_squared()
}

#[inline]
fn capsule_hits_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
    let r2 = r * r;
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

/// Игрок vs мир. По XZ **игнорирует** dynamic-тела —
/// они не блокируют движение, их толкает `character::push_dynamic_bodies`.
fn capsule_hits(
    world: &World,
    renderer: &Renderer,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> bool {
    capsule_hits_impl(world, renderer, feet_pos, cap, false)
}

/// То же, но с учётом dynamic-тел. Используется в Y-проверке
/// `resolve_movement`, чтобы игрок мог стоять на ящике.
fn capsule_hits_with_dynamic(
    world: &World,
    renderer: &Renderer,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> bool {
    capsule_hits_impl(world, renderer, feet_pos, cap, true)
}

fn capsule_hits_impl(
    world: &World,
    renderer: &Renderer,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
    include_dynamic: bool,
) -> bool {
    let r = cap.radius;
    let h = cap.height.max(2.0 * r + 1e-3);
    let a = feet_pos + Vec3::Y * r;
    let b = feet_pos + Vec3::Y * (h - r);

    for &e in world.entities() {
        // Skip invisible.
        if let Some(v) = world.get::<Visible>(e) {
            if !v.0 {
                continue;
            }
        }

        // Skip dynamic bodies by XZ (or everywhere if include_dynamic=false).
        if !include_dynamic {
            if let Some(rb) = world.get::<RigidBody>(e) {
                if rb.body_type == BodyType::Dynamic {
                    continue;
                }
            }
        }

        let (Some(_t), Some(mh)) = (world.get::<Transform>(e), world.get::<MeshHandle>(e))
        else {
            continue;
        };
        let Some(mesh) = renderer.meshes.get(&mh.0) else {
            continue;
        };

        let model = crate::game::world_matrix(world, e);
        let (amin, amax) = mesh.world_aabb(&model);
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

/// Ищет entity, на чьей верхней плоскости стоит игрок.
/// Работает для всех тел, включая dynamic — можно стоять на ящике.
pub fn find_support_entity(
    world: &World,
    renderer: &Renderer,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<Entity> {
    let r = cap.radius;
    let mut best: Option<(Entity, f32)> = None;

    for &e in world.entities() {
        if let Some(v) = world.get::<Visible>(e) {
            if !v.0 {
                continue;
            }
        }
        let Some(mh) = world.get::<MeshHandle>(e) else { continue };
        let Some(mesh) = renderer.meshes.get(&mh.0) else { continue };
        if world.get::<Transform>(e).is_none() {
            continue;
        }

        let model = crate::game::world_matrix(world, e);
        let (amin, amax) = mesh.world_aabb(&model);
        let extent = (amax - amin).length();
        if extent > MAX_COLLIDABLE_EXTENT {
            continue;
        }

        let top_y = amax.y;
        let dy = feet_pos.y - top_y;
        if !(-0.02..=SUPPORT_TOLERANCE).contains(&dy) {
            continue;
        }

        if feet_pos.x + r < amin.x || feet_pos.x - r > amax.x {
            continue;
        }
        if feet_pos.z + r < amin.z || feet_pos.z - r > amax.z {
            continue;
        }

        if best.map_or(true, |(_, y)| top_y > y) {
            best = Some((e, top_y));
        }
    }
    best.map(|(e, _)| e)
}

/// Разрешает движение `delta` из `start_feet` с учётом коллизий.
///
/// Возвращает `(new_feet, on_ground, support_entity)`.
///
/// XZ — с игнорированием dynamic-тел (player passes through, потом толкает).
/// Y  — со всеми телами (можно стоять на ящике).
pub fn resolve_movement(
    world: &World,
    renderer: &Renderer,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
) -> (Vec3, bool, Option<Entity>) {
    let stuck = capsule_hits_with_dynamic(world, renderer, start_feet, cap);

    if stuck {
        let mut pos = start_feet + delta;
        if pos.y < floor_y {
            pos.y = floor_y;
        }
        let on_ground = (start_feet.y + delta.y) <= floor_y + 1e-4;
        let support = if on_ground {
            find_support_entity(world, renderer, pos, cap)
        } else {
            None
        };
        return (pos, on_ground, support);
    }

    let mut pos = start_feet;
    let mut on_ground = false;

    let can_step_up = start_feet.y <= floor_y + 1e-3;
    let step_heights = [0.15_f32, 0.30, 0.45];

    // === X — без dynamic ===
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

    // === Z — без dynamic ===
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

    // === Y — субшагами, со всеми телами ===
    if delta.y.abs() > 1e-6 {
        let max_step = 0.1_f32;
        let steps = (delta.y.abs() / max_step).ceil().max(1.0) as i32;
        let step = delta.y / steps as f32;

        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, step, 0.0);
            if !capsule_hits_with_dynamic(world, renderer, try_pos, cap) {
                pos = try_pos;
            } else {
                if step < 0.0 {
                    on_ground = true;
                }
                break;
            }
        }
    }

    if pos.y <= floor_y {
        pos.y = floor_y;
        on_ground = true;
    }

    let support = find_support_entity(world, renderer, pos, cap);
    if support.is_some() {
        on_ground = true;
    }

    (pos, on_ground, support)
}