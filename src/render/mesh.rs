use bytemuck::{Pod, Zeroable};
use glam::{Mat3, Mat4, Vec3};
use wgpu::util::DeviceExt;

/// Вершина 3D-меша. Поддерживает 4-костный скелетный скиннинг.
///
/// Если `weights` все нули — скиннинг не применяется (обычный меш).
/// Если хотя бы одна weight > 0 — позиция/normal считаются как смесь
/// `world_bone_matrix * position` по 4 костям.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct Vertex3D {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub color: [f32; 4],
    /// Индексы 4 костей, влияющих на вершину.
    pub joints: [u32; 4],
    /// Веса 4 костей. Сумма весов = 1.0 для скелетных мешей.
    pub weights: [f32; 4],
}

impl Vertex3D {
    /// Статическая вершина без скелета.
    pub fn static_vertex(
        position: [f32; 3],
        normal: [f32; 3],
        uv: [f32; 2],
        color: [f32; 4],
    ) -> Self {
        Self {
            position,
            normal,
            uv,
            color,
            joints: [0; 4],
            weights: [0.0; 4],
        }
    }

    const ATTRS: [wgpu::VertexAttribute; 6] = wgpu::vertex_attr_array![
        0 => Float32x3,
        1 => Float32x3,
        2 => Float32x2,
        3 => Float32x4,
        4 => Uint32x4,
        5 => Float32x4
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Vertex,
            attributes: &Self::ATTRS,
        }
    }
}

/// Per-instance данные: матрица модели + inverse-transpose 3×3 + цвет.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct InstanceData {
    pub model: [[f32; 4]; 4],
    pub normal_matrix: [[f32; 4]; 4],
    pub color: [f32; 4],
}

impl InstanceData {
    pub fn new(model: Mat4, color: [f32; 4]) -> Self {
        let m3 = Mat3::from_mat4(model);
        let normal = if m3.determinant().abs() > 1e-8 {
            m3.inverse().transpose()
        } else {
            Mat3::IDENTITY
        };
        Self {
            model: model.to_cols_array_2d(),
            normal_matrix: Mat4::from_mat3(normal).to_cols_array_2d(),
            color,
        }
    }

    // Вершинные атрибуты теперь занимают 0..5, значит инстансные — с 6.
    const ATTRS: [wgpu::VertexAttribute; 9] = wgpu::vertex_attr_array![
        6  => Float32x4, 7  => Float32x4, 8  => Float32x4, 9  => Float32x4,
        10 => Float32x4, 11 => Float32x4, 12 => Float32x4, 13 => Float32x4,
        14 => Float32x4
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}

pub struct Mesh {
    pub vertex_buffer: wgpu::Buffer,
    pub index_buffer: wgpu::Buffer,
    pub index_count: u32,
    pub bounds_center: Vec3,
    pub bounds_radius: f32,
}

impl Mesh {
    pub fn new(
        device: &wgpu::Device,
        vertices: &[Vertex3D],
        indices: &[u32],
        label: &str,
    ) -> Self {
        let vertex_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}_vb")),
            contents: bytemuck::cast_slice(vertices),
            usage: wgpu::BufferUsages::VERTEX,
        });
        let index_buffer = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some(&format!("{label}_ib")),
            contents: bytemuck::cast_slice(indices),
            usage: wgpu::BufferUsages::INDEX,
        });

        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        for v in vertices {
            let p = Vec3::from(v.position);
            min = min.min(p);
            max = max.max(p);
        }
        let center = (min + max) * 0.5;
        let mut radius = 0.0f32;
        for v in vertices {
            let p = Vec3::from(v.position);
            radius = radius.max((p - center).length());
        }

        Self {
            vertex_buffer,
            index_buffer,
            index_count: indices.len() as u32,
            bounds_center: center,
            bounds_radius: radius,
        }
    }

    pub fn world_bounds(&self, model: &Mat4) -> (Vec3, f32) {
        let center_world = model.transform_point3(self.bounds_center);
        let m3 = Mat3::from_mat4(*model);
        let sx = m3.x_axis.length();
        let sy = m3.y_axis.length();
        let sz = m3.z_axis.length();
        let scale = sx.max(sy).max(sz);
        (center_world, self.bounds_radius * scale)
    }

    // ============================================================
    // Генераторы (все возвращают меши без скелета — joints=[0;4], weights=0)
    // ============================================================

    pub fn cube(device: &wgpu::Device, size: f32) -> Self {
        let h = size * 0.5;
        let mut vertices = Vec::with_capacity(24);
        let mut indices = Vec::with_capacity(36);

        let faces: [([f32; 3], [f32; 3], [f32; 3], [f32; 3]); 6] = [
            ([0.0, 0.0, 1.0],  [1.0, 0.0, 0.0],  [0.0, 1.0, 0.0],  [0.0, 0.0, 1.0]),
            ([0.0, 0.0, -1.0], [-1.0, 0.0, 0.0], [0.0, 1.0, 0.0],  [0.0, 0.0, -1.0]),
            ([1.0, 0.0, 0.0],  [0.0, 0.0, -1.0], [0.0, 1.0, 0.0],  [1.0, 0.0, 0.0]),
            ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0],  [0.0, 1.0, 0.0],  [-1.0, 0.0, 0.0]),
            ([0.0, 1.0, 0.0],  [1.0, 0.0, 0.0],  [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]),
            ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0],  [0.0, 0.0, 1.0],  [0.0, -1.0, 0.0]),
        ];

        for (normal, right, up, base) in faces.iter() {
            let start = vertices.len() as u32;
            let corners = [(-1.0f32, -1.0f32), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
            let uvs = [(0.0f32, 1.0f32), (1.0, 1.0), (1.0, 0.0), (0.0, 0.0)];

            for (i, (cr, cu)) in corners.iter().enumerate() {
                let pos = [
                    (base[0] + right[0] * cr + up[0] * cu) * h,
                    (base[1] + right[1] * cr + up[1] * cu) * h,
                    (base[2] + right[2] * cr + up[2] * cu) * h,
                ];
                vertices.push(Vertex3D::static_vertex(
                    pos,
                    *normal,
                    [uvs[i].0, uvs[i].1],
                    [1.0, 1.0, 1.0, 1.0],
                ));
            }
            indices.extend_from_slice(&[start, start + 1, start + 2, start, start + 2, start + 3]);
        }

        Self::new(device, &vertices, &indices, "cube")
    }

    pub fn sphere(device: &wgpu::Device, radius: f32, rings: u32, segments: u32) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        for ring in 0..=rings {
            let phi = std::f32::consts::PI * ring as f32 / rings as f32;
            let (sp, cp) = phi.sin_cos();
            let y = cp * radius;
            let r = sp * radius;

            for seg in 0..=segments {
                let theta = std::f32::consts::TAU * seg as f32 / segments as f32;
                let (st, ct) = theta.sin_cos();
                vertices.push(Vertex3D::static_vertex(
                    [ct * r, y, st * r],
                    [ct * sp, cp, st * sp],
                    [seg as f32 / segments as f32, ring as f32 / rings as f32],
                    [1.0, 1.0, 1.0, 1.0],
                ));
            }
        }

        let stride = segments + 1;
        for ring in 0..rings {
            for seg in 0..segments {
                let a = ring * stride + seg;
                let b = a + stride;
                indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }

        Self::new(device, &vertices, &indices, "sphere")
    }

    pub fn plane(device: &wgpu::Device, size: f32, subdivisions: u32) -> Self {
        let mut vertices = Vec::new();
        let mut indices = Vec::new();
        let n = subdivisions.max(1);
        let h = size * 0.5;

        for z in 0..=n {
            for x in 0..=n {
                let fx = x as f32 / n as f32;
                let fz = z as f32 / n as f32;
                vertices.push(Vertex3D::static_vertex(
                    [fx * size - h, 0.0, fz * size - h],
                    [0.0, 1.0, 0.0],
                    [fx, fz],
                    [1.0, 1.0, 1.0, 1.0],
                ));
            }
        }

        let stride = n + 1;
        for z in 0..n {
            for x in 0..n {
                let a = z * stride + x;
                let b = a + stride;
                indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
            }
        }

        Self::new(device, &vertices, &indices, "plane")
    }
}