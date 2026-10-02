//! Физика: rigid bodies, коллайдеры, разрешение коллизий.
//!
//! Упрощения v1 (сознательные, для расширяемости):
//! - Только линейная динамика (без вращения тел).
//! - Broad-phase O(n²) — заменим на uniform grid / SAP при росте сцены.
//! - Colliders: Sphere / Aabb / Capsule (капсула как AABB).
//! - Никаких constraints (fixed/hinge/spring) — только столкновения.
//!
//! Точки расширения:
//! - `PhysicsWorld::step` — сердце; можно разрезать на подсистемы.
//! - `broad_phase` — заменить на grid без изменения интерфейса.
//! - `collide` — заменить на GJK/EPA для convex hulls.
//! - `BodyState` — добавить `rotation: Quat`, `inertia`, `angular_velocity`.
//! - `Contact` — добавить `point`, `r_a`, `r_b` для rotation resolve.

pub mod components;
pub mod world;

pub use components::{BodyType, Collider, PhysicsMaterial, RigidBody};
pub use world::PhysicsWorld;