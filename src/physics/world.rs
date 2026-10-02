//! PhysicsWorld: пошаговая симуляция.

use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::{Parent, Transform};

use super::components::{BodyType, Collider, PhysicsMaterial, RigidBody};

const SLEEP_LINEAR_THRESHOLD: f32 = 0.05;
const SLEEP_TIME_REQUIRED: f32 = 0.5;
const CONTACT_SLOP: f32 = 0.001;
const BAUMGARTE: f32 = 0.2;
const MAX_LINEAR_VELOCITY: f32 = 500.0;
const VELOCITY_ITERATIONS: u32 = 4;
const MAX_DT: f32 = 0.05;

pub struct PhysicsWorld {
    pub gravity: Vec3,
    pub enabled: bool,
    /// Диагностика: сколько пар нашёл broad-phase на прошлом шаге.
    pub last_broad_pairs: usize,
    /// Диагностика: сколько контактов нашёл narrow-phase.
    pub last_contacts: usize,
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            enabled: true,
            last_broad_pairs: 0,
            last_contacts: 0,
        }
    }
}

#[derive(Clone, Copy)]
struct BodyState {
    entity: Entity,
    body_type: BodyType,
    mass: f32,
    inv_mass: f32,
    position: Vec3,
    velocity: Vec3,
    gravity_scale: f32,
    linear_damping: f32,
    collider: Collider,
    material: PhysicsMaterial,
    scale: Vec3,
    sleeping: bool,
    sleep_timer: f32,
}

struct Contact {
    a: usize,
    b: usize,
    normal: Vec3,
    penetration: f32,
}

impl PhysicsWorld {
    pub fn step(&mut self, world: &mut World, dt: f32) {
        if !self.enabled || dt <= 0.0 { return; }
        let dt = dt.min(MAX_DT);

        let mut states = collect_states(world);
        if states.is_empty() {
            self.last_broad_pairs = 0;
            self.last_contacts = 0;
            return;
        }

        // 1. Интеграция.
        for s in states.iter_mut() {
            if s.body_type != BodyType::Dynamic || s.sleeping { continue; }

            s.velocity += self.gravity * s.gravity_scale * dt;

            let d = (1.0 - s.linear_damping * dt).max(0.0);
            s.velocity *= d;

            let vlen = s.velocity.length();
            if vlen > MAX_LINEAR_VELOCITY {
                s.velocity *= MAX_LINEAR_VELOCITY / vlen;
            }

            s.position += s.velocity * dt;
        }

        // 2. Broad-phase.
        let pairs = broad_phase(&states);
        self.last_broad_pairs = pairs.len();

        // 3. Narrow-phase.
        let mut contacts: Vec<Contact> = Vec::new();
        for (i, j) in &pairs {
            if let Some(c) = collide(&states[*i], &states[*j], *i, *j) {
                contacts.push(c);
            }
        }
        self.last_contacts = contacts.len();

        // 4. Velocity solve (несколько итераций для устойчивости стопок).
        for _ in 0..VELOCITY_ITERATIONS {
            for c in &contacts {
                resolve_velocity(c, &mut states);
            }
        }

        // 5. Position correction (Baumgarte).
        for c in &contacts {
            resolve_position(c, &mut states);
        }

        // 6. Sleep management.
        for s in states.iter_mut() {
            if s.body_type != BodyType::Dynamic { continue; }
            let speed = s.velocity.length();
            if speed < SLEEP_LINEAR_THRESHOLD {
                s.sleep_timer += dt;
                if s.sleep_timer > SLEEP_TIME_REQUIRED {
                    s.sleeping = true;
                    s.velocity = Vec3::ZERO;
                }
            } else {
                s.sleep_timer = 0.0;
                s.sleeping = false;
            }
        }

        // 7. Записать обратно в ECS.
        write_back(world, &states);
    }
}

fn collect_states(world: &World) -> Vec<BodyState> {
    let mut out = Vec::new();
    for &e in world.entities() {
        let Some(rb) = world.get::<RigidBody>(e).copied() else { continue };
        let Some(col) = world.get::<Collider>(e).copied() else { continue };
        let Some(t) = world.get::<Transform>(e) else { continue };

        if world.get::<Parent>(e).is_some() {
            log::warn!(
                "Physics: entity #{} has Parent — Parent ignored (physics works in world space)",
                e
            );
        }

        let material = world
            .get::<PhysicsMaterial>(e)
            .copied()
            .unwrap_or_default();

        let mass = rb.mass.max(1e-4);

        out.push(BodyState {
            entity: e,
            body_type: rb.body_type,
            mass,
            inv_mass: rb.inv_mass(),
            position: t.position,
            velocity: rb.velocity,
            gravity_scale: rb.gravity_scale,
            linear_damping: rb.linear_damping,
            collider: col,
            material,
            scale: t.scale.abs(),
            sleeping: rb.sleeping,
            sleep_timer: rb.sleep_timer,
        });
    }
    out
}

fn write_back(world: &mut World, states: &[BodyState]) {
    for s in states {
        if let Some(t) = world.get_mut::<Transform>(s.entity) {
            t.position = s.position;
        }
        if let Some(rb) = world.get_mut::<RigidBody>(s.entity) {
            rb.velocity = s.velocity;
            rb.sleeping = s.sleeping;
            rb.sleep_timer = s.sleep_timer;
            rb.force = Vec3::ZERO;
            rb.torque = Vec3::ZERO;
        }
    }
}

// ============================================================
// Broad-phase (O(n²) с early-exit)
// ============================================================
//
// Точка расширения: заменить на uniform grid / sweep-and-prune.
// Интерфейс (`Vec<(usize, usize)>`) останется тем же.

fn broad_phase(states: &[BodyState]) -> Vec<(usize, usize)> {
    let n = states.len();
    let aabbs: Vec<(Vec3, Vec3)> = states.iter().map(global_aabb).collect();
    let mut pairs = Vec::new();
    for i in 0..n {
        for j in (i + 1)..n {
            let si = &states[i];
            let sj = &states[j];
            if !can_move(si.body_type) && !can_move(sj.body_type) { continue; }
            if si.sleeping && sj.sleeping { continue; }
            if aabb_overlap(aabbs[i], aabbs[j]) {
                pairs.push((i, j));
            }
        }
    }
    pairs
}

fn can_move(bt: BodyType) -> bool {
    matches!(bt, BodyType::Dynamic | BodyType::Kinematic)
}

fn global_aabb(s: &BodyState) -> (Vec3, Vec3) {
    match s.collider {
        Collider::Sphere { radius } => {
            let r = radius * s.scale.max_element();
            (s.position - Vec3::splat(r), s.position + Vec3::splat(r))
        }
        Collider::Aabb { half_extents } => {
            let h = half_extents * s.scale;
            (s.position - h, s.position + h)
        }
        Collider::Capsule { radius, height } => {
            let r = radius * s.scale.max_element();
            let hy = height * s.scale.y * 0.5;
            let h = Vec3::new(r, hy + r, r);
            (s.position - h, s.position + h)
        }
    }
}

fn aabb_overlap(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
    a.0.x <= b.1.x && a.1.x >= b.0.x
        && a.0.y <= b.1.y && a.1.y >= b.0.y
        && a.0.z <= b.1.z && a.1.z >= b.0.z
}

// ============================================================
// Narrow-phase
// ============================================================
//
// Точка расширения: заменить на GJK/EPA для convex hulls.
// Интерфейс (`Option<Contact>`) останется тем же.

fn collide(a: &BodyState, b: &BodyState, ia: usize, ib: usize) -> Option<Contact> {
    match (a.collider, b.collider) {
        (Collider::Sphere { radius: ra }, Collider::Sphere { radius: rb }) => {
            let r_a = ra * a.scale.max_element();
            let r_b = rb * b.scale.max_element();
            sphere_sphere(a.position, r_a, b.position, r_b, ia, ib)
        }
        (Collider::Sphere { radius }, Collider::Aabb { half_extents }) => {
            let r = radius * a.scale.max_element();
            let hb = half_extents * b.scale;
            sphere_aabb(a.position, r, b.position, hb, ia, ib)
        }
        (Collider::Aabb { half_extents }, Collider::Sphere { radius }) => {
            let ha = half_extents * a.scale;
            let r = radius * b.scale.max_element();
            // Считаем как sphere_aabb, но нормаль инвертируем.
            let mut c = sphere_aabb(b.position, r, a.position, ha, ib, ia)?;
            c.normal = -c.normal;
            Some(Contact { a: ia, b: ib, normal: c.normal, penetration: c.penetration })
        }
        (Collider::Aabb { half_extents: ha }, Collider::Aabb { half_extents: hb }) => {
            let h_a = ha * a.scale;
            let h_b = hb * b.scale;
            aabb_aabb(a.position, h_a, b.position, h_b, ia, ib)
        }
        // Любая капсула — через AABB (простое приближение).
        _ => {
            let (pa, ha) = state_as_aabb(a);
            let (pb, hb) = state_as_aabb(b);
            aabb_aabb(pa, ha, pb, hb, ia, ib)
        }
    }
}

fn state_as_aabb(s: &BodyState) -> (Vec3, Vec3) {
    let (mn, mx) = global_aabb(s);
    (s.position, (mx - mn) * 0.5)
}

fn sphere_sphere(pa: Vec3, ra: f32, pb: Vec3, rb: f32, ia: usize, ib: usize) -> Option<Contact> {
    let d = pb - pa;
    let dist_sq = d.length_squared();
    let r_sum = ra + rb;
    if dist_sq >= r_sum * r_sum { return None; }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-6 { d / dist } else { Vec3::Y };
    let pen = r_sum - dist;
    Some(Contact { a: ia, b: ib, normal, penetration: pen })
}

fn sphere_aabb(pc: Vec3, r: f32, center: Vec3, half: Vec3, ia: usize, ib: usize) -> Option<Contact> {
    let closest = pc.clamp(center - half, center + half);
    let d = pc - closest;
    let dist_sq = d.length_squared();
    if dist_sq >= r * r { return None; }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-6 {
        -(d / dist)
    } else {
        // Сфера внутри AABB — выталкиваем по минимальной оси.
        let diff = pc - center;
        let dx = half.x - diff.x.abs();
        let dy = half.y - diff.y.abs();
        let dz = half.z - diff.z.abs();
        if dx < dy && dx < dz {
            Vec3::new(diff.x.signum(), 0.0, 0.0)
        } else if dy < dz {
            Vec3::new(0.0, diff.y.signum(), 0.0)
        } else {
            Vec3::new(0.0, 0.0, diff.z.signum())
        }
    };
    let pen = r - dist;
    Some(Contact { a: ia, b: ib, normal, penetration: pen })
}

fn aabb_aabb(pa: Vec3, ha: Vec3, pb: Vec3, hb: Vec3, ia: usize, ib: usize) -> Option<Contact> {
    let d = pb - pa;
    let ox = ha.x + hb.x - d.x.abs();
    let oy = ha.y + hb.y - d.y.abs();
    let oz = ha.z + hb.z - d.z.abs();
    if ox <= 0.0 || oy <= 0.0 || oz <= 0.0 { return None; }
    let (pen, normal) = if ox <= oy && ox <= oz {
        (ox, Vec3::new(d.x.signum(), 0.0, 0.0))
    } else if oy <= oz {
        (oy, Vec3::new(0.0, d.y.signum(), 0.0))
    } else {
        (oz, Vec3::new(0.0, 0.0, d.z.signum()))
    };
    Some(Contact { a: ia, b: ib, normal, penetration: pen })
}

// ============================================================
// Resolve
// ============================================================

fn resolve_velocity(c: &Contact, states: &mut [BodyState]) {
    let inv_m_a = states[c.a].inv_mass;
    let inv_m_b = states[c.b].inv_mass;
    let inv_sum = inv_m_a + inv_m_b;
    if inv_sum <= 0.0 { return; }

    let v_rel = states[c.b].velocity - states[c.a].velocity;
    let v_n = v_rel.dot(c.normal);
    if v_n > 0.0 { return; }

    let e = states[c.a]
        .material
        .restitution
        .min(states[c.b].material.restitution);

    let j = -(1.0 + e) * v_n / inv_sum;
    let impulse = c.normal * j;
    states[c.a].velocity -= impulse * inv_m_a;
    states[c.b].velocity += impulse * inv_m_b;

    // Friction (Coulomb).
    let v_rel = states[c.b].velocity - states[c.a].velocity;
    let v_n_after = v_rel.dot(c.normal);
    let v_t = v_rel - c.normal * v_n_after;
    let v_t_len = v_t.length();
    if v_t_len > 1e-6 {
        let tangent = v_t / v_t_len;
        let mu = (states[c.a].material.friction + states[c.b].material.friction) * 0.5;
        let jt_unclamped = -v_t.dot(tangent) / inv_sum;
        let max_friction = mu * j.abs();
        let jt = jt_unclamped.clamp(-max_friction, max_friction);
        let tan_impulse = tangent * jt;
        states[c.a].velocity -= tan_impulse * inv_m_a;
        states[c.b].velocity += tan_impulse * inv_m_b;
    }

    // Любое столкновение будит обоих.
    states[c.a].sleeping = false;
    states[c.a].sleep_timer = 0.0;
    states[c.b].sleeping = false;
    states[c.b].sleep_timer = 0.0;
}

fn resolve_position(c: &Contact, states: &mut [BodyState]) {
    let inv_m_a = states[c.a].inv_mass;
    let inv_m_b = states[c.b].inv_mass;
    let inv_sum = inv_m_a + inv_m_b;
    if inv_sum <= 0.0 { return; }

    let pen = (c.penetration - CONTACT_SLOP).max(0.0);
    if pen < 1e-6 { return; }

    let correction = c.normal * (BAUMGARTE * pen / inv_sum);
    states[c.a].position -= correction * inv_m_a;
    states[c.b].position += correction * inv_m_b;
}