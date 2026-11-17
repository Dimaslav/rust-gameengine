use std::sync::Arc;

use glam::{Mat4, Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{Transform, Visible};
use crate::physics::{BodyType, Collider, MeshCollider, RigidBody};

use super::spatial_hash::SpatialHash2D;

const MIN_SCALE: f32 = 1e-4;
const MAX_SCALE: f32 = 1e6;
const MIN_DET: f32 = 1e-12;
const SHEAR_EPS_ABS: f32 = 1e-3;
const SHEAR_EPS_REL: f32 = 1e-3;

pub struct ColliderEntry {
    pub entity: Entity,
    pub body_type: BodyType,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
    pub model: Mat4,
    pub world_pos: Vec3,
    pub world_rot: Quat,
    pub world_scale: Vec3,
    pub collider: Option<Collider>,
    pub mesh: Option<Arc<MeshCollider>>,
    pub primitive_aabb: Option<(Vec3, Vec3)>,
    pub mesh_aabb: Option<(Vec3, Vec3)>,
}

pub struct ColliderCache {
    entries: Vec<ColliderEntry>,
    hash: SpatialHash2D,
}

impl ColliderCache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            hash: SpatialHash2D::new(4.0),
        }
    }

    pub fn build(&mut self, world: &World) {
        self.entries.clear();
        self.hash.clear();

        for &e in world.entities() {
            if let Some(v) = world.get::<Visible>(e)
                && !v.0
            {
                continue;
            }

            let primitive = world.get::<Collider>(e).copied();
            let mesh = world.get::<MeshCollider>(e).cloned();
            if primitive.is_none() && mesh.is_none() {
                continue;
            }
            if world.get::<Transform>(e).is_none() {
                continue;
            }

            if let Some(c) = &primitive
                && let Err(reason) = validate_collider(c)
            {
                log::error!("ColliderCache: entity #{} skipped: {}", e, reason);
                continue;
            }
            if let Some(m) = &mesh
                && (m.vertices.is_empty() || m.indices.is_empty())
            {
                log::error!(
                    "ColliderCache: entity #{} skipped: MeshCollider empty",
                    e
                );
                continue;
            }

            let body_type = world
                .get::<RigidBody>(e)
                .map(|rb| rb.body_type)
                .unwrap_or(BodyType::Static);

            let model = crate::game::world_matrix(world, e);
            if !model.is_finite() {
                log::error!(
                    "ColliderCache: entity #{} skipped: model not finite",
                    e
                );
                continue;
            }

            let det = model.determinant();
            if det.abs() < MIN_DET {
                log::error!(
                    "ColliderCache: entity #{} skipped: |det| = {:.3e} < {}",
                    e,
                    det.abs(),
                    MIN_DET
                );
                continue;
            }

            if det < 0.0 {
                log::error!(
                    "ColliderCache: entity #{} skipped: negative determinant \
                     (reflection, det = {:.3e}). Не поддерживается.",
                    e,
                    det
                );
                continue;
            }

            let (s, r, p) = model.to_scale_rotation_translation();

            let world_scale = s.abs();
            if world_scale.min_element() < MIN_SCALE {
                log::error!(
                    "ColliderCache: entity #{} skipped: min(scale) = {:.3e} < {}",
                    e,
                    world_scale.min_element(),
                    MIN_SCALE
                );
                continue;
            }
            if world_scale.max_element() > MAX_SCALE {
                log::error!(
                    "ColliderCache: entity #{} skipped: max(scale) = {:.3e} > {}",
                    e,
                    world_scale.max_element(),
                    MAX_SCALE
                );
                continue;
            }

            let reconstructed = Mat4::from_scale_rotation_translation(s, r, p);
            let shear_diff = (model - reconstructed)
                .to_cols_array()
                .iter()
                .map(|x| x.abs())
                .fold(0.0f32, f32::max);
            let model_mag = model
                .to_cols_array()
                .iter()
                .map(|x| x.abs())
                .fold(0.0f32, f32::max);
            let shear_threshold = SHEAR_EPS_ABS + SHEAR_EPS_REL * model_mag;
            if shear_diff > shear_threshold {
                log::error!(
                    "ColliderCache: entity #{} skipped: shear (diff = {:.4}, \
                     threshold = {:.4}). Не поддерживается.",
                    e,
                    shear_diff,
                    shear_threshold
                );
                continue;
            }

            let primitive_aabb = primitive
                .as_ref()
                .map(|c| c.world_aabb(p, r, world_scale));
            let mesh_aabb = mesh
                .as_ref()
                .map(|m| m.world_aabb(p, r, world_scale));

            for (label, aabb) in [
                ("primitive", primitive_aabb),
                ("mesh", mesh_aabb),
            ] {
                if let Some((mn, mx)) = aabb
                    && (!mn.is_finite()
                        || !mx.is_finite()
                        || mn.x > mx.x + 1e-6
                        || mn.y > mx.y + 1e-6
                        || mn.z > mx.z + 1e-6)
                {
                    log::error!(
                        "ColliderCache: entity #{} skipped: bad {} AABB \
                         (min = {:?}, max = {:?})",
                        e,
                        label,
                        mn,
                        mx
                    );
                }
            }

            let (aabb_min, aabb_max) = match (primitive_aabb, mesh_aabb) {
                (Some((a1, b1)), Some((a2, b2))) => (a1.min(a2), b1.max(b2)),
                (Some((a, b)), None) => (a, b),
                (None, Some((a, b))) => (a, b),
                (None, None) => unreachable!(),
            };

            if !aabb_min.is_finite()
                || !aabb_max.is_finite()
                || aabb_min.x > aabb_max.x + 1e-6
                || aabb_min.y > aabb_max.y + 1e-6
                || aabb_min.z > aabb_max.z + 1e-6
            {
                log::error!(
                    "ColliderCache: entity #{} skipped: combined AABB invalid",
                    e
                );
                continue;
            }

            let idx = u32::try_from(self.entries.len())
                .expect("ColliderCache: more than 4 billion colliders");

            self.entries.push(ColliderEntry {
                entity: e,
                body_type,
                aabb_min,
                aabb_max,
                model,
                world_pos: p,
                world_rot: r,
                world_scale,
                collider: primitive,
                mesh: mesh.map(Arc::new),
                primitive_aabb,
                mesh_aabb,
            });

            self.hash.insert_aabb(idx, aabb_min, aabb_max);
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
    pub fn entries(&self) -> &[ColliderEntry] {
        &self.entries
    }
    pub fn query_aabb(&self, min: Vec3, max: Vec3, out: &mut Vec<u32>) {
        self.hash.query_aabb(min, max, out);
    }

    pub fn capsule_hits(
        &self,
        a: Vec3,
        b: Vec3,
        r: f32,
        include_dynamic: bool,
    ) -> bool {
        self.capsule_hits_except(a, b, r, include_dynamic, None)
    }

    pub fn capsule_hits_except(
        &self,
        a: Vec3,
        b: Vec3,
        r: f32,
        include_dynamic: bool,
        except: Option<Entity>,
    ) -> bool {
        if !a.is_finite() || !b.is_finite() || !r.is_finite() || r < 0.0 {
            return false;
        }
        let mn = a.min(b) - Vec3::splat(r);
        let mx = a.max(b) + Vec3::splat(r);
        let mut cand = Vec::with_capacity(16);
        self.hash.query_aabb(mn, mx, &mut cand);

        for &i in &cand {
            let e = &self.entries[i as usize];
            if !include_dynamic && e.body_type == BodyType::Dynamic {
                continue;
            }
            if let Some(ex) = except
                && ex == e.entity
            {
                continue;
            }

            if !capsule_may_overlap_aabb(a, b, r, e.aabb_min, e.aabb_max) {
                continue;
            }

            if let Some((pa, pb)) = e.primitive_aabb {
                if capsule_may_overlap_aabb(a, b, r, pa, pb)
                    && let Some(col) = &e.collider
                    && capsule_hits_primitive(a, b, r, col, e)
                {
                    return true;
                }
            }

            // Narrow-phase 2: mesh (pre-check уже внутри).
            if let Some(mesh) = &e.mesh
                && capsule_hits_mesh(a, b, r, mesh, e)
            {
                return true;
            }
        }
        false
    }
}

impl Default for ColliderCache {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================
// Валидация коллайдера (C1.2)
// ============================================================

fn validate_collider(c: &Collider) -> Result<(), String> {
    match *c {
        Collider::Sphere { radius } => {
            if !radius.is_finite() || radius <= 0.0 {
                return Err(format!("sphere.radius = {} (must be > 0)", radius));
            }
        }
        Collider::Aabb { half_extents } => {
            if !half_extents.is_finite()
                || half_extents.x <= 0.0
                || half_extents.y <= 0.0
                || half_extents.z <= 0.0
            {
                return Err(format!(
                    "aabb.half_extents = {:?} (must be all > 0)",
                    half_extents
                ));
            }
        }
        Collider::Capsule { radius, height } => {
            if !radius.is_finite() || radius <= 0.0 {
                return Err(format!("capsule.radius = {} (must be > 0)", radius));
            }
            if !height.is_finite() || height <= 0.0 {
                return Err(format!("capsule.height = {} (must be > 0)", height));
            }
        }
    }
    Ok(())
}

// ============================================================
// Broad-phase: segment-AABB (консервативный)
// ============================================================

/// **Broad phase**: капсула [a, b] радиуса r vs AABB. Консервативен.
///
/// **Нет отдельной ветки для коротких отрезков** (C1.2 после ревью #2):
/// slab-test корректно обрабатывает и нулевые, и короткие отрезки.
/// Раньше было `if d² < 1e-12 { проверить a или b }` — это давало false
/// negative, когда оба конца снаружи, а отрезок внутри.
pub fn capsule_may_overlap_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
    if r < 0.0 {
        return false;
    }
    let emin = amin - Vec3::splat(r);
    let emax = amax + Vec3::splat(r);

    let d = b - a;
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;
    for i in 0..3 {
        let di = d[i];
        let ai = a[i];
        if di.abs() < 1e-12 {
            if ai < emin[i] || ai > emax[i] {
                return false;
            }
        } else {
            let inv = 1.0 / di;
            let mut t1 = (emin[i] - ai) * inv;
            let mut t2 = (emax[i] - ai) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return false;
            }
        }
    }
    true
}

/// Совместимость: старое имя.
#[inline]
pub fn capsule_hits_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
    capsule_may_overlap_aabb(a, b, r, amin, amax)
}

// ============================================================
// Narrow-phase 1: primitive
// ============================================================

fn capsule_hits_primitive(
    a: Vec3,
    b: Vec3,
    r: f32,
    col: &Collider,
    e: &ColliderEntry,
) -> bool {
    let scale = e.world_scale;
    let inv_rot = e.world_rot.inverse();

    match *col {
        Collider::Sphere { radius } => {
            let world_r = radius * scale.max_element();
            let rr = r + world_r;
            segment_point_dist_sq(a, b, e.world_pos) <= rr * rr
        }
        Collider::Aabb { half_extents } => {
            let local_a = inv_rot * (a - e.world_pos);
            let local_b = inv_rot * (b - e.world_pos);
            let half = half_extents * scale;
            segment_aabb_dist_sq(local_a, local_b, -half, half) <= r * r
        }
        Collider::Capsule { radius, height } => {
            let world_r = radius * scale.max_element();
            let hy = height * scale.y * 0.5;
            let up = e.world_rot * Vec3::new(0.0, hy, 0.0);
            let seg_a = e.world_pos - up;
            let seg_b = e.world_pos + up;
            let rr = r + world_r;
            segment_segment_dist_sq(a, b, seg_a, seg_b) <= rr * rr
        }
    }
}

// ============================================================
// Narrow-phase 2: mesh
// ============================================================

fn capsule_hits_mesh(
    a: Vec3,
    b: Vec3,
    r: f32,
    mesh: &MeshCollider,
    e: &ColliderEntry,
) -> bool {
    let Some((wmn, wmx)) = e.mesh_aabb else {
        return false;
    };
    let qmin = a.min(b) - Vec3::splat(r);
    let qmax = a.max(b) + Vec3::splat(r);
    if qmax.x < wmn.x
        || qmin.x > wmx.x
        || qmax.y < wmn.y
        || qmin.y > wmx.y
        || qmax.z < wmn.z
        || qmin.z > wmx.z
    {
        return false;
    }

    // scale валидирован в build (≥ MIN_SCALE), safe_inv не даст 1.0.
    let scale_inv = safe_inv(e.world_scale);
    let inv_rot = e.world_rot.inverse();
    let local_a = (inv_rot * (a - e.world_pos)) * scale_inv;
    let local_b = (inv_rot * (b - e.world_pos)) * scale_inv;
    let local_r = r / e.world_scale.min_element();

    let triangles = mesh.triangles();
    if triangles.is_empty() {
        return false;
    }
    let bvh = mesh.bvh();

    let cmin = local_a.min(local_b) - Vec3::splat(local_r);
    let cmax = local_a.max(local_b) + Vec3::splat(local_r);
    let r2 = r * r;
    let mut hit = false;

    bvh.for_each_in_aabb(cmin, cmax, |tri_idx| {
        let Some(tri) = triangles.get(tri_idx as usize) else {
            debug_assert!(false, "BVH returned invalid tri index {}", tri_idx);
            return true;
        };
        let wa = e.model.transform_point3(tri[0]);
        let wb = e.model.transform_point3(tri[1]);
        let wc = e.model.transform_point3(tri[2]);
        if segment_triangle_dist_sq(a, b, wa, wb, wc) <= r2 {
            hit = true;
            return false;
        }
        true
    });
    hit
}

// ============================================================
// Helpers
// ============================================================

#[inline]
fn safe_inv(v: Vec3) -> Vec3 {
    // После валидации в build все элементы ≥ MIN_SCALE > 1e-6.
    // Fallback 1.0 — защита от прямого вызова, не для боевого пути.
    Vec3::new(
        if v.x.abs() > 1e-6 { 1.0 / v.x } else { 1.0 },
        if v.y.abs() > 1e-6 { 1.0 / v.y } else { 1.0 },
        if v.z.abs() > 1e-6 { 1.0 / v.z } else { 1.0 },
    )
}

#[inline]
fn segment_point_dist_sq(a: Vec3, b: Vec3, p: Vec3) -> f32 {
    let ab = b - a;
    let len2 = ab.length_squared();
    if len2 < 1e-12 {
        return (p - a).length_squared();
    }
    let t = ((p - a).dot(ab) / len2).clamp(0.0, 1.0);
    let closest = a + ab * t;
    (p - closest).length_squared()
}

fn segment_segment_dist_sq(p1: Vec3, q1: Vec3, p2: Vec3, q2: Vec3) -> f32 {
    let d1 = q1 - p1;
    let d2 = q2 - p2;
    let r = p1 - p2;
    let a = d1.dot(d1);
    let e = d2.dot(d2);
    let f = d2.dot(r);

    let (mut s, mut t): (f32, f32);
    if a <= 1e-12 && e <= 1e-12 {
        return (p1 - p2).length_squared();
    }
    if a <= 1e-12 {
        s = 0.0;
        t = (f / e).clamp(0.0, 1.0);
    } else {
        let c = d1.dot(r);
        if e <= 1e-12 {
            t = 0.0;
            s = (-c / a).clamp(0.0, 1.0);
        } else {
            let b = d1.dot(d2);
            let denom = a * e - b * b;
            s = if denom.abs() > 1e-12 {
                ((b * f - c * e) / denom).clamp(0.0, 1.0)
            } else {
                0.0
            };
            t = (b * s + f) / e;
            if t < 0.0 {
                t = 0.0;
                s = (-c / a).clamp(0.0, 1.0);
            } else if t > 1.0 {
                t = 1.0;
                s = ((b - c) / a).clamp(0.0, 1.0);
            }
        }
    }
    let c1 = p1 + d1 * s;
    let c2 = p2 + d2 * t;
    (c1 - c2).length_squared()
}

/// C1.2: точно, минимальное squared-расстояние от segment до AABB.
/// Через 12 треугольников (2 на грань). Покрывает грани и рёбра.
///
/// **`best < 1e-12 → 0.0` убрано** — возвращаем точное значение.
/// Касание при r=0 обрабатывается сравнением `<=` у вызывающего.
fn segment_aabb_dist_sq(a: Vec3, b: Vec3, amin: Vec3, amax: Vec3) -> f32 {
    if segment_inside_aabb(a, b, amin, amax) {
        return 0.0;
    }
    let c0 = Vec3::new(amin.x, amin.y, amin.z);
    let c1 = Vec3::new(amax.x, amin.y, amin.z);
    let c2 = Vec3::new(amin.x, amax.y, amin.z);
    let c3 = Vec3::new(amax.x, amax.y, amin.z);
    let c4 = Vec3::new(amin.x, amin.y, amax.z);
    let c5 = Vec3::new(amax.x, amin.y, amax.z);
    let c6 = Vec3::new(amin.x, amax.y, amax.z);
    let c7 = Vec3::new(amax.x, amax.y, amax.z);
    let corners = [c0, c1, c2, c3, c4, c5, c6, c7];
    let tris = [
        (0, 1, 5), (0, 5, 4),
        (2, 6, 7), (2, 7, 3),
        (0, 2, 3), (0, 3, 1),
        (4, 5, 7), (4, 7, 6),
        (0, 4, 6), (0, 6, 2),
        (1, 3, 7), (1, 7, 5),
    ];
    let mut best = f32::INFINITY;
    for (i, j, k) in tris {
        let d = segment_triangle_dist_sq(a, b, corners[i], corners[j], corners[k]);
        if d < best {
            best = d;
        }
    }
    best
}

fn segment_inside_aabb(a: Vec3, b: Vec3, amin: Vec3, amax: Vec3) -> bool {
    let d = b - a;
    let mut tmin = 0.0f32;
    let mut tmax = 1.0f32;
    for i in 0..3 {
        if d[i].abs() < 1e-12 {
            if a[i] < amin[i] || a[i] > amax[i] {
                return false;
            }
        } else {
            let inv = 1.0 / d[i];
            let mut t1 = (amin[i] - a[i]) * inv;
            let mut t2 = (amax[i] - a[i]) * inv;
            if t1 > t2 {
                std::mem::swap(&mut t1, &mut t2);
            }
            tmin = tmin.max(t1);
            tmax = tmax.min(t2);
            if tmin > tmax {
                return false;
            }
        }
    }
    true
}

fn segment_triangle_dist_sq(p0: Vec3, p1: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f32 {
    if segment_triangle_intersect(p0, p1, a, b, c) {
        return 0.0;
    }
    let dp0 = point_triangle_dist_sq(p0, a, b, c);
    let dp1 = point_triangle_dist_sq(p1, a, b, c);
    let de0 = segment_segment_dist_sq(p0, p1, a, b);
    let de1 = segment_segment_dist_sq(p0, p1, b, c);
    let de2 = segment_segment_dist_sq(p0, p1, c, a);
    dp0.min(dp1).min(de0).min(de1).min(de2)
}

fn segment_triangle_intersect(p0: Vec3, p1: Vec3, a: Vec3, b: Vec3, c: Vec3) -> bool {
    let e1 = b - a;
    let e2 = c - a;
    let d = p1 - p0;
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-10 {
        return false;
    }
    let inv_det = 1.0 / det;
    let tvec = p0 - a;
    let u = tvec.dot(p) * inv_det;
    if !(0.0..=1.0).contains(&u) {
        return false;
    }
    let q = tvec.cross(e1);
    let v = d.dot(q) * inv_det;
    if v < 0.0 || u + v > 1.0 {
        return false;
    }
    let t = e2.dot(q) * inv_det;
    (0.0..=1.0).contains(&t)
}

/// Точка ↔ треугольник. **C1.2 после ревью #2**: масштабно-относительный
/// порог вырожденности. `n2 < ab² · ac² · 1e-8` означает
/// `sin²(angle) < 1e-8` (= sin < 1e-4, угол < 0.006°). Маленькие, но
/// валидные треугольники (1e-4 м) больше не считаются вырожденными.
fn point_triangle_dist_sq(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f32 {
    let ab = b - a;
    let ac = c - a;
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    let ab2 = ab.length_squared();
    let ac2 = ac.length_squared();

    // Вырожденность: нулевое ребро ИЛИ синус угла < 1e-4.
    if ab2 < 1e-20 || ac2 < 1e-20 || n2 < ab2 * ac2 * 1e-8 {
        return segment_point_dist_sq(a, b, p)
            .min(segment_point_dist_sq(b, c, p))
            .min(segment_point_dist_sq(c, a, p));
    }

    let ap = p - a;
    let d1 = ab.dot(ap);
    let d2 = ac.dot(ap);
    if d1 <= 0.0 && d2 <= 0.0 {
        return ap.length_squared();
    }

    let bp = p - b;
    let d3 = ab.dot(bp);
    let d4 = ac.dot(bp);
    if d3 >= 0.0 && d4 <= d3 {
        return bp.length_squared();
    }

    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        let closest = a + v * ab;
        return (p - closest).length_squared();
    }

    let cp = p - c;
    let d5 = ab.dot(cp);
    let d6 = ac.dot(cp);
    if d6 >= 0.0 && d5 <= d6 {
        return cp.length_squared();
    }

    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        let closest = a + w * ac;
        return (p - closest).length_squared();
    }

    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        let closest = b + w * (c - b);
        return (p - closest).length_squared();
    }

    let d = ap.dot(n);
    d * d / n2
}

// ============================================================
// Тесты
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::components::Transform;
    use crate::physics::RigidBody;

    fn spawn_box(world: &mut World, pos: Vec3, half: Vec3, dynamic: bool) -> Entity {
        let e = world.spawn();
        world.insert(e, Transform::at(pos));
        world.insert(e, Collider::aabb(half));
        world.insert(
            e,
            if dynamic {
                RigidBody::dynamic(1.0)
            } else {
                RigidBody::static_body()
            },
        );
        e
    }

    #[test]
    fn empty_world_builds_empty_cache() {
        let w = World::new();
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y, 0.5, false));
    }

    #[test]
    fn build_ignores_entities_without_collider() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Transform::at(Vec3::ZERO));
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(cache.is_empty());
    }

    #[test]
    fn build_ignores_hidden_entities() {
        let mut w = World::new();
        let hidden = spawn_box(&mut w, Vec3::ZERO, Vec3::splat(0.5), false);
        w.insert(hidden, Visible(false));
        spawn_box(&mut w, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(0.5), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.entries()[0].world_pos, Vec3::new(10.0, 0.0, 0.0));
    }

    /// C1.2: negative scale → entity отклоняется.
    #[test]
    fn build_rejects_negative_scale() {
        let mut w = World::new();
        let e = w.spawn();
        let mut t = Transform::at(Vec3::ZERO);
        t.scale = Vec3::new(-1.0, 1.0, 1.0);
        w.insert(e, t);
        w.insert(e, Collider::aabb(Vec3::splat(0.5)));
        w.insert(e, RigidBody::static_body());
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 0);
    }

    /// C1.2 после ревью #2: shear → entity отклоняется.
    #[test]
    fn build_rejects_shear() {
        let mut w = World::new();
        let parent = w.spawn();
        let mut pt = Transform::at(Vec3::ZERO);
        pt.scale = Vec3::new(2.0, 1.0, 1.0); // non-uniform
        w.insert(parent, pt);

        let child = w.spawn();
        let mut ct = Transform::at(Vec3::new(0.0, 0.0, 0.0));
        ct.rotation = glam::Quat::from_rotation_z(std::f32::consts::FRAC_PI_4);
        w.insert(child, ct);
        w.insert(child, crate::game::components::Parent(parent));
        w.insert(child, Collider::aabb(Vec3::splat(0.5)));
        w.insert(child, RigidBody::static_body());

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 0, "shear must be rejected");
    }

    #[test]
    fn build_rejects_zero_scale() {
        let mut w = World::new();
        let e = w.spawn();
        let mut t = Transform::at(Vec3::ZERO);
        t.scale = Vec3::new(0.0, 1.0, 1.0);
        w.insert(e, t);
        w.insert(e, Collider::aabb(Vec3::splat(0.5)));
        w.insert(e, RigidBody::static_body());
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 0);
    }

    #[test]
    fn build_rejects_invalid_collider() {
        let mut w = World::new();
        let e1 = w.spawn();
        w.insert(e1, Transform::at(Vec3::ZERO));
        w.insert(e1, Collider::Sphere { radius: -1.0 });
        w.insert(e1, RigidBody::static_body());

        let e2 = w.spawn();
        w.insert(e2, Transform::at(Vec3::new(5.0, 0.0, 0.0)));
        w.insert(e2, Collider::Capsule { radius: 0.5, height: 0.0 });
        w.insert(e2, RigidBody::static_body());

        let e3 = w.spawn();
        w.insert(e3, Transform::at(Vec3::new(10.0, 0.0, 0.0)));
        w.insert(e3, Collider::Aabb { half_extents: Vec3::new(-1.0, 1.0, 1.0) });
        w.insert(e3, RigidBody::static_body());

        let e4 = w.spawn();
        w.insert(e4, Transform::at(Vec3::new(15.0, 0.0, 0.0)));
        w.insert(e4, Collider::sphere(0.5));
        w.insert(e4, RigidBody::static_body());

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1, "only the valid sphere should remain");
        assert_eq!(cache.entries()[0].entity, e4);
    }

    #[test]
    fn build_computes_world_aabb() {
        let mut w = World::new();
        spawn_box(
            &mut w,
            Vec3::new(5.0, 1.0, -3.0),
            Vec3::new(1.0, 2.0, 0.5),
            false,
        );
        let mut cache = ColliderCache::new();
        cache.build(&w);
        let e = &cache.entries()[0];
        assert!((e.aabb_min - Vec3::new(4.0, -1.0, -3.5)).length() < 1e-5);
        assert!((e.aabb_max - Vec3::new(6.0, 3.0, -2.5)).length() < 1e-5);
    }

    #[test]
    fn capsule_hits_static_box() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::new(0.0, 0.5, 0.0), Vec3::splat(0.5), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(cache.capsule_hits(
            Vec3::new(0.0, 0.5, -2.0),
            Vec3::new(0.0, 0.5, 2.0),
            0.1,
            false,
        ));
    }

    #[test]
    fn capsule_misses_far_box() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::new(100.0, 0.0, 0.0), Vec3::splat(0.5), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y * 2.0, 0.35, false));
    }

    #[test]
    fn capsule_near_corner_no_hit() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(1.0), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        let p = Vec3::new(1.9, 1.9, 0.0);
        assert!(!cache.capsule_hits(p, p, 1.0, false));
        assert!(cache.capsule_hits(p, p, 1.5, false));
    }

    #[test]
    fn dynamic_ignored_when_include_dynamic_false() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(1.0), true);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y, 0.5, false));
        assert!(cache.capsule_hits(Vec3::ZERO, Vec3::Y, 0.5, true));
    }

    #[test]
    fn thin_box_between_samples_detected() {
        let mut w = World::new();
        spawn_box(
            &mut w,
            Vec3::new(0.375, 0.0, 0.0),
            Vec3::splat(0.01),
            false,
        );
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(cache.capsule_hits(
            Vec3::new(-3.0, 0.0, 0.0),
            Vec3::new(3.0, 0.0, 0.0),
            0.05,
            false,
        ));
    }

    #[test]
    fn short_segment_inside_aabb() {
        assert!(capsule_may_overlap_aabb(
            Vec3::new(-0.001, 0.0, 0.0),
            Vec3::new(0.001, 0.0, 0.0),
            0.0,
            Vec3::splat(-1.0),
            Vec3::splat(1.0),
        ));
    }

    /// C1.2 после ревью #2: контрпример ревьюера — отрезок пересекает
    /// маленький AABB, оба конца снаружи. Старый код возвращал `false`.
    #[test]
    fn short_segment_crosses_aabb_with_endpoints_outside() {
        let a = Vec3::new(-4e-7, 0.0, 0.0);
        let b = Vec3::new(4e-7, 0.0, 0.0);
        let min = Vec3::new(-1e-7, -1.0, -1.0);
        let max = Vec3::new(1e-7, 1.0, 1.0);
        assert!(capsule_may_overlap_aabb(a, b, 0.0, min, max));
    }

    #[test]
    fn capsule_hits_aabb_basic() {
        let amin = Vec3::splat(-0.5);
        let amax = Vec3::splat(0.5);
        assert!(capsule_hits_aabb(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            0.1,
            amin,
            amax,
        ));
        assert!(!capsule_hits_aabb(
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 0.0, -5.0),
            0.1,
            amin,
            amax,
        ));
    }

    #[test]
    fn capsule_hits_aabb_touching_zero_radius() {
        let amin = Vec3::splat(-0.5);
        let amax = Vec3::splat(0.5);
        assert!(capsule_hits_aabb(
            Vec3::new(-1.0, 0.5, 0.0),
            Vec3::new(1.0, 0.5, 0.0),
            0.0,
            amin,
            amax,
        ));
    }

    #[test]
    fn nan_input_returns_false() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(1.0), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(!cache.capsule_hits(
            Vec3::new(f32::NAN, 0.0, 0.0),
            Vec3::Y,
            0.5,
            false
        ));
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y, f32::NAN, false));
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y, -1.0, false));
        assert!(!cache.capsule_hits(
            Vec3::new(f32::INFINITY, 0.0, 0.0),
            Vec3::Y,
            0.5,
            false
        ));
    }

    #[test]
    fn query_aabb_returns_nearby_entries() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(0.5), false);
        spawn_box(&mut w, Vec3::new(2.0, 0.0, 0.0), Vec3::splat(0.5), false);
        spawn_box(&mut w, Vec3::new(100.0, 0.0, 0.0), Vec3::splat(0.5), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        let mut out = Vec::new();
        cache.query_aabb(
            Vec3::new(-1.0, -1.0, -1.0),
            Vec3::new(3.0, 1.0, 1.0),
            &mut out,
        );
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn rebuild_clears_previous_state() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(0.5), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);
        let e = w.entities()[0];
        w.despawn(e);
        cache.build(&w);
        assert!(cache.is_empty());
    }

    #[test]
    fn large_floor_detected_at_edge() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::new(10.0, 0.25, 10.0), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        let feet_a = Vec3::new(9.0, 0.35, 0.0);
        let feet_b = Vec3::new(9.0, 1.45, 0.0);
        assert!(cache.capsule_hits(feet_a, feet_b, 0.35, false));
        let far_a = Vec3::new(15.0, 0.35, 0.0);
        let far_b = Vec3::new(15.0, 1.45, 0.0);
        assert!(!cache.capsule_hits(far_a, far_b, 0.35, false));
    }

    #[test]
    fn capsule_hits_except_skips_entity() {
        let mut w = World::new();
        let e = spawn_box(&mut w, Vec3::ZERO, Vec3::splat(1.0), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        let a = Vec3::ZERO;
        let b = Vec3::Y * 2.0;
        assert!(cache.capsule_hits(a, b, 0.5, false));
        assert!(!cache.capsule_hits_except(a, b, 0.5, false, Some(e)));
    }

    #[test]
    fn capsule_hits_near_tilted_triangle_face() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Transform::at(Vec3::ZERO));
        let verts = vec![
            Vec3::new(-2.0, 0.0, -2.0),
            Vec3::new(2.0, 0.0, -2.0),
            Vec3::new(0.0, 1.0, 2.0),
        ];
        w.insert(e, MeshCollider::from_mesh(verts, vec![[0, 1, 2]]));
        let mut cache = ColliderCache::new();
        cache.build(&w);
        let p = Vec3::new(0.0, 0.5, 0.0);
        assert!(cache.capsule_hits(p, p, 0.1, false));
        let far = Vec3::new(0.0, 5.0, 2.0);
        assert!(!cache.capsule_hits(far, far, 0.1, false));
    }

    /// C1.2 после ревью #2: **реальный поворот**, проверка точки на
    /// грани (не на вершине).
    #[test]
    fn capsule_hits_mesh_with_rotation_and_scale() {
        let mut w = World::new();
        let e = w.spawn();
        // Поворот на 90° вокруг Y: локальная вершина (1, 0, 0) → world (0, 0, -1).
        let rot = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let mut t = Transform::at(Vec3::new(5.0, 0.0, 0.0));
        t.rotation = rot;
        t.scale = Vec3::new(2.0, 1.0, 1.0);
        w.insert(e, t);
        // Треугольник в XY-плоскости локально.
        let verts = vec![
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        ];
        w.insert(e, MeshCollider::from_mesh(verts, vec![[0, 1, 2]]));
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);

        // Точка на середине гипотенузы. Локально (0.5, 0.5, 0) →
        // world: (5,0,0) + R * S * (0.5, 0.5, 0).
        // R вокруг Y на 90°: (x, y, z) → (z, y, -x). S = (2, 1, 1).
        // S * (0.5, 0.5, 0) = (1.0, 0.5, 0).
        // R * (1.0, 0.5, 0) = (0, 0.5, -1.0).
        // world = (5.0, 0.5, -1.0).
        let p = Vec3::new(5.0, 0.5, -1.0);
        assert!(cache.capsule_hits(p, p, 0.1, false), "point on face");
        // Далеко — miss.
        let far = Vec3::new(5.0, 5.0, 0.0);
        assert!(!cache.capsule_hits(far, far, 0.1, false));
    }

    /// C1.2 после ревью #2: маленький валидный треугольник (не должен
    /// считаться вырожденным). Сторона 1e-4 м, прямой угол.
    #[test]
    fn small_valid_triangle_not_degenerate() {
        let a = Vec3::ZERO;
        let b = Vec3::new(1e-4, 0.0, 0.0);
        let c = Vec3::new(0.0, 1e-4, 0.0);
        // Точка над центром треугольника на расстоянии 5e-5.
        let p = Vec3::new(2.5e-5, 2.5e-5, 5e-5);
        let d2 = point_triangle_dist_sq(p, a, b, c);
        let expected = 25e-10;
        assert!(
            (d2 - expected).abs() < 1e-10,
            "expected d2 ~ {:.3e}, got {:.3e}",
            expected,
            d2
        );
    }
}