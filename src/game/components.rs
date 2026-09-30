use glam::{Mat4, Quat, Vec3};

#[derive(Debug, Clone, Copy)]
pub struct Transform {
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Transform {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            position: Vec3::new(x, y, z),
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    pub fn at(position: Vec3) -> Self {
        Self {
            position,
            rotation: Quat::IDENTITY,
            scale: Vec3::ONE,
        }
    }

    pub fn with_scale(mut self, s: f32) -> Self {
        self.scale = Vec3::splat(s);
        self
    }

    pub fn with_scale_xyz(mut self, x: f32, y: f32, z: f32) -> Self {
        self.scale = Vec3::new(x, y, z);
        self
    }

    pub fn with_rotation(mut self, q: Quat) -> Self {
        self.rotation = q;
        self
    }

    pub fn matrix(&self) -> Mat4 {
        Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}

/// Ссылка на родительскую сущность. Если компонента нет — сущность
/// находится в корне иерархии.
#[derive(Debug, Clone, Copy)]
pub struct Parent(pub u32);

#[derive(Debug, Clone)]
pub struct Name(pub String);

impl Name {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone)]
pub struct MeshHandle(pub String);

#[derive(Debug, Clone)]
pub struct MaterialHandle(pub String);

#[derive(Debug, Clone)]
pub struct SkeletonHandle(pub String);

#[derive(Debug, Clone)]
pub struct AnimationPlayer {
    pub clip: String,
    pub time: f32,
    pub speed: f32,
    pub looping: bool,
}

impl AnimationPlayer {
    pub fn new(clip: impl Into<String>) -> Self {
        Self {
            clip: clip.into(),
            time: 0.0,
            speed: 1.0,
            looping: true,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Spinner {
    pub axis: Vec3,
    pub speed: f32,
}

impl Spinner {
    pub fn new(axis: Vec3, speed: f32) -> Self {
        Self { axis, speed }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Velocity {
    pub value: Vec3,
}

impl Velocity {
    pub fn new(x: f32, y: f32, z: f32) -> Self {
        Self {
            value: Vec3::new(x, y, z),
        }
    }
}