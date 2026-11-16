use glam::Vec3;

use crate::ecs::{Entity, World};
use crate::game::components::{Parent, Transform, Visible};
use crate::game::world_matrix;

use std::sync::Arc;
use super::components::{BodyType, Collider, MeshCollider, RigidBody};

#[derive(Debug, Clone, Copy)]
pub struct QueryFilter {
    pub include_static: bool,
    pub include_dynamic: bool,
    pub include_kinematic: bool,
    pub visible_only: bool,
}

impl Default for QueryFilter {
    fn default() -> Self {
        Self {
            include_static: true,
            include_dynamic: true,
            include_kinematic: true,
            visible_only: true,
        }
    }
}

impl QueryFilter {
    pub fn all() -> Self { Self::default() }

    pub fn static_only() -> Self {
        Self { include_dynamic: false, include_kinematic: false, ..Self::default() }
    }

    pub fn dynamic_only() -> Self {
        Self { include_static: false, include_kinematic: false, ..Self::default() }
    }

    pub fn including_hidden(mut self) -> Self {
        self.visible_only = false;
        self
    }

    fn accepts(&self, bt: BodyType) -> bool {
        match bt {
            BodyType::Static => self.include_static,
            BodyType::Dynamic => self.include_dynamic,
            BodyType::Kinematic => self.include_kinematic,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct RayHit {
    pub entity: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub distance: f32,
}

struct ColliderSnapshot {
    entity: Entity,
    primitive: Option<Collider>,
    mesh: Option<Arc<MeshCollider>>,
    position: Vec3,
    scale: Vec3,
    rotation: glam::Quat,
}

fn collect(world: &World, filter: &QueryFilter) -> Vec<ColliderSnapshot> {
    let mut out = Vec::new();
    for &e in world.entities() {
        if filter.visible_only
            && let Some(v) = world.get::<Visible>(e)
                && !v.0 { continue; }
        let primitive = world.get::<Collider>(e).copied();
        let mesh = world.get::<MeshCollider>(e).cloned().map(Arc::new);
        if primitive.is_none() && mesh.is_none() { continue; }
        let Some(t) = world.get::<Transform>(e) else { continue };

        let bt = world.get::<RigidBody>(e).map(|rb| rb.body_type).unwrap_or(BodyType::Static);
        if !filter.accepts(bt) { continue; }

        let (position, rotation, scale) = if world.has::<Parent>(e) {
            let model = world_matrix(world, e);
            let (s, r, p) = model.to_scale_rotation_translation();
            (p, r, s.abs())
        } else {
            (t.position, t.rotation, t.scale.abs())
        };

        out.push(ColliderSnapshot {
            entity: e,
            primitive,
            mesh,
            position,
            scale,
            rotation,
        });
    }
    out
}

impl super::PhysicsWorld {
    pub fn raycast(
        &self,
        world: &World,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
        filter: QueryFilter,
    ) -> Option<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir.length_squared() < 1e-12 { return None; }
        let mut best: Option<RayHit> = None;
        for s in collect(world, &filter) {
            let Some((t, n)) = ray_collider(origin, dir, &s) else { continue };
            if t > max_dist { continue; }
            if best.is_none_or(|b| t < b.distance) {
                best = Some(RayHit {
                    entity: s.entity,
                    point: origin + dir * t,
                    normal: n,
                    distance: t,
                });
            }
        }
        best
    }

    pub fn raycast_all(
        &self,
        world: &World,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
        filter: QueryFilter,
    ) -> Vec<RayHit> {
        let dir = dir.normalize_or_zero();
        if dir.length_squared() < 1e-12 { return Vec::new(); }
        let mut hits = Vec::new();
        for s in collect(world, &filter) {
            let Some((t, n)) = ray_collider(origin, dir, &s) else { continue };
            if t > max_dist { continue; }
            hits.push(RayHit { entity: s.entity, point: origin + dir * t, normal: n, distance: t });
        }
        hits.sort_by(|a, b| a.distance.partial_cmp(&b.distance).unwrap_or(std::cmp::Ordering::Equal));
        hits
    }

    pub fn ray_blocked(
        &self,
        world: &World,
        origin: Vec3,
        dir: Vec3,
        max_dist: f32,
        filter: QueryFilter,
    ) -> bool {
        let dir = dir.normalize_or_zero();
        if dir.length_squared() < 1e-12 { return false; }
        for s in collect(world, &filter) {
            if let Some((t, _)) = ray_collider(origin, dir, &s)
                && t <= max_dist { return true; }
        }
        false
    }

    pub fn overlap_sphere(
        &self,
        world: &World,
        center: Vec3,
        radius: f32,
        filter: QueryFilter,
    ) -> Vec<Entity> {
        let mut out = Vec::new();
        let s_min = center - Vec3::splat(radius);
        let s_max = center + Vec3::splat(radius);
        for s in collect(world, &filter) {
            let (bmin, bmax) = world_aabb(&s);
            if !aabb_overlap(s_min, s_max, bmin, bmax) { continue; }
            if sphere_hits(center, radius, &s) { out.push(s.entity); }
        }
        out
    }

    pub fn overlap_aabb(
        &self,
        world: &World,
        min: Vec3,
        max: Vec3,
        filter: QueryFilter,
    ) -> Vec<Entity> {
        let mut out = Vec::new();
        for s in collect(world, &filter) {
            let (bmin, bmax) = world_aabb(&s);
            if aabb_overlap(min, max, bmin, bmax) { out.push(s.entity); }
        }
        out
    }

    pub fn overlap_capsule(
        &self,
        world: &World,
        a: Vec3,
        b: Vec3,
        radius: f32,
        filter: QueryFilter,
    ) -> Vec<Entity> {
        let mut out = Vec::new();
        let c = (a + b) * 0.5;
        let half = (b - a) * 0.5;
        let cap_min = c - half.abs() - Vec3::splat(radius);
        let cap_max = c + half.abs() + Vec3::splat(radius);
        for s in collect(world, &filter) {
            let (bmin, bmax) = world_aabb(&s);
            if !aabb_overlap(cap_min, cap_max, bmin, bmax) { continue; }
            if capsule_hits(a, b, radius, &s) { out.push(s.entity); }
        }
        out
    }
}

fn ray_collider(origin: Vec3, dir: Vec3, s: &ColliderSnapshot) -> Option<(f32, Vec3)> {
    // Проверяем primitive.
    let mut best: Option<(f32, Vec3)> = s.primitive.and_then(|c| ray_primitive(origin, dir, &c, s));

    // Проверяем mesh (если есть).
    if let Some(m) = &s.mesh
        && let Some((t, n)) = ray_mesh(origin, dir, m, s)
        && best.is_none_or(|(bt, _)| t < bt)
    {
        best = Some((t, n));
    }
    best
}

fn ray_primitive(
    origin: Vec3,
    dir: Vec3,
    col: &Collider,
    s: &ColliderSnapshot,
) -> Option<(f32, Vec3)> {
    match *col {
        Collider::Sphere { radius } => {
            ray_sphere(origin, dir, s.position, radius * s.scale.max_element())
        }
        Collider::Aabb { half_extents } => {
            let h = half_extents * s.scale;
            let inv_rot = s.rotation.inverse();
            let local_o = inv_rot * (origin - s.position);
            let local_d = inv_rot * dir;
            let (t, local_n) = ray_aabb(local_o, local_d, -h, h)?;
            let world_n = (s.rotation * local_n).normalize_or(Vec3::Y);
            Some((t, world_n))
        }
        Collider::Capsule { radius, height } => {
            let r = radius * s.scale.max_element();
            let hy = height * s.scale.y * 0.5;
            let up = s.rotation * Vec3::new(0.0, hy, 0.0);
            ray_capsule(
                origin, dir,
                s.position - up,
                s.position + up,
                r,
            )
        }
    }
}

fn ray_mesh(
    origin: Vec3,
    dir: Vec3,
    m: &MeshCollider,
    s: &ColliderSnapshot,
) -> Option<(f32, Vec3)> {
    let scale_inv = Vec3::new(
        if s.scale.x.abs() > 1e-6 { 1.0 / s.scale.x } else { 1.0 },
        if s.scale.y.abs() > 1e-6 { 1.0 / s.scale.y } else { 1.0 },
        if s.scale.z.abs() > 1e-6 { 1.0 / s.scale.z } else { 1.0 },
    );
    let inv_rot = s.rotation.inverse();
    let local_o = (origin - s.position) * scale_inv;
    let local_o = inv_rot * local_o;
    let local_d_unnorm = inv_rot * (dir * scale_inv);
    let local_d_len = local_d_unnorm.length();
    if local_d_len < 1e-12 { return None; }
    let local_d = local_d_unnorm / local_d_len;
    let triangles = m.triangles();
    if triangles.is_empty() { return None; }
    let bvh = m.bvh();
    let (tri_idx, t_local) = bvh.raycast(local_o, local_d, f32::INFINITY, &triangles)?;
    let tri = &triangles[tri_idx as usize];
    let e1 = tri[1] - tri[0];
    let e2 = tri[2] - tri[0];
    let n_local = e1.cross(e2).normalize_or(Vec3::Y);
    let n_scaled = Vec3::new(
        n_local.x / s.scale.x.max(1e-6),
        n_local.y / s.scale.y.max(1e-6),
        n_local.z / s.scale.z.max(1e-6),
    );
    let n_world = (s.rotation * n_scaled).normalize_or(Vec3::Y);
    let t_world = t_local / local_d_len;
    Some((t_world, n_world))
}

fn ray_sphere(origin: Vec3, dir: Vec3, center: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    let oc = origin - center;
    let b = oc.dot(dir);
    let c = oc.length_squared() - radius * radius;
    let disc = b * b - c;
    if disc < 0.0 { return None; }
    let sq = disc.sqrt();
    let t1 = -b - sq;
    let t2 = -b + sq;
    let t = if t1 > 1e-6 { t1 } else if t2 > 1e-6 { t2 } else { return None };
    let point = origin + dir * t;
    let n = (point - center).normalize_or_zero();
    Some((t, n))
}

fn ray_aabb(origin: Vec3, dir: Vec3, bmin: Vec3, bmax: Vec3) -> Option<(f32, Vec3)> {
    let mut tmin = 0.0f32;
    let mut tmax = f32::INFINITY;
    let mut normal = Vec3::ZERO;
    for i in 0..3 {
        let o = origin[i];
        let d = dir[i];
        if d.abs() < 1e-8 {
            if o < bmin[i] || o > bmax[i] { return None; }
        } else {
            let inv = 1.0 / d;
            let mut t1 = (bmin[i] - o) * inv;
            let mut t2 = (bmax[i] - o) * inv;
            let sign = if inv < 0.0 { -1.0 } else { 1.0 };
            if t1 > t2 { std::mem::swap(&mut t1, &mut t2); }
            if t1 > tmin {
                tmin = t1;
                let mut n = Vec3::ZERO;
                n[i] = sign;
                normal = n;
            }
            if t2 < tmax { tmax = t2; }
            if tmin > tmax { return None; }
        }
    }
    if tmin <= 1e-6 {
        return Some((0.0, -dir));
    }
    Some((tmin, normal))
}

fn ray_capsule(origin: Vec3, dir: Vec3, a: Vec3, b: Vec3, radius: f32) -> Option<(f32, Vec3)> {
    let ba = b - a;
    let oa = origin - a;
    let baba = ba.dot(ba);
    let bard = ba.dot(dir);
    let baoa = ba.dot(oa);
    let rdoa = dir.dot(oa);
    let oaoa = oa.dot(oa);
    let a_coef = baba - bard * bard;
    let b_coef = baba * rdoa - baoa * bard;
    let c_coef = baba * oaoa - baoa * baoa - radius * radius * baba;

    // Тело цилиндра.
    if a_coef.abs() > 1e-12 {
        let h = b_coef * b_coef - a_coef * c_coef;
        if h >= 0.0 {
            let t = (-b_coef - h.sqrt()) / a_coef;
            if t > 1e-6 {
                let y = baoa + t * bard;
                if y > 0.0 && y < baba {
                    let point = origin + dir * t;
                    let seg = a + ba * (y / baba);
                    let n = (point - seg).normalize_or_zero();
                    return Some((t, n));
                }
            }
        }
    }

    let mut best: Option<(f32, Vec3)> = None;
    for (center, sgn) in [(a, -1.0f32), (b, 1.0)] {
        if let Some((t, n)) = ray_sphere(origin, dir, center, radius) {
            let pt = origin + dir * t;
            let along = (pt - a).dot(ba) / baba.max(1e-12);
            let on_cap = if sgn < 0.0 { along <= 0.0 } else { along >= 1.0 };
            if !on_cap { continue; }
            if best.is_none_or(|(bt, _)| t < bt) {
                best = Some((t, n));
            }
        }
    }
    best
}

fn world_aabb(s: &ColliderSnapshot) -> (Vec3, Vec3) {
    let mut mn = Vec3::splat(f32::INFINITY);
    let mut mx = Vec3::splat(f32::NEG_INFINITY);
    if let Some(col) = &s.primitive {
        let (a, b) = col.world_aabb(s.position, s.rotation, s.scale);
        mn = mn.min(a);
        mx = mx.max(b);
    }
    if let Some(m) = &s.mesh {
        let (a, b) = m.world_aabb(s.position, s.rotation, s.scale);
        mn = mn.min(a);
        mx = mx.max(b);
    }
    (mn, mx)
}

fn sphere_hits(center: Vec3, radius: f32, s: &ColliderSnapshot) -> bool {
    if let Some(col) = &s.primitive {
        if sphere_hits_primitive(center, radius, col, s) {
            return true;
        }
    }
    if let Some(m) = &s.mesh {
        if sphere_hits_mesh(center, radius, m, s) {
            return true;
        }
    }
    false
}

fn sphere_hits_primitive(center: Vec3, radius: f32, col: &Collider, s: &ColliderSnapshot) -> bool {
    match *col {
        Collider::Sphere { radius: r } => {
            let rr = r * s.scale.max_element();
            (center - s.position).length_squared() < (radius + rr) * (radius + rr)
        }
        Collider::Aabb { half_extents } => {
            let h = half_extents * s.scale;
            let inv_rot = s.rotation.inverse();
            let local_center = inv_rot * (center - s.position);
            let p = local_center.clamp(-h, h);
            (local_center - p).length_squared() < radius * radius
        }
        Collider::Capsule { radius: r, height } => {
            let rr = r * s.scale.max_element();
            let hy = height * s.scale.y * 0.5;
            let up = s.rotation * Vec3::new(0.0, hy, 0.0);
            let a = s.position - up;
            let b = s.position + up;
            let p = closest_on_segment(center, a, b);
            (center - p).length_squared() < (radius + rr) * (radius + rr)
        }
    }
}


fn sphere_hits_mesh(
    center: Vec3,
    radius: f32,
    m: &MeshCollider,
    s: &ColliderSnapshot,
) -> bool {
    let (wmn, wmx) = m.world_aabb(s.position, s.rotation, s.scale);
    if center.x + radius < wmn.x || center.x - radius > wmx.x
        || center.y + radius < wmn.y || center.y - radius > wmx.y
        || center.z + radius < wmn.z || center.z - radius > wmx.z
    {
        return false;
    }

    let scale_inv = Vec3::new(
        if s.scale.x.abs() > 1e-6 { 1.0 / s.scale.x } else { 1.0 },
        if s.scale.y.abs() > 1e-6 { 1.0 / s.scale.y } else { 1.0 },
        if s.scale.z.abs() > 1e-6 { 1.0 / s.scale.z } else { 1.0 },
    );
    let inv_rot = s.rotation.inverse();
    let local_c = inv_rot * ((center - s.position) * scale_inv);
    let scale_max = s.scale.max_element();
    let local_r = radius / scale_max.max(1e-6);
    let triangles = m.triangles();
    let bvh = m.bvh();
    for tri in &triangles {
        if sphere_triangle_hit(local_c, local_r, tri[0], tri[1], tri[2]) {
            return true;
        }
    }
    let _ = bvh;
    false
}

fn sphere_triangle_hit(center: Vec3, radius: f32, a: Vec3, b: Vec3, c: Vec3) -> bool {
    let r2 = radius * radius;
    if (center - a).length_squared() < r2 { return true; }
    if (center - b).length_squared() < r2 { return true; }
    if (center - c).length_squared() < r2 { return true; }
    let e1 = b - a;
    let e2 = c - a;
    let n = e1.cross(e2);
    let n_len2 = n.length_squared();
    if n_len2 < 1e-12 { return false; }
    let dist = (center - a).dot(n);
    let d2 = dist * dist / n_len2;
    if d2 > r2 { return false; }
    let proj = center - n * (dist / n_len2);
    let v0 = e1;
    let v1 = e2;
    let v2 = proj - a;
    let d00 = v0.dot(v0);
    let d01 = v0.dot(v1);
    let d11 = v1.dot(v1);
    let d20 = v2.dot(v0);
    let d21 = v2.dot(v1);
    let denom = d00 * d11 - d01 * d01;
    if denom.abs() < 1e-12 { return false; }
    let u = (d11 * d20 - d01 * d21) / denom;
    let v = (d00 * d21 - d01 * d20) / denom;
    u >= 0.0 && v >= 0.0 && u + v <= 1.0
}

fn capsule_hits(a: Vec3, b: Vec3, radius: f32, s: &ColliderSnapshot) -> bool {
    let samples = 8;
    for i in 0..=samples {
        let t = i as f32 / samples as f32;
        let p = a.lerp(b, t);
        if sphere_hits(p, radius, s) { return true; }
    }
    false
}

fn closest_on_segment(p: Vec3, a: Vec3, b: Vec3) -> Vec3 {
    let ab = b - a;
    let t = (p - a).dot(ab) / ab.length_squared().max(1e-12);
    a + ab * t.clamp(0.0, 1.0)
}

#[inline]
fn aabb_overlap(amin: Vec3, amax: Vec3, bmin: Vec3, bmax: Vec3) -> bool {
    amin.x <= bmax.x && amax.x >= bmin.x
        && amin.y <= bmax.y && amax.y >= bmin.y
        && amin.z <= bmax.z && amax.z >= bmin.z
}