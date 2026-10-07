//! Компоненты физики: RigidBody, Collider, PhysicsMaterial.

use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BodyType {
    Static,
    Dynamic,
    Kinematic,
}

impl Default for BodyType {
    fn default() -> Self { Self::Dynamic }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RigidBody {
    pub body_type: BodyType,
    pub mass: f32,
    pub velocity: Vec3,
    #[serde(default)]
    pub angular_velocity: Vec3,
    pub linear_damping: f32,
    #[serde(default)]
    pub angular_damping: f32,
    pub gravity_scale: f32,
    #[serde(skip)]
    pub force: Vec3,
    #[serde(skip)]
    pub torque: Vec3,
    #[serde(skip)]
    pub sleeping: bool,
    #[serde(skip)]
    pub sleep_timer: f32,
}

impl RigidBody {
    pub fn dynamic(mass: f32) -> Self {
        Self {
            body_type: BodyType::Dynamic,
            mass: mass.max(1e-4),
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            linear_damping: 0.01,
            angular_damping: 0.05,
            gravity_scale: 1.0,
            force: Vec3::ZERO,
            torque: Vec3::ZERO,
            sleeping: false,
            sleep_timer: 0.0,
        }
    }

    pub fn static_body() -> Self {
        Self {
            body_type: BodyType::Static,
            mass: 0.0,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 0.0,
            force: Vec3::ZERO,
            torque: Vec3::ZERO,
            sleeping: false,
            sleep_timer: 0.0,
        }
    }

    pub fn kinematic() -> Self {
        Self {
            body_type: BodyType::Kinematic,
            mass: 0.0,
            velocity: Vec3::ZERO,
            angular_velocity: Vec3::ZERO,
            linear_damping: 0.0,
            angular_damping: 0.0,
            gravity_scale: 0.0,
            force: Vec3::ZERO,
            torque: Vec3::ZERO,
            sleeping: false,
            sleep_timer: 0.0,
        }
    }

    pub fn inv_mass(&self) -> f32 {
        match self.body_type {
            BodyType::Dynamic if self.mass > 1e-8 => 1.0 / self.mass,
            _ => 0.0,
        }
    }

    pub fn is_dynamic(&self) -> bool { self.body_type == BodyType::Dynamic }
    pub fn is_static(&self) -> bool { self.body_type == BodyType::Static }

    pub fn apply_force(&mut self, f: Vec3) {
        if self.is_dynamic() { self.force += f; }
    }

    pub fn apply_impulse(&mut self, j: Vec3) {
        if self.is_dynamic() && self.mass > 1e-8 {
            self.velocity += j / self.mass;
            self.wake();
        }
    }

    pub fn wake(&mut self) {
        self.sleeping = false;
        self.sleep_timer = 0.0;
    }
}

impl Default for RigidBody {
    fn default() -> Self { Self::dynamic(1.0) }
}

/// Коллайдер. Центр совпадает с `Transform.position`.
///
/// `Capsule::height` — **полная высота, включая обе полусферы-крышки**.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Collider {
    Sphere { radius: f32 },
    Aabb { half_extents: Vec3 },
    Capsule { radius: f32, height: f32 },
}

impl Collider {
    pub fn sphere(radius: f32) -> Self {
        Self::Sphere { radius: radius.max(1e-4) }
    }

    pub fn aabb(half_extents: Vec3) -> Self {
        Self::Aabb { half_extents: half_extents.max(Vec3::splat(1e-4)) }
    }

    pub fn capsule(radius: f32, height: f32) -> Self {
        let r = radius.max(1e-4);
        Self::Capsule {
            radius: r,
            height: height.max(2.0 * r + 1e-4),
        }
    }

    /// Мировой AABB этого коллайдера с учётом поворота.
    ///
    /// Для AABB-коллайдера вычисляется **обёртывающий** AABB повёрнутого
    /// OBB: каждая из 3 локальных полуосей умножается на `rotation`, и
    /// берётся сумма абсолютных значений компонент. Для поворотов,
    /// кратных 90°, это **точный** AABB. Для произвольных — слегка
    /// завышенный (максимум в √3 раз), что безопасно для collision
    /// (только false positives на очень острых углах).
    ///
    /// Для Capsule мировые концы сегмента (±Y · height/2) поворачиваются,
    /// берётся их AABB, и к результату добавляется радиус.
    ///
    /// **ИЗМЕНЕНО:** раньше в `engine/collision.rs` и `physics/*` поворот
    /// полностью игнорировался — коллайдеры стен, повёрнутых на 90°,
    /// оказывались перпендикулярны визуальному мешу. Это был главный
    /// источник «плохих коллизий».
    pub fn world_aabb(&self, pos: Vec3, rotation: Quat, scale: Vec3) -> (Vec3, Vec3) {
        let scale = scale.abs();
        match self {
            Collider::Sphere { radius } => {
                let r = radius * scale.max_element();
                (pos - Vec3::splat(r), pos + Vec3::splat(r))
            }
            Collider::Aabb { half_extents } => {
                let h = *half_extents * scale;
                let ax = rotation * Vec3::new(h.x, 0.0, 0.0);
                let ay = rotation * Vec3::new(0.0, h.y, 0.0);
                let az = rotation * Vec3::new(0.0, 0.0, h.z);
                let wh = Vec3::new(
                    ax.x.abs() + ay.x.abs() + az.x.abs(),
                    ax.y.abs() + ay.y.abs() + az.y.abs(),
                    ax.z.abs() + ay.z.abs() + az.z.abs(),
                );
                (pos - wh, pos + wh)
            }
            Collider::Capsule { radius, height } => {
                let r = radius * scale.max_element();
                let hh = height * scale.y * 0.5;
                let up = rotation * Vec3::new(0.0, hh, 0.0);
                let a = pos + up;
                let b = pos - up;
                let mn = a.min(b) - Vec3::splat(r);
                let mx = a.max(b) + Vec3::splat(r);
                (mn, mx)
            }
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct PhysicsMaterial {
    pub restitution: f32,
    pub friction: f32,
}

impl PhysicsMaterial {
    pub fn new(restitution: f32, friction: f32) -> Self {
        Self {
            restitution: restitution.clamp(0.0, 1.0),
            friction: friction.max(0.0),
        }
    }

    pub fn rubber() -> Self { Self::new(0.7, 0.6) }
    pub fn metal() -> Self { Self::new(0.3, 0.4) }
    pub fn wood() -> Self { Self::new(0.2, 0.7) }
    pub fn ice() -> Self { Self::new(0.05, 0.05) }
    pub fn concrete() -> Self { Self::new(0.1, 0.9) }
}

impl Default for PhysicsMaterial {
    fn default() -> Self { Self::wood() }
}