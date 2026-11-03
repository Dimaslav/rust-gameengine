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
            if let Some(v) = world.get::<Visible>(e)
                && !v.0 { continue; }
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

// ============================================================
// Тесты
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::components::{Name, Transform};
    use crate::physics::RigidBody;

    fn spawn_box(world: &mut World, pos: Vec3, half: Vec3, dynamic: bool) -> Entity {
        let e = world.spawn();
        world.insert(e, Transform::at(pos));
        world.insert(e, Collider::aabb(half));
        world.insert(
            e,
            if dynamic { RigidBody::dynamic(1.0) } else { RigidBody::static_body() },
        );
        e
    }

    fn spawn_hidden_box(world: &mut World, pos: Vec3, half: Vec3) -> Entity {
        let e = spawn_box(world, pos, half, false);
        world.insert(e, Visible(false));
        e
    }

    #[test]
    fn empty_world_builds_empty_cache() {
        let mut w = World::new();
        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(cache.is_empty());
        assert_eq!(cache.len(), 0);
        // capsule_hits не должен паниковать на пустом кэше.
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y, 0.5, false));
    }

    #[test]
    fn build_ignores_entities_without_collider() {
        let mut w = World::new();
        let e = w.spawn();
        w.insert(e, Transform::at(Vec3::ZERO));
        // нет Collider.

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert!(cache.is_empty());
    }

    #[test]
    fn build_ignores_hidden_entities() {
        let mut w = World::new();
        spawn_hidden_box(&mut w, Vec3::ZERO, Vec3::splat(0.5));
        spawn_box(&mut w, Vec3::new(10.0, 0.0, 0.0), Vec3::splat(0.5), false);

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.entries()[0].world_pos, Vec3::new(10.0, 0.0, 0.0));
    }

    #[test]
    fn build_computes_world_aabb() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::new(5.0, 1.0, -3.0), Vec3::new(1.0, 2.0, 0.5), false);

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

        // Капсула проходит через центр бокса.
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

        assert!(!cache.capsule_hits(
            Vec3::ZERO,
            Vec3::Y * 2.0,
            0.35,
            false,
        ));
    }

    #[test]
    fn dynamic_ignored_when_include_dynamic_false() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(1.0), true);

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);

        // include_dynamic = false → игнорируем.
        assert!(!cache.capsule_hits(Vec3::ZERO, Vec3::Y, 0.5, false));
        // include_dynamic = true → ловим.
        assert!(cache.capsule_hits(Vec3::ZERO, Vec3::Y, 0.5, true));
    }

    #[test]
    fn capsule_hits_aabb_basic() {
        let amin = Vec3::splat(-0.5);
        let amax = Vec3::splat(0.5);
        // Капсула пересекает центр.
        assert!(capsule_hits_aabb(
            Vec3::new(0.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 1.0),
            0.1,
            amin,
            amax,
        ));
        // Далеко — не пересекает.
        assert!(!capsule_hits_aabb(
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 0.0, -5.0),
            0.1,
            amin,
            amax,
        ));
    }

    #[test]
    fn query_aabb_returns_nearby_entries() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::new(0.0, 0.0, 0.0), Vec3::splat(0.5), false);
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
        // Должны найтись два ближних, дальний (100,0,0) — вне AABB.
        assert_eq!(out.len(), 2);
    }

    #[test]
    fn capsule_samples_cover_whole_segment() {
        // Тонкий длинный бокс: капсула должна его задеть в середине,
        // даже если концы далеко.
        let mut w = World::new();
        spawn_box(
            &mut w,
            Vec3::new(0.0, 0.5, 0.0),
            Vec3::new(0.1, 0.05, 5.0),
            false,
        );

        let mut cache = ColliderCache::new();
        cache.build(&w);

        assert!(cache.capsule_hits(
            Vec3::new(0.0, 0.5, -3.0),
            Vec3::new(0.0, 0.5, 3.0),
            0.1,
            false,
        ));
    }

    #[test]
    fn rebuild_clears_previous_state() {
        let mut w = World::new();
        spawn_box(&mut w, Vec3::ZERO, Vec3::splat(0.5), false);

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);

        // Удаляем и перестраиваем — кэш должен очиститься.
        let e = w.entities()[0];
        w.despawn(e);
        cache.build(&w);
        assert!(cache.is_empty());
    }

    #[test]
    fn name_component_does_not_affect_cache() {
        let mut w = World::new();
        let e = spawn_box(&mut w, Vec3::ZERO, Vec3::splat(0.5), false);
        w.insert(e, Name("test".into()));

        let mut cache = ColliderCache::new();
        cache.build(&w);
        assert_eq!(cache.len(), 1);
    }
}