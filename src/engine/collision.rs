//! Коллизии игрока со сценой.
//!
//! Использует компонент `Collider` (Sphere/Aabb/Capsule) вместо AABB
//! меша. Это даёт корректные коллизии для стен, дверей, NPC и любых
//! объектов, у которых меш не совпадает с логикой.
//!
//! **Dynamic-тела не блокируют игрока по XZ** — он проходит сквозь их
//! footprint, а `character::push_dynamic_bodies` после этого толкает
//! пересекающиеся тела. По Y dynamic-тела блокируют — можно стоять
//! на ящике.
//!
//! # Фаза 5: платформерный контроллер
//!
//! Добавлено:
//!   * `find_support` возвращает **нормаль поверхности** под ногами —
//!     для slope slide и будущего slope-limit наклона персонажа.
//!   * `resolve_movement_ex` возвращает `MovementResult` с нормалью
//!     и информацией о support-entity.
//!   * **Step-down**: при спуске с уступа < `step_down_max` позиция
//!     снэпится к поверхности, чтобы персонаж не «парил» и не
//!     «отскакивал» при ходьбе по неровностям.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::Visible;
use crate::physics::{BodyType, Collider, RigidBody};

const CAPSULE_SAMPLES: usize = 8;
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

/// ИЗМЕНЕНО (Фаза 5): информация о поверхности под ногами.
///
/// `normal` указывает НАРУЖУ от поверхности (для плоского пола — `+Y`).
/// Для наклонного склона нормаль наклонена, что используется для:
///   * slope slide — проекция гравитации на плоскость склона;
///   * slope walk_limit — если `normal.y < cos(limit)`, ходить нельзя.
#[derive(Clone, Copy, Debug)]
pub struct SupportInfo {
    pub entity: Entity,
    pub normal: Vec3,
    pub top_y: f32,
}

/// ИЗМЕНЕНО (Фаза 5): результат `resolve_movement_ex`.
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

fn collider_world_aabb(col: &Collider, world_pos: Vec3, world_scale: Vec3) -> (Vec3, Vec3) {
    match col {
        Collider::Sphere { radius } => {
            let r = radius * world_scale.max_element();
            (world_pos - Vec3::splat(r), world_pos + Vec3::splat(r))
        }
        Collider::Aabb { half_extents } => {
            let h = *half_extents * world_scale;
            (world_pos - h, world_pos + h)
        }
        Collider::Capsule { radius, height } => {
            let r = radius * world_scale.max_element();
            let hy = height * world_scale.y * 0.5;
            let h = Vec3::new(r, hy, r);
            (world_pos - h, world_pos + h)
        }
    }
}

fn world_pos_scale(world: &World, e: Entity) -> Option<(Vec3, Vec3)> {
    let model = crate::game::world_matrix(world, e);
    let (s, _, t) = model.to_scale_rotation_translation();
    Some((t, s.abs()))
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
        let Some((wp, ws)) = world_pos_scale(world, e) else { continue };

        let (amin, amax) = collider_world_aabb(col, wp, ws);
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

/// Нормаль поверхности коллайдера в точке `probe` (обычно — ноги).
/// Простой, но работающий вариант для Sphere/Aabb/Capsule без rotation.
fn surface_normal_at(col: &Collider, wp: Vec3, ws: Vec3, probe: Vec3) -> Vec3 {
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
            let d = probe - wp;
            let dx = (h.x - d.x.abs()).max(0.0);
            let dy = (h.y - d.y.abs()).max(0.0);
            let dz = (h.z - d.z.abs()).max(0.0);
            if dy <= dx && dy <= dz {
                // Верхняя (или нижняя) грань — почти всегда верхняя.
                Vec3::new(0.0, if d.y >= 0.0 { 1.0 } else { -1.0 }, 0.0)
            } else if dx <= dz {
                Vec3::new(d.x.signum(), 0.0, 0.0)
            } else {
                Vec3::new(0.0, 0.0, d.z.signum())
            }
        }
        Collider::Capsule { radius, height } => {
            let r = radius * ws.max_element();
            let hy = height * ws.y * 0.5;
            let a = wp - Vec3::Y * hy;
            let b = wp + Vec3::Y * hy;
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

/// ИЗМЕНЕНО (Фаза 5): ищет support с нормалью.
///
/// Возвращает ближайшую под ногами поверхность, если её верхняя
/// точка в пределах `SUPPORT_TOLERANCE`. Нормаль нормализована.
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
        let Some((wp, ws)) = world_pos_scale(world, e) else { continue };

        let (amin, amax) = collider_world_aabb(col, wp, ws);
        let top_y = amax.y;
        let dy = feet_pos.y - top_y;
        if !(-0.02..=SUPPORT_TOLERANCE).contains(&dy) { continue; }
        if feet_pos.x + r < amin.x || feet_pos.x - r > amax.x { continue; }
        if feet_pos.z + r < amin.z || feet_pos.z - r > amax.z { continue; }

        let normal = surface_normal_at(col, wp, ws, feet_pos);

        if best.as_ref().map_or(true, |b| top_y > b.top_y) {
            best = Some(SupportInfo { entity: e, normal, top_y });
        }
    }
    best
}

/// Оставлено для обратной совместимости (используется App для
/// moving-platform support).
pub fn find_support_entity(
    world: &World,
    feet_pos: Vec3,
    cap: &PlayerCapsule,
) -> Option<Entity> {
    find_support(world, feet_pos, cap).map(|s| s.entity)
}

/// ИЗМЕНЕНО (Фаза 5): возвращает `MovementResult` вместо голого
/// кортежа. Добавлен step-down.
///
/// `step_down_max = 0.0` — отключает step-down (поведение как раньше).
pub fn resolve_movement_ex(
    world: &World,
    start_feet: Vec3,
    delta: Vec3,
    cap: &PlayerCapsule,
    floor_y: f32,
    step_down_max: f32,
) -> MovementResult {
    let stuck = capsule_hits_with_dynamic(world, start_feet, cap);

    if stuck {
        let mut pos = start_feet + delta;
        if pos.y < floor_y { pos.y = floor_y; }
        let on_ground = (start_feet.y + delta.y) <= floor_y + 1e-4;
        let support = if on_ground { find_support(world, pos, cap) } else { None };
        return MovementResult { new_feet: pos, landed: on_ground, support };
    }

    let mut pos = start_feet;
    let mut on_ground = false;

    let can_step_up = start_feet.y <= floor_y + 1e-3;
    let step_heights = [0.15_f32, 0.30, 0.45];

    // === X — без dynamic ===
    if delta.x.abs() > 1e-6 {
        let try_pos = pos + Vec3::new(delta.x, 0.0, 0.0);
        if !capsule_hits(world, try_pos, cap) {
            pos = try_pos;
        } else if can_step_up {
            for h in step_heights {
                let lifted = try_pos + Vec3::new(0.0, h, 0.0);
                if !capsule_hits(world, lifted, cap) {
                    pos = lifted;
                    break;
                }
            }
        }
    }

    // === Z — без dynamic ===
    if delta.z.abs() > 1e-6 {
        let try_pos = pos + Vec3::new(0.0, 0.0, delta.z);
        if !capsule_hits(world, try_pos, cap) {
            pos = try_pos;
        } else if can_step_up {
            for h in step_heights {
                let lifted = try_pos + Vec3::new(0.0, h, 0.0);
                if !capsule_hits(world, lifted, cap) {
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
            if !capsule_hits_with_dynamic(world, try_pos, cap) {
                pos = try_pos;
            } else {
                if step < 0.0 { on_ground = true; }
                break;
            }
        }
    }

    // === ИЗМЕНЕНО (Фаза 5): step-down ===
    //
    // Применяется только когда мы движемся ВНИЗ (delta.y <= 0) и не
    // приземлились на этом шаге. Если под ногами в пределах
    // `step_down_max` есть поверхность — снэпим к ней. Это
    // предотвращает «парение» при спуске с уступов и сглаживает
    // ходьбу по неровностям.
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
    if support.is_some() {
        on_ground = true;
    }

    MovementResult { new_feet: pos, landed: on_ground, support }
}

/// Совместимость с прежним API. Не используется внутри — оставлено
/// на случай внешних вызовов.
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