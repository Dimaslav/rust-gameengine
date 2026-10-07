//! Физика: rigid bodies, коллайдеры, разрешение коллизий, запросы.

pub mod components;
pub mod navmesh;
pub mod queries;
pub mod world;

pub use components::{BodyType, Collider, PhysicsMaterial, RigidBody};
pub use navmesh::{BakeOpts, Navmesh};
pub use queries::{QueryFilter, RayHit};
pub use world::PhysicsWorld;