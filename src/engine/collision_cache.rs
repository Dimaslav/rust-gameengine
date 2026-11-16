use std::sync::Arc;
use glam::{Mat4, Quat, Vec3};
use crate::ecs::{Entity, World};
use crate::game::components::{Transform, Visible};
use crate::physics::{BodyType, Collider, MeshCollider, RigidBody};
use super::spatial_hash::SpatialHash2D;

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

            let body_type = world
                .get::<RigidBody>(e)
                .map(|rb| rb.body_type)
                .unwrap_or(BodyType::Static);

            let model = crate::game::world_matrix(world, e);
            let (s, r, p) = model.to_scale_rotation_translation();

            // C1.2: negative scale (reflection) — отклоняем.
            if s.x < 0.0 || s.y < 0.0 || s.z < 0.0 {
                log::error!(
                    "ColliderCache: entity #{} has negative scale ({:?}). \
                     Skipped — reflection не поддерживается в C1.2.",
                    e,
                    s
                );
                continue;
            }

            // C1.2: детект shear — non-uniform scale родителя + поворот
            // дочернего даёт shear, который to_scale_rotation_translation
            // теряет. Сравниваем model с восстановленной T·R·S.
            let reconstructed = Mat4::from_scale_rotation_translation(s, r, p);
            let shear_diff = (model - reconstructed)
                .to_cols_array()
                .iter()
                .map(|x| x.abs())
                .fold(0.0f32, f32::max);
            if shear_diff > 1e-3 {
                log::warn!(
                    "ColliderCache: entity #{} has shear in transform \
                     (diff={:.4}). Коллизия будет приближённой.",
                    e,
                    shear_diff
                );
            }

            let world_scale = s.abs();

            // Отдельные AABB для primitive и mesh (C1.2 п.4.4 — не пересчитывать
            // mesh.world_aabb на каждом запросе).
            let primitive_aabb = primitive
                .as_ref()
                .map(|c| c.world_aabb(p, r, world_scale));
            let mesh_aabb = mesh
                .as_ref()
                .map(|m| m.world_aabb(p, r, world_scale));

            let (aabb_min, aabb_max) = match (primitive_aabb, mesh_aabb) {
                (Some((a1, b1)), Some((a2, b2))) => (a1.min(a2), b1.max(b2)),
                (Some((a, b)), None) => (a, b),
                (None, Some((a, b))) => (a, b),
                (None, None) => unreachable!(),
            };

            // C1.2: u32::try_from вместо as (переполнение → panic с сообщением).
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

    /// Проверка капсулы (segment [a, b], radius r) на пересечение.
    /// `include_dynamic == false` → Dynamic-тела игнорируются (Kinematic
    /// и Static — проверяются). NaN/Inf → `false` (C1.2).
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
        // C1.2: NaN/Inf проверки. Публичный API — «нет пересечения» при
        // некорректном вводе. Причины логируются на уровне вызывающего.
        if !a.is_finite()
            || !b.is_finite()
            || !r.is_finite()
            || r < 0.0
        {
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

            // Broad-phase: консервативный segment-AABB.
            if !capsule_may_overlap_aabb(a, b, r, e.aabb_min, e.aabb_max) {
                continue;
            }

            if let Some(col) = &e.collider
                && capsule_hits_primitive(a, b, r, col, e)
            {
                return true;
            }

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
// Broad-phase: capsule-AABB (консервативный)
// ============================================================

/// **Broad phase**: капсула [a, b] радиуса r vs AABB. Консервативен:
/// возможны false positives у углов расширенного AABB (Minkowski-sum
/// AABB + sphere — это скруглённый бокс, но его AABB больше).
///
/// `r == 0` тоже работает. **Короткие отрезки**: если `a ≈ b`, проверяем
/// **оба** конца (C1.2 п.1.7).
#[inline]
pub fn capsule_may_overlap_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
    if r < 0.0 {
        return false;
    }
    let emin = amin - Vec3::splat(r);
    let emax = amax + Vec3::splat(r);

    let d = b - a;

    // C1.2 п.1.7: короткий отрезок — проверяем ОБА конца.
    if d.length_squared() < 1e-12 {
        return point_in_aabb(a, emin, emax) || point_in_aabb(b, emin, emax);
    }

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

#[inline]
fn point_in_aabb(p: Vec3, amin: Vec3, amax: Vec3) -> bool {
    p.x >= amin.x
        && p.x <= amax.x
        && p.y >= amin.y
        && p.y <= amax.y
        && p.z >= amin.z
        && p.z <= amax.z
}

/// Совместимость: старое имя. Помечено как alias — используйте
/// `capsule_may_overlap_aabb` для ясности.
#[inline]
pub fn capsule_hits_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
    capsule_may_overlap_aabb(a, b, r, amin, amax)
}

// ============================================================
// Narrow-phase 1: primitive (точные формулы)
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
            // Non-uniform scale сферы — консервативная аппроксимация
            // по max(scale). Документировано в Collider::world_aabb.
            let world_r = radius * scale.max_element();
            let rr = r + world_r;
            segment_point_dist_sq(a, b, e.world_pos) <= rr * rr
        }
        Collider::Aabb { half_extents } => {
            // C1.2 п.1.1: точный narrow phase через segment-AABB distance.
            // Segment и полуразмеры — в локали, r — в world (сферу
            // масштабировать не нужно, т.к. расстояние в world).
            let local_a = inv_rot * (a - e.world_pos);
            let local_b = inv_rot * (b - e.world_pos);
            let half = half_extents * scale;
            // r остаётся в world (расстояние в мире = расстояние в
            // касательном пространстве OBB, т.к. поворот — изометрия).
            // Масштаб полуразмеров даёт OBB в world.
            segment_aabb_dist_sq(local_a, local_b, -half, half) <= r * r
        }
        Collider::Capsule { radius, height } => {
            // Non-uniform scale капсулы — по max(scale) для радиуса,
            // по scale.y для высоты. Консервативно.
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
// Narrow-phase 2: mesh через BVH + точная проверка в world
// ============================================================

fn capsule_hits_mesh(
    a: Vec3,
    b: Vec3,
    r: f32,
    mesh: &MeshCollider,
    e: &ColliderEntry,
) -> bool {
    // Broad (world): sphere-AABB капсулы vs AABB mesh (уже посчитан в build).
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

    // BVH в local. local_r = r / min(scale) — консервативно.
    let scale_inv = safe_inv(e.world_scale);
    let inv_rot = e.world_rot.inverse();
    let local_a = (inv_rot * (a - e.world_pos)) * scale_inv;
    let local_b = (inv_rot * (b - e.world_pos)) * scale_inv;
    let local_r = r / e.world_scale.min_element().max(1e-6);

    let triangles = mesh.triangles();
    if triangles.is_empty() {
        return false;
    }
    let bvh = mesh.bvh();

    let cmin = local_a.min(local_b) - Vec3::splat(local_r);
    let cmax = local_a.max(local_b) + Vec3::splat(local_r);
    let r2 = r * r;
    let mut hit = false;

    // C1.2 п.4.3: for_each_in_aabb с early-break (bool return).
    bvh.for_each_in_aabb(cmin, cmax, |tri_idx| {
        let Some(tri) = triangles.get(tri_idx as usize) else {
            // C1.2 п.5.6: debug_assert, не молчаливый return.
            debug_assert!(false, "BVH returned invalid tri index {}", tri_idx);
            return true;
        };
        // C1.2 п.1.2: transform в world, точное расстояние в world.
        let wa = e.model.transform_point3(tri[0]);
        let wb = e.model.transform_point3(tri[1]);
        let wc = e.model.transform_point3(tri[2]);
        if segment_triangle_dist_sq(a, b, wa, wb, wc) <= r2 {
            hit = true;
            return false; // early break
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
    Vec3::new(
        if v.x.abs() > 1e-6 { 1.0 / v.x } else { 1.0 },
        if v.y.abs() > 1e-6 { 1.0 / v.y } else { 1.0 },
        if v.z.abs() > 1e-6 { 1.0 / v.z } else { 1.0 },
    )
}

/// Точно: минимальное squared-расстояние от точки до segment.
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

/// Точно: минимальное squared-расстояние между двумя segments.
/// Reference: Ericson, RTCD, 5.1.9.
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

    // 12 треугольников (2 на грань).
    let tris = [
        // Bottom (y = y0)
        (0, 1, 5), (0, 5, 4),
        // Top (y = y1)
        (2, 6, 7), (2, 7, 3),
        // Front (z = z0)
        (0, 2, 3), (0, 3, 1),
        // Back (z = z1)
        (4, 5, 7), (4, 7, 6),
        // Left (x = x0)
        (0, 4, 6), (0, 6, 2),
        // Right (x = x1)
        (1, 3, 7), (1, 7, 5),
    ];

    let mut best = f32::INFINITY;
    for (i, j, k) in tris {
        let d = segment_triangle_dist_sq(a, b, corners[i], corners[j], corners[k]);
        if d < best {
            best = d;
            if best < 1e-12 {
                return 0.0;
            }
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

/// Расстояние segment ↔ triangle. Через segment-triangle intersection
/// + минимум по вершинам/рёбрам.
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

/// Segment-Triangle intersection (Möller–Trumbore).
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

/// Минимальное squared-расстояние от точки до треугольника.
/// Reference: Ericson, RTCD, 5.1.5. Вырожденные — min по рёбрам.
fn point_triangle_dist_sq(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> f32 {
    // C1.2 п.2.3: вырожденность проверяем В НАЧАЛЕ, до делений.
    let ab = b - a;
    let ac = c - a;
    let n = ab.cross(ac);
    let n2 = n.length_squared();
    if n2 < 1e-12 {
        // Вырожденный треугольник — минимум по рёбрам.
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

    // Внутри треугольника — до плоскости.
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
        assert_eq!(cache.len(), 0, "negative scale must be rejected");
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

    /// C1.2 п.3.2: регрессионный тест — угол OBB. Точечная капсула
    /// в (1.9, 1.9, 0), r=1. Бокс [-1,1]³. Broad-phase даст true,
    /// но точный narrow-phase должен дать false: расстояние до
    /// угла (1, 1, 0) = √(0.9² + 0.9²) ≈ 1.27 > 1.
    #[test]
    fn capsule_near_corner_no_hit() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(1.0), false);
        let mut cache = ColliderCache::new();
        cache.build(&w);
        // Точечная капсула в (1.9, 1.9, 0), r = 1.
        let p = Vec3::new(1.9, 1.9, 0.0);
        let hit = cache.capsule_hits(p, p, 1.0, false);
        assert!(
            !hit,
            "capsule at (1.9,1.9,0) r=1 vs box [-1,1]³: distance ≈ 1.27 > 1 → must miss"
        );
        // При r = 1.5 — попадает.
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

    /// C1.2 п.3.1: РАНЬШЕ тест ставил бокс в 0.0 и проходил по случайности.
    /// Теперь — 0.375, между сэмплами старого broad-phase (0.0 и 0.75).
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

    /// C1.2 п.1.7: очень короткий segment, пересекающий AABB.
    #[test]
    fn short_segment_inside_aabb() {
        // Segment [-0.001, 0.001] внутри AABB [-1, 1]³.
        assert!(capsule_may_overlap_aabb(
            Vec3::new(-0.001, 0.0, 0.0),
            Vec3::new(0.001, 0.0, 0.0),
            0.0,
            Vec3::splat(-1.0),
            Vec3::splat(1.0),
        ));
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

    /// C1.2 п.2.4: NaN/Inf → false.
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

    /// C1.2 п.3.3: mesh narrow-phase — точное расстояние до наклонной
    /// грани, не только вершины.
    #[test]
    fn capsule_hits_near_tilted_triangle_face() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Transform::at(Vec3::ZERO));
        // Большой треугольник, наклонённый.
        let verts = vec![
            Vec3::new(-2.0, 0.0, -2.0),
            Vec3::new(2.0, 0.0, -2.0),
            Vec3::new(0.0, 1.0, 2.0),
        ];
        w.insert(e, MeshCollider::from_mesh(verts, vec![[0, 1, 2]]));
        let mut cache = ColliderCache::new();
        cache.build(&w);
        // Точка в центре грани (0, 0.5, 0) — точно на плоскости треугольника.
        let p = Vec3::new(0.0, 0.5, 0.0);
        assert!(cache.capsule_hits(p, p, 0.1, false), "point on face must hit");
        // Точка далеко — miss.
        let far = Vec3::new(0.0, 5.0, 2.0);
        assert!(!cache.capsule_hits(far, far, 0.1, false));
    }

    /// C1.2 п.3.3: mesh с поворотом и non-uniform scale.
    #[test]
    fn capsule_hits_mesh_with_transform() {
        let mut w = World::new();
        let e = w.spawn();
        let mut t = Transform::at(Vec3::new(5.0, 0.0, 0.0));
        t.scale = Vec3::new(2.0, 1.0, 1.0); // non-uniform
        w.insert(e, t);
        let verts = vec![
            Vec3::new(-1.0, 0.0, -1.0),
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(0.0, 1.0, 1.0),
        ];
        w.insert(e, MeshCollider::from_mesh(verts, vec![[0, 1, 2]]));
        let mut cache = ColliderCache::new();
        cache.build(&w);
        // Точка около вершины в мире (5 + 0 * 2, 1, 0 + 1) = (5, 1, 1).
        let near = Vec3::new(5.0, 1.0, 1.0);
        assert!(cache.capsule_hits(near, near, 0.2, false));
    }
}