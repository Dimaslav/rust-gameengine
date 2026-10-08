//! Кэш коллайдеров для быстрых collision queries.
//!
//! Строится раз в кадр (или реже), хранит pre-computed world AABB +
//! world pos/rot/scale каждого коллайдера. Используется в
//! `collision::resolve_movement_ex` и `find_support` вместо прямого
//! обхода `World`.
//!
//! # Зачем
//!
//! Раньше `capsule_hits` вызывался ~4 раза за кадр (по X, Z, Y,
//! step-down), и каждый раз обходил **все** entity в World: `Visible`
//! check → `Collider` get → `world_matrix` (обход Parent-цепочки) →
//! `world_aabb` → `capsule_hits_aabb`.
//!
//! При 500 entity это 2000 полных обходов в кадр. При этом
//! 99% коллайдеров далеко и не пересекаются с капсулой игрока.
//!
//! Теперь:
//!   1. Один проход по World → `ColliderCache::build`.
//!   2. Запросы к spatial hash → только кандидаты в AABB игрока.
//!   3. AABB-проверка только для них.

use glam::{Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{Transform, Visible};
use crate::physics::{BodyType, Collider, RigidBody};

use super::spatial_hash::SpatialHash2D;

// ============================================================
// Entry
// ============================================================

pub struct ColliderEntry {
    pub entity: Entity,
    pub body_type: BodyType,
    pub aabb_min: Vec3,
    pub aabb_max: Vec3,
    pub world_pos: Vec3,
    pub world_rot: Quat,
    pub world_scale: Vec3,
    pub collider: Collider,
}

// ============================================================
// Cache
// ============================================================

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

    /// Перестроить кэш из World. **O(n)** один раз за кадр.
    pub fn build(&mut self, world: &World) {
        self.entries.clear();
        self.hash.clear();

        for &e in world.entities() {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }
            let Some(col) = world.get::<Collider>(e).copied() else { continue };
            if world.get::<Transform>(e).is_none() { continue; }

            let body_type = world.get::<RigidBody>(e)
                .map(|rb| rb.body_type)
                .unwrap_or(BodyType::Static);

            let model = crate::game::world_matrix(world, e);
            let (s, r, p) = model.to_scale_rotation_translation();
            let world_scale = s.abs();

            let (aabb_min, aabb_max) = col.world_aabb(p, r, world_scale);

            let idx = self.entries.len() as u32;
            self.entries.push(ColliderEntry {
                entity: e,
                body_type,
                aabb_min,
                aabb_max,
                world_pos: p,
                world_rot: r,
                world_scale,
                collider: col,
            });

            let center = (aabb_min + aabb_max) * 0.5;
            self.hash.insert(idx, center);
        }
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }

    pub fn entries(&self) -> &[ColliderEntry] { &self.entries }

    /// Кандидаты в AABB `[min, max]` (XZ-проекция + грубая Y).
    /// Пишет индексы в `out` (очищая).
    pub fn query_aabb(&self, min: Vec3, max: Vec3, out: &mut Vec<u32>) {
        self.hash.query_aabb(min, max, out);
    }

    /// Проверка капсулы [a, b] с радиусом r.
    ///
    /// `include_dynamic == false` → dynamic-тела **игнорируются**
    /// (это режим «игрок проходит сквозь ящики по XZ»).
    ///
    /// `include_dynamic == true` → dynamic-тела проверяются
    /// (это режим «игрок стоит на ящике по Y»).
    pub fn capsule_hits(
        &self,
        a: Vec3,
        b: Vec3,
        r: f32,
        include_dynamic: bool,
    ) -> bool {
        // AABB запроса: box вокруг капсулы расширенный на r.
        let mn = a.min(b) - Vec3::splat(r);
        let mx = a.max(b) + Vec3::splat(r);

        let mut cand = Vec::with_capacity(16);
        self.hash.query_aabb(mn, mx, &mut cand);

        for &i in &cand {
            let e = &self.entries[i as usize];
            if !include_dynamic && e.body_type == BodyType::Dynamic {
                continue;
            }
            if capsule_hits_aabb(a, b, r, e.aabb_min, e.aabb_max) {
                return true;
            }
        }
        false
    }
}

impl Default for ColliderCache {
    fn default() -> Self { Self::new() }
}

// ============================================================
// Capsule vs AABB (перенесено из collision.rs)
// ============================================================

const CAPSULE_SAMPLES: usize = 8;

#[inline]
fn point_aabb_dist_sq(p: Vec3, min: Vec3, max: Vec3) -> f32 {
    let clamped = p.clamp(min, max);
    (p - clamped).length_squared()
}

/// Пересечение капсулы [a, b] радиуса r с AABB `[amin, amax]`.
pub fn capsule_hits_aabb(a: Vec3, b: Vec3, r: f32, amin: Vec3, amax: Vec3) -> bool {
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