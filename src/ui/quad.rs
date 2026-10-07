//! UiQuad — один инстанс UI. Screen-space прямоугольник с цветом и UV.
//! Рендерится instanced-квадами: 6 вершин генерируются в шейдере.

use bytemuck::{Pod, Zeroable};

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct UiQuad {
    /// x, y, w, h в пикселях (top-left origin).
    pub rect: [f32; 4],
    /// RGBA (premultiplied не нужен, используем straight alpha).
    pub color: [f32; 4],
    /// u0, v0, u1, v1 — область атласа.
    pub uv: [f32; 4],
}

impl UiQuad {
    const ATTRS: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
        2 => Float32x4,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}

/// Uniform-блок для UI-шейдера. Screen size в пикселях и
/// предвычисленные 2/w, 2/h для NDC-конверсии.
#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct UiGlobal {
    /// x = width_px, y = height_px, z = 2/width, w = 2/height
    pub screen: [f32; 4],
    /// x = time (для анимаций)
    pub time: [f32; 4],
}