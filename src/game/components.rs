use glam::{Quat, Vec3};

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

    pub fn matrix(&self) -> glam::Mat4 {
        glam::Mat4::from_scale_rotation_translation(self.scale, self.rotation, self.position)
    }
}

/// Ссылка на меш в реестре Renderer.
#[derive(Debug, Clone)]
pub struct MeshHandle(pub String);

/// Ссылка на материал в реестре Renderer.
#[derive(Debug, Clone)]
pub struct MaterialHandle(pub String);

/// Вращение вокруг оси.
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
        Self { value: Vec3::new(x, y, z) }
    }
}