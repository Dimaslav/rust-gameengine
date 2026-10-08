//! Коллизии игрока со сценой.
//!
//! ## Что нового
//!
//! * **step3-spatial-hash**: broad-phase через `SpatialHash2D`.
//! * **step4-collider-cache**: `resolve_movement_ex` и `find_support`
//!   принимают `&ColliderCache` вместо обхода всего World. Это
//!   ускоряет player collision в 5–10 раз при больших сценах.
//! * Sprint B1: поддержка terrain. `resolve_movement_ex` принимает
//!   `Option<&Heightmap>`.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::Visible;
use crate::physics::{BodyType, Collider, RigidBody};
use crate::render::terrain::Heightmap;

use super::collision_cache::{capsule_hits_aabb, ColliderCache};

const SUPPORT_TOLERANCE: f32 = 0.15;
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

// ============================================================
// Helpers (используются только для surface_normal_at)
// ============================================================

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let t = (p - a).dot(ab) / ab.length_squared().max(1e-12);
    a + ab * t.clamp(0.0, 1.0)
}

/// Нормаль поверхности коллайдера в точке `probe` (world-space).
///
/// Использует данные из `ColliderEntry` (уже готовые world pos/rot/scale).
fn surface_normal_at_entry(e: &super::collision_cache::ColliderEntry, probe: Vec3) -> Vec3 {
    let inv_rot = e.world_rot.inverse();
    match e.collider {
        Collider::Sphere { radius } => {
            let r = radius * e.world_scale.max_element();
            let to = probe - e.world_pos;
            let dist = to.length();
            if dist < 1e-4 || r < 1e-6 { Vec3::Y }
            else { (to / dist).normalize_or(Vec3::Y) }
        }
        Collider::Aabb { half_extents } => {
            let h = half_extents * e.world_scale;
            let d = inv_rot * (probe - e.world_pos);
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
            (e.world_rot * local_normal).normalize_or(Vec3::Y)
        }
        Collider::Capsule { radius, height } => {
            let r = radius * e.world_scale.max_element();
            let hy = height * e.world_scale.y * 0.5;
            let up = e.world_rot * Vec3::new(0.0, hy, 0.0);
            let a = e.world_pos - up;
            let b = e.world_pos + up;
            let closest = closest_on_segment(probe, a, b);
            let to = probe - closest;
            let dist = to.length();
            if dist < 1e-4 || r < 1e-6 { Vec3::Y }
            else { (to / dist).normalize_or(Vec3::Y) }
        }
    }
}

// ============================================================
// Find support — принимает кэш
// ============================================================

pub fn find_support(
    cache: &ColliderCache,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<SupportInfo> {
    let r = cap.radius;
    let mut best: Option<SupportInfo> = None;

    for e in cache.entries() {
        let amin = e.aabb_min;
        let amax = e.aabb_max;
        let top_y = amax.y;
        let dy = feet_pos.y - top_y;
        if !(-0.02..=SUPPORT_TOLERANCE).contains(&dy) { continue; }
        if feet_pos.x + r < amin.x || feet_pos.x - r > amax.x { continue; }
        if feet_pos.z + r < amin.z || feet_pos.z - r > amax.z { continue; }

        let normal = surface_normal_at_entry(e, feet_pos);

        if best.as_ref().map_or(true, |b| top_y > b.top_y) {
            best = Some(SupportInfo { entity: e.entity, normal, top_y });
        }
    }
    best
}

pub fn find_support_entity(
    cache: &ColliderCache,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<Entity> {
    find_support(cache, feet_pos, cap).map(|s| s.entity)
}

// ============================================================
// Movement resolution — принимает кэш
// ============================================================

pub fn resolve_movement_ex(
    _world: &World,
    cache: &ColliderCache,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
    step_down_max: f32,
    terrain: Option<&Heightmap>,
) -> MovementResult {
    let mut pos = start_feet;
    let mut on_ground = false;

    let can_step_up = true;
    let step_heights = [0.15_f32, 0.30, 0.45];

    let r = cap.radius;
    let h = cap.height.max(2.0 * r + 1e-3);

    // === X — субшагами ===
    if delta.x.abs() > 1e-6 {
        let steps = (delta.x.abs() / SUBSTEP_SIZE).ceil().max(1.0) as i32;
        let step = delta.x / steps as f32;
        for _ in 0..steps {
            let try_pos = pos + Vec3::new(step, 0.0, 0.0);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, false) {
                pos = try_pos;
            } else if can_step_up {
                let mut stepped = false;
                for sh in step_heights {
                    let lifted = try_pos + Vec3::new(0.0, sh, 0.0);
                    let a2 = lifted + Vec3::Y * r;
                    let b2 = lifted + Vec3::Y * (h - r);
                    if !cache.capsule_hits(a2, b2, r, false) {
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

    // === Z — субшагами ===
    if delta.z.abs() > 1e-6 {
        let steps = (delta.z.abs() / SUBSTEP_SIZE).ceil().max(1.0) as i32;
        let step = delta.z / steps as f32;
        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, 0.0, step);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, false) {
                pos = try_pos;
            } else if can_step_up {
                let mut stepped = false;
                for sh in step_heights {
                    let lifted = try_pos + Vec3::new(0.0, sh, 0.0);
                    let a2 = lifted + Vec3::Y * r;
                    let b2 = lifted + Vec3::Y * (h - r);
                    if !cache.capsule_hits(a2, b2, r, false) {
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

    // === Y — субшагами (с dynamic-телами: можно стоять на ящике) ===
    if delta.y.abs() > 1e-6 {
        let max_step = 0.1_f32;
        let steps = (delta.y.abs() / max_step).ceil().max(1.0) as i32;
        let step = delta.y / steps as f32;

        for _ in 0..steps {
            let try_pos = pos + Vec3::new(0.0, step, 0.0);
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, true) {
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
            let a = try_pos + Vec3::Y * r;
            let b = try_pos + Vec3::Y * (h - r);
            if !cache.capsule_hits(a, b, r, true) {
                probe = try_pos;
            } else {
                pos = probe;
                on_ground = true;
                break;
            }
        }
    }

    // === Terrain приземление ===
    let local_floor = match terrain {
        Some(t) if t.contains(pos.x, pos.z) => t.sample(pos.x, pos.z).max(floor_y),
        _ => floor_y,
    };
    if pos.y <= local_floor {
        pos.y = local_floor;
        on_ground = true;
    }

    let support = find_support(cache, pos, cap);
    if support.is_some() && delta.y <= 0.0 {
        on_ground = true;
    }

    MovementResult { new_feet: pos, landed: on_ground, support }
}

/// Устаревшая обёртка: строит кэш внутри. Не использовать в hot-path.
pub fn resolve_movement(
    world: &World,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
) -> (Vec3, bool, Option<Entity>) {
    let mut cache = ColliderCache::new();
    cache.build(world);
    let r = resolve_movement_ex(world, &cache, start_feet, delta, cap, floor_y, 0.0, None);
    (r.new_feet, r.landed, r.support.map(|s| s.entity))
}

// ============================================================
// Legacy: keep visible-check helper — некоторые системы его используют
// ============================================================

#[allow(dead_code)]
fn entity_is_visible(world: &World, e: Entity) -> bool {
    world.get::<Visible>(e).map(|v| v.0).unwrap_or(true)
}