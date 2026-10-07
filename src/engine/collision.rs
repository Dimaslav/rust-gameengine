//! Коллизии игрока со сценой.
//!
//! **ИЗМЕНЕНО (rotation fix):** теперь корректно учитывается `Transform.rotation`
//! при вычислении мирового AABB коллайдера. Раньше стены, повёрнутые на 90°
//! (Wall_W, Wall_E), имели коллизию, перпендикулярную мешу.
//!
//! **ИЗМЕНЕНО (step-up fix):** step-up больше не ограничен полом y=0.
//! Теперь игрок может подниматься на ступени, платформы, ящики.
//!
//! **ИЗМЕНЕНО (sub-stepping):** X и Z движения субделятся на шаги ≤0.2м,
//! чтобы игрок не «протуннелировался» сквозь тонкие стены на высокой
//! скорости.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::Visible;
use crate::physics::{BodyType, Collider, RigidBody};

const CAPSULE_SAMPLES: usize = 8;
const SUPPORT_TOLERANCE: f32 = 0.15;
/// Максимальный шаг субдиления X/Z. Должен быть меньше минимальной
/// толщины стен в сцене.
const SUBSTEP_SIZE: f32 = 0.15;

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

#[derive(Clone, Copy, Debug)]
pub struct SupportInfo {
    pub entity: Entity,
    pub normal: Vec3,
    pub top_y: f32,
}

#[derive(Clone, Copy, Debug)]
pub struct MovementResult {
    pub new_feet: Vec3,
    pub landed: bool,
    pub support: Option<SupportInfo>,
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

/// Возвращает (world_pos, world_rotation, world_scale).
fn world_pos_rot_scale(world: &World, e: Entity) -> Option<(Vec3, glam::Quat, Vec3)> {
    let model = crate::game::world_matrix(world, e);
    let (s, r, t) = model.to_scale_rotation_translation();
    Some((t, r, s.abs()))
}

fn capsule_hits_impl(
    world: &World,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
    include_dynamic: bool,
) -> bool {
    let r = cap.radius;
    let h = cap.height.max(2.0 * r + 1e-3);
    let a = feet_pos + Vec3::Y * r;
    let b = feet_pos + Vec3::Y * (h - r);

    for &e in world.entities() {
        if let Some(v) = world.get::<Visible>(e) {
            if !v.0 { continue; }
        }
        if !include_dynamic {
            if let Some(rb) = world.get::<RigidBody>(e) {
                if rb.body_type == BodyType::Dynamic { continue; }
            }
        }
        let Some(col) = world.get::<Collider>(e) else { continue };
        let Some((wp, wr, ws)) = world_pos_rot_scale(world, e) else { continue };

        let (amin, amax) = col.world_aabb(wp, wr, ws);
        if capsule_hits_aabb(a, b, r, amin, amax) {
            return true;
        }
    }
    false
}

fn capsule_hits(world: &World, feet_pos: Vec3, cap: &PlayerCapsule) -> bool {
    capsule_hits_impl(world, feet_pos, cap, false)
}

fn capsule_hits_with_dynamic(world: &World, feet_pos: Vec3, cap: &PlayerCapsule) -> bool {
    capsule_hits_impl(world, feet_pos, cap, true)
}

/// Нормаль поверхности коллайдера в точке `probe`.
///
/// **ИЗМЕНЕНО:** работает в локальном пространстве коллайдера —
/// probe трансформируется через inverse rotation, и нормаль
/// возвращается в world space.
fn surface_normal_at(
    col: &Collider,
    wp: Vec3,
    wr: glam::Quat,
    ws: Vec3,
    probe: Vec3,
) -> Vec3 {
    let inv_rot = wr.inverse();
    match col {
        Collider::Sphere { radius } => {
            let r = radius * ws.max_element();
            let to = probe - wp;
            let dist = to.length();
            if dist < 1e-4 || r < 1e-6 { Vec3::Y }
            else { (to / dist).normalize_or(Vec3::Y) }
        }
        Collider::Aabb { half_extents } => {
            let h = *half_extents * ws;
            let d = inv_rot * (probe - wp);
            let dx = (h.x - d.x.abs()).max(0.0);
            let dy = (h.y - d.y.abs()).max(0.0);
            let dz = (h.z - d.z.abs()).max(0.0);
            let local_normal = if dy <= dx && dy <= dz {
                Vec3::new(0.0, if d.y >= 0.0 { 1.0 } else { -1.0 }, 0.0)
            } else if dx <= dz {
                Vec3::new(d.x.signum(), 0.0, 0.0)
            } else {
                Vec3::new(0.0, 0.0, d.z.signum())
            };
            (wr * local_normal).normalize_or(Vec3::Y)
        }
        Collider::Capsule { radius, height } => {
            let r = radius * ws.max_element();
            let hy = height * ws.y * 0.5;
            let up = wr * Vec3::new(0.0, hy, 0.0);
            let a = wp - up;
            let b = wp + up;
            let closest = closest_on_segment(probe, a, b);
            let to = probe - closest;
            let dist = to.length();
            if dist < 1e-4 || r < 1e-6 { Vec3::Y }
            else { (to / dist).normalize_or(Vec3::Y) }
        }
    }
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let t = (p - a).dot(ab) / ab.length_squared().max(1e-12);
    a + ab * t.clamp(0.0, 1.0)
}

pub fn find_support(
    world: &World,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<SupportInfo> {
    let r = cap.radius;
    let mut best: Option<SupportInfo> = None;

    for &e in world.entities() {
        if let Some(v) = world.get::<Visible>(e) {
            if !v.0 { continue; }
        }
        let Some(col) = world.get::<Collider>(e) else { continue };
        let Some((wp, wr, ws)) = world_pos_rot_scale(world, e) else { continue };

        let (amin, amax) = col.world_aabb(wp, wr, ws);
        let top_y = amax.y;
        let dy = feet_pos.y - top_y;
        if !(-0.02..=SUPPORT_TOLERANCE).contains(&dy) { continue; }
        if feet_pos.x + r < amin.x || feet_pos.x - r > amax.x { continue; }
        if feet_pos.z + r < amin.z || feet_pos.z - r > amax.z { continue; }

        let normal = surface_normal_at(col, wp, wr, ws, feet_pos);

        if best.as_ref().map_or(true, |b| top_y > b.top_y) {
            best = Some(SupportInfo { entity: e, normal, top_y });
        }
    }
    best
}

pub fn find_support_entity(
    world: &World,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<Entity> {
    find_support(world, feet_pos, cap).map(|s| s.entity)
}

pub fn resolve_movement_ex(
    world: &World,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
    step_down_max: f32,
) -> MovementResult {
    let mut pos = start_feet;
    let mut on_ground = false;

    // Step-up доступен всегда. Ограничение `y <= floor_y` убрано —
    // теперь игрок может подниматься на ступени, платформы, ящики.
    let can_step_up = true;
    let step_heights = [0.15_f32, 0.30, 0.45];

    // === X — субшагами, без dynamic ===
    if delta.x.abs() > 1e-6 {
        let steps = (delta.x.abs() / SUBSTEP_SIZE).ceil().max(1.0) as i32;
        let step = delta.x / steps as f32;
        for _ in 0..steps {
            let try_pos = pos + Vec3::new(step, 0.0, 0.0);
            if !capsule_hits(world, try_pos, cap) {
                pos = try_pos;
            } else if can_step_up {
                let mut stepped = false;
                for h in step_heights {
                    let lifted = try_pos + Vec3::new(0.0, h, 0.0);
                    if !capsule_hits(world, lifted, cap) {
                        pos = lifted;
                        stepped = true;
                        break;
                    }
                }
                if !stepped { break; }
            } else {
                break;
            }
        }
    }

    // === Z — субшагами, без dynamic ===
    if delta.z.abs() > 1e-6 {
        let steps = (delta.z.abs() / SUBSTEP_SIZE).ceil().max(1.0) as i32;
        let step = delta.z / steps as f32;
        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, 0.0, step);
            if !capsule_hits(world, try_pos, cap) {
                pos = try_pos;
            } else if can_step_up {
                let mut stepped = false;
                for h in step_heights {
                    let lifted = try_pos + Vec3::new(0.0, h, 0.0);
                    if !capsule_hits(world, lifted, cap) {
                        pos = lifted;
                        stepped = true;
                        break;
                    }
                }
                if !stepped { break; }
            } else {
                break;
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
            if !capsule_hits_with_dynamic(world, try_pos, cap) {
                pos = try_pos;
            } else {
                if step < 0.0 { on_ground = true; }
                break;
            }
        }
    }

    // === Step-down ===
    if !on_ground && delta.y <= 0.0 && step_down_max > 1e-4 {
        let probe_step = 0.05_f32;
        let n = (step_down_max / probe_step).ceil() as i32;
        let mut probe = pos;
        for _ in 0..n {
            let try_pos = probe + Vec3::new(0.0, -probe_step, 0.0);
            if !capsule_hits_with_dynamic(world, try_pos, cap) {
                probe = try_pos;
            } else {
                pos = probe;
                on_ground = true;
                break;
            }
        }
    }

    if pos.y <= floor_y {
        pos.y = floor_y;
        on_ground = true;
    }

    let support = find_support(world, pos, cap);
    if support.is_some() && delta.y <= 0.0 {
        on_ground = true;
    }

    MovementResult { new_feet: pos, landed: on_ground, support }
}

pub fn resolve_movement(
    world: &World,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
) -> (Vec3, bool, Option<Entity>) {
    let r = resolve_movement_ex(world, start_feet, delta, cap, floor_y, 0.0);
    (r.new_feet, r.landed, r.support.map(|s| s.entity))
}