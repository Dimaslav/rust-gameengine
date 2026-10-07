use std::collections::HashSet;

use glam::{Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{Parent, Transform};
use crate::render::terrain::Heightmap; // === TERRAIN ===

use super::components::{BodyType, Collider, PhysicsMaterial, RigidBody};

const SLEEP_LINEAR_THRESHOLD: f32 = 0.05;
const SLEEP_ANGULAR_THRESHOLD: f32 = 0.1;
const SLEEP_TIME_REQUIRED: f32 = 0.5;
const CONTACT_SLOP: f32 = 0.001;
const BAUMGARTE: f32 = 0.2;
const MAX_LINEAR_VELOCITY: f32 = 500.0;
const MAX_ANGULAR_VELOCITY: f32 = 50.0;
const VELOCITY_ITERATIONS: u32 = 4;

pub const MAX_DT: f32 = 0.05;

pub struct PhysicsWorld {
    pub gravity: Vec3,
    pub enabled: bool,
    pub last_broad_pairs: usize,
    pub last_contacts: usize,
    warned_parent_bodies: HashSet<Entity>,
}

impl Default for PhysicsWorld {
    fn default() -> Self {
        Self {
            gravity: Vec3::new(0.0, -9.81, 0.0),
            enabled: true,
            last_broad_pairs: 0,
            last_contacts: 0,
            warned_parent_bodies: HashSet::new(),
        }
    }
}

#[derive(Clone, Copy)]
struct BodyState {
    entity: Entity,
    body_type: BodyType,
    mass: f32,
    inv_mass: f32,
    inv_inertia: f32,
    position: Vec3,
    velocity: Vec3,
    rotation: Quat,
    angular_velocity: Vec3,
    gravity_scale: f32,
    linear_damping: f32,
    angular_damping: f32,
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
    point: Vec3,
}

impl PhysicsWorld {
    pub fn step(
        &mut self,
        world: &mut World,
        terrain: Option<&Heightmap>, // === TERRAIN ===
        dt: f32,
    ) {
        if !self.enabled || dt <= 0.0 { return; }
        let dt = dt.min(MAX_DT);

        let (mut states, skipped) = collect_states(world);
        self.warn_skipped_bodies(&skipped);

        if states.is_empty() {
            self.last_broad_pairs = 0;
            self.last_contacts = 0;
            return;
        }

        // 1. Интеграция.
        for s in states.iter_mut() {
            match s.body_type {
                BodyType::Dynamic if !s.sleeping => {
                    s.velocity += self.gravity * s.gravity_scale * dt;
                    let d = (1.0 - s.linear_damping * dt).max(0.0);
                    s.velocity *= d;
                    s.angular_velocity *= (1.0 - s.angular_damping * dt).max(0.0);

                    let vlen = s.velocity.length();
                    if vlen > MAX_LINEAR_VELOCITY {
                        s.velocity *= MAX_LINEAR_VELOCITY / vlen;
                    }
                    let wlen = s.angular_velocity.length();
                    if wlen > MAX_ANGULAR_VELOCITY {
                        s.angular_velocity *= MAX_ANGULAR_VELOCITY / wlen;
                    }

                    s.position += s.velocity * dt;
                    integrate_rotation(s, dt);
                }
                BodyType::Kinematic => {
                    s.position += s.velocity * dt;
                    integrate_rotation(s, dt);
                }
                _ => {}
            }
        }

        // === TERRAIN: вертикальная коллизия с heightmap ===
        // Простейший heightfield-контакт: по нижней точке AABB тела
        // берём высоту ground, выталкиваем по +Y, гасим vertical velocity.
        // Для инди-игр этого достаточно; для точного sliding по склону
        // нужен contact normal из heightmap (следующий шаг).
        if let Some(hm) = terrain {
            for s in states.iter_mut() {
                if s.body_type != BodyType::Dynamic { continue; }
                let (mn, _mx) = global_aabb(s);
                let feet = Vec3::new(s.position.x, mn.y, s.position.z);
                if !hm.contains(feet.x, feet.z) { continue; }
                let ground = hm.sample(feet.x, feet.z);
                if feet.y < ground {
                    let pen = ground - feet.y;
                    s.position.y += pen;
                    if s.velocity.y < 0.0 {
                        let restitution = s.material.restitution.min(0.5);
                        s.velocity.y = -s.velocity.y * restitution;
                    }
                    s.sleeping = false;
                    s.sleep_timer = 0.0;
                }
            }
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

        // 4. Velocity solve.
        for _ in 0..VELOCITY_ITERATIONS {
            for c in &contacts {
                resolve_velocity(c, &mut states);
            }
        }

        // 4b. Support transfer.
        for c in &contacts {
            let a_kin = states[c.a].body_type == BodyType::Kinematic;
            let b_kin = states[c.b].body_type == BodyType::Kinematic;
            let a_dyn = states[c.a].body_type == BodyType::Dynamic;
            let b_dyn = states[c.b].body_type == BodyType::Dynamic;

            if a_kin && b_dyn && c.normal.y > 0.5 {
                states[c.b].velocity = states[c.a].velocity;
                states[c.b].angular_velocity = states[c.a].angular_velocity;
                states[c.b].sleeping = false;
                states[c.b].sleep_timer = 0.0;
            }
            if b_kin && a_dyn && c.normal.y < -0.5 {
                states[c.a].velocity = states[c.b].velocity;
                states[c.a].angular_velocity = states[c.b].angular_velocity;
                states[c.a].sleeping = false;
                states[c.a].sleep_timer = 0.0;
            }
        }

        // 5. Position correction.
        for c in &contacts {
            resolve_position(c, &mut states);
        }

        // 6. Sleep management.
        for s in states.iter_mut() {
            if s.body_type != BodyType::Dynamic { continue; }
            let lin = s.velocity.length();
            let ang = s.angular_velocity.length();
            if lin < SLEEP_LINEAR_THRESHOLD && ang < SLEEP_ANGULAR_THRESHOLD {
                s.sleep_timer += dt;
                if s.sleep_timer > SLEEP_TIME_REQUIRED {
                    s.sleeping = true;
                    s.velocity = Vec3::ZERO;
                    s.angular_velocity = Vec3::ZERO;
                }
            } else {
                s.sleep_timer = 0.0;
                s.sleeping = false;
            }
        }

        // 7. Записать обратно.
        write_back(world, &states);
    }

    fn warn_skipped_bodies(&mut self, skipped: &[Entity]) {
        let now: HashSet<Entity> = skipped.iter().copied().collect();
        for &e in &now {
            if !self.warned_parent_bodies.contains(&e) {
                log::warn!(
                    "Physics: entity #{} has RigidBody + Parent — not simulated.",
                    e
                );
            }
        }
        self.warned_parent_bodies = now;
    }
}

fn integrate_rotation(s: &mut BodyState, dt: f32) {
    if s.angular_velocity.length_squared() < 1e-10 { return; }
    let w = s.angular_velocity;
    let wq = Quat::from_xyzw(w.x, w.y, w.z, 0.0);
    let dq = wq * s.rotation;
    let new_q = Quat::from_xyzw(
        s.rotation.x + dq.x * 0.5 * dt,
        s.rotation.y + dq.y * 0.5 * dt,
        s.rotation.z + dq.z * 0.5 * dt,
        s.rotation.w + dq.w * 0.5 * dt,
    );
    s.rotation = new_q.normalize();
}

fn compute_inv_inertia(col: &Collider, mass: f32, scale: Vec3) -> f32 {
    if mass < 1e-6 { return 0.0; }
    let i = match col {
        Collider::Sphere { radius } => {
            let r = radius * scale.max_element();
            0.4 * mass * r * r
        }
        Collider::Aabb { half_extents } => {
            let h = *half_extents * scale;
            mass * (h.x * h.x + h.y * h.y + h.z * h.z) / 3.0
        }
        Collider::Capsule { radius, height } => {
            let r = radius * scale.max_element();
            let h = height * scale.y * 0.5;
            mass * (r * r * 0.5 + h * h / 3.0)
        }
    };
    if i > 1e-8 { 1.0 / i } else { 0.0 }
}

fn collect_states(world: &World) -> (Vec<BodyState>, Vec<Entity>) {
    let mut out = Vec::new();
    let mut skipped = Vec::new();
    for &e in world.entities() {
        let Some(rb) = world.get::<RigidBody>(e).copied() else { continue };
        let Some(col) = world.get::<Collider>(e).copied() else { continue };

        if world.get::<Parent>(e).is_some() {
            if rb.body_type != BodyType::Static {
                skipped.push(e);
            }
            continue;
        }

        let Some(t) = world.get::<Transform>(e) else { continue };

        let material = world.get::<PhysicsMaterial>(e).copied().unwrap_or_default();
        let mass = rb.mass.max(1e-4);
        let scale = t.scale.abs();
        let inv_inertia = compute_inv_inertia(&col, mass, scale);

        out.push(BodyState {
            entity: e,
            body_type: rb.body_type,
            mass,
            inv_mass: rb.inv_mass(),
            inv_inertia,
            position: t.position,
            velocity: rb.velocity,
            rotation: t.rotation.normalize(),
            angular_velocity: rb.angular_velocity,
            gravity_scale: rb.gravity_scale,
            linear_damping: rb.linear_damping,
            angular_damping: rb.angular_damping,
            collider: col,
            material,
            scale,
            sleeping: rb.sleeping,
            sleep_timer: rb.sleep_timer,
        });
    }
    (out, skipped)
}

fn write_back(world: &mut World, states: &[BodyState]) {
    for s in states {
        if let Some(t) = world.get_mut::<Transform>(s.entity) {
            t.position = s.position;
            t.rotation = s.rotation;
        }
        if let Some(rb) = world.get_mut::<RigidBody>(s.entity) {
            rb.velocity = s.velocity;
            rb.angular_velocity = s.angular_velocity;
            rb.sleeping = s.sleeping;
            rb.sleep_timer = s.sleep_timer;
            rb.force = Vec3::ZERO;
            rb.torque = Vec3::ZERO;
        }
    }
}

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
    s.collider.world_aabb(s.position, s.rotation, s.scale)
}

fn aabb_overlap(a: (Vec3, Vec3), b: (Vec3, Vec3)) -> bool {
    a.0.x <= b.1.x && a.1.x >= b.0.x
        && a.0.y <= b.1.y && a.1.y >= b.0.y
        && a.0.z <= b.1.z && a.1.z >= b.0.z
}

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
            let mut c = sphere_aabb(b.position, r, a.position, ha, ib, ia)?;
            c.normal = -c.normal;
            Some(Contact { a: ia, b: ib, normal: c.normal, penetration: c.penetration, point: c.point })
        }
        (Collider::Aabb { half_extents: ha }, Collider::Aabb { half_extents: hb }) => {
            let h_a = ha * a.scale;
            let h_b = hb * b.scale;
            aabb_aabb(a.position, h_a, b.position, h_b, ia, ib)
        }
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

fn closest_point_on_aabb(p: Vec3, center: Vec3, half: Vec3) -> Vec3 {
    p.clamp(center - half, center + half)
}

fn sphere_sphere(pa: Vec3, ra: f32, pb: Vec3, rb: f32, ia: usize, ib: usize) -> Option<Contact> {
    let d = pb - pa;
    let dist_sq = d.length_squared();
    let r_sum = ra + rb;
    if dist_sq >= r_sum * r_sum { return None; }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-6 { d / dist } else { Vec3::Y };
    let pen = r_sum - dist;
    let point = pa + normal * (ra - pen * 0.5);
    Some(Contact { a: ia, b: ib, normal, penetration: pen, point })
}

fn sphere_aabb(pc: Vec3, r: f32, center: Vec3, half: Vec3, ia: usize, ib: usize) -> Option<Contact> {
    let closest = closest_point_on_aabb(pc, center, half);
    let d = pc - closest;
    let dist_sq = d.length_squared();
    if dist_sq >= r * r { return None; }
    let dist = dist_sq.sqrt();
    let normal = if dist > 1e-6 {
        -(d / dist)
    } else {
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
    let point = closest;
    Some(Contact { a: ia, b: ib, normal, penetration: pen, point })
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
    let point_a = closest_point_on_aabb(pb, pa, ha);
    let point_b = closest_point_on_aabb(pa, pb, hb);
    let point = (point_a + point_b) * 0.5;
    Some(Contact { a: ia, b: ib, normal, penetration: pen, point })
}

fn resolve_velocity(c: &Contact, states: &mut [BodyState]) {
    let inv_m_a = states[c.a].inv_mass;
    let inv_m_b = states[c.b].inv_mass;
    let inv_i_a = states[c.a].inv_inertia;
    let inv_i_b = states[c.b].inv_inertia;
    let inv_sum = inv_m_a + inv_m_b;
    if inv_sum <= 0.0 && inv_i_a <= 0.0 && inv_i_b <= 0.0 { return; }

    let ra = c.point - states[c.a].position;
    let rb = c.point - states[c.b].position;

    let va = states[c.a].velocity + states[c.a].angular_velocity.cross(ra);
    let vb = states[c.b].velocity + states[c.b].angular_velocity.cross(rb);
    let v_rel = vb - va;
    let v_n = v_rel.dot(c.normal);
    if v_n > 0.0 { return; }

    let ra_xn = ra.cross(c.normal);
    let rb_xn = rb.cross(c.normal);
    let ang_a = inv_i_a * ra_xn.length_squared();
    let ang_b = inv_i_b * rb_xn.length_squared();
    let k = inv_sum + ang_a + ang_b;
    if k <= 1e-8 { return; }

    let e = states[c.a].material.restitution.min(states[c.b].material.restitution);
    let j = -(1.0 + e) * v_n / k;
    let impulse = c.normal * j;

    states[c.a].velocity -= impulse * inv_m_a;
    states[c.b].velocity += impulse * inv_m_b;
    states[c.a].angular_velocity -= ra.cross(impulse) * inv_i_a;
    states[c.b].angular_velocity += rb.cross(impulse) * inv_i_b;

    let va = states[c.a].velocity + states[c.a].angular_velocity.cross(ra);
    let vb = states[c.b].velocity + states[c.b].angular_velocity.cross(rb);
    let v_rel = vb - va;
    let v_n_after = v_rel.dot(c.normal);
    let v_t = v_rel - c.normal * v_n_after;
    let v_t_len = v_t.length();
    if v_t_len > 1e-6 {
        let tangent = v_t / v_t_len;
        let ra_xt = ra.cross(tangent);
        let rb_xt = rb.cross(tangent);
        let kt = inv_sum + inv_i_a * ra_xt.length_squared() + inv_i_b * rb_xt.length_squared();
        if kt > 1e-8 {
            let jt_unclamped = -v_t.dot(tangent) / kt;
            let mu = (states[c.a].material.friction + states[c.b].material.friction) * 0.5;
            let max_friction = mu * j.abs();
            let jt = jt_unclamped.clamp(-max_friction, max_friction);
            let tan_impulse = tangent * jt;
            states[c.a].velocity -= tan_impulse * inv_m_a;
            states[c.b].velocity += tan_impulse * inv_m_b;
            states[c.a].angular_velocity -= ra.cross(tan_impulse) * inv_i_a;
            states[c.b].angular_velocity += rb.cross(tan_impulse) * inv_i_b;
        }
    }

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