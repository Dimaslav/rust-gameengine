use glam::{Mat4, Vec3, Vec4};

/// Перспективная 3D-камера с orbit-управлением вокруг target.
pub struct Camera3D {
    pub target: Vec3,
    pub distance: f32,
    /// Радианы, вращение вокруг вертикальной оси.
    pub yaw: f32,
    /// Радианы, наклон. Ограничен [-1.5, 1.5] чтобы избежать gimbal-lock.
    pub pitch: f32,

    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl Camera3D {
    pub fn new(aspect: f32) -> Self {
        Self {
            target: Vec3::ZERO,
            distance: 8.0,
            yaw: 0.7,
            pitch: 0.5,
            fov_y: 60.0_f32.to_radians(),
            aspect,
            near: 0.1,
            far: 1000.0,
        }
    }

    pub fn set_viewport(&mut self, width: u32, height: u32) {
        if height > 0 {
            self.aspect = width as f32 / height as f32;
        }
    }

    pub fn position(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        self.target
            + Vec3::new(
                self.distance * cp * cy,
                self.distance * sp,
                self.distance * cp * sy,
            )
    }

    pub fn view_matrix(&self) -> Mat4 {
        Mat4::look_at_rh(self.position(), self.target, Vec3::Y)
    }

    pub fn proj_matrix(&self) -> Mat4 {
        Mat4::perspective_rh(self.fov_y, self.aspect, self.near, self.far)
    }

    pub fn view_projection(&self) -> Mat4 {
        self.proj_matrix() * self.view_matrix()
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx;
        self.pitch = (self.pitch + dy).clamp(-1.5, 1.5);
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance * (1.0 - delta)).clamp(0.5, 500.0);
    }

    pub fn pan(&mut self, dx: f32, dy: f32) {
        // Панорама в плоскости, перпендикулярной взгляду.
        let view = self.view_matrix();
        let right = Vec3::new(view.x_axis.x, view.y_axis.x, view.z_axis.x);
        let up = Vec3::new(view.x_axis.y, view.y_axis.y, view.z_axis.y);
        self.target += (right * -dx + up * dy) * self.distance * 0.002;
    }

    /// Возвращает 6 плоскостей frustum: [left, right, bottom, top, near, far],
    /// каждая — Vec4(a,b,c,d), где a·x+b·y+c·z+d >= 0 внутри.
    pub fn frustum_planes(&self) -> [Vec4; 6] {
        let m = self.view_projection();
        // Извлечение плоскостей из матрицы (метод Gribb/Hartmann)
        [
            m.row(3) + m.row(0), // left
            m.row(3) - m.row(0), // right
            m.row(3) + m.row(1), // bottom
            m.row(3) - m.row(1), // top
            m.row(2),            // near (для RH с [0,1] depth)
            m.row(3) - m.row(2), // far
        ]
        .map(|p| p.normalize())
    }
}