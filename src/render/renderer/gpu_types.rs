//! GPU-структуры, константы и вспомогательные функции.

use std::sync::Arc;

use bytemuck::{Pod, Zeroable};
use wgpu::util::DeviceExt;

use crate::render::csm::CASCADE_COUNT;
use crate::render::debug::DebugView;
use crate::render::material::Material;
use crate::render::mesh::InstanceData;
use crate::render::skinning::MAX_JOINTS;
use crate::render::texture::Texture;

pub const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
pub const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const GBUFFER_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const SSAO_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R16Float;
pub const MOTION_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg16Float;
pub const LDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

pub const VOLUMETRIC_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
pub const VOLUMETRIC_GRID_W: u32 = 160;
pub const VOLUMETRIC_GRID_H: u32 = 90;
pub const VOLUMETRIC_GRID_D: u32 = 64;

pub const MAX_DIR_LIGHTS: usize = 4;
pub const MAX_POINT_LIGHTS: usize = 16;

pub const MAX_SHADOW_CUBES: usize = 4;

pub const SHADOW_SLOT_COUNT: u64 = (3 + 6 * MAX_SHADOW_CUBES) as u64;

pub const TAA_JITTER_SEQUENCE: u32 = 8;

// ============================================================
// GPU-структуры
// ============================================================

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct GpuLight {
    pub direction: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct GpuPointLight {
    pub position: [f32; 4],
    pub color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct CameraUniform {
    pub view_proj: [[f32; 4]; 4],
    pub inv_view_proj: [[f32; 4]; 4],
    pub view: [[f32; 4]; 4],
    pub inv_view: [[f32; 4]; 4],
    pub camera_pos: [f32; 4],
    pub near_far: [f32; 4],
    pub prev_view_proj: [[f32; 4]; 4],
    pub screen_size: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LightsUniform {
    pub cascade_vp: [[[f32; 4]; 4]; CASCADE_COUNT],
    pub cascade_splits: [f32; 4],
    pub ambient_color: [f32; 4],
    pub counts: [u32; 4],
    pub light_view_proj: [[f32; 4]; 4],
    pub misc: [f32; 4],
    pub fog_params: [f32; 4],
    pub fog_color: [f32; 4],
    pub shadow_params: [f32; 4],
    pub dir_lights: [[f32; 4]; 8],
    pub point_lights: [[f32; 4]; 32],
    pub cube_shadow_pos: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct VolumetricParams {
    pub grid: [f32; 4],
    pub cam: [f32; 4],
    pub params: [f32; 4],
    pub fog_color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct PostParams {
    pub values: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct BloomParams {
    pub texel: [f32; 4],
    pub params: [f32; 4],
}

// ============================================================
// Tonemap (Спринт 1.1 + 1.2)
// ============================================================

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TonemapParams {
    pub values: [f32; 4],
    pub effects: [f32; 4],
    pub grading_a: [f32; 4],
    pub grading_b: [f32; 4],
    pub lift: [f32; 4],
    pub gain: [f32; 4],
    pub gamma: [f32; 4],
}

// ============================================================
// Lens Flare (Спринт 1.4)
// ============================================================

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct LensFlareParams {
    pub sun_screen: [f32; 4],
    pub sun_color: [f32; 4],
    pub params: [f32; 4],
}

// ============================================================
// Depth of Field (Спринт 2.1)
// ============================================================

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct DofParams {
    pub params: [f32; 4],
    pub screen: [f32; 4],
    pub depth: [f32; 4],
}

// ============================================================
// Motion Blur (Спринт 2.2)
// ============================================================

/// Uniform для motion blur.
///
/// Layout (32 байта):
///   params: x = intensity (множитель velocity), y = max_blur_px,
///           z = samples (float), w = unused
///   screen: xy = (w_px, h_px), zw = unused
#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MotionBlurParams {
    pub params: [f32; 4],
    pub screen: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SkyboxParams {
    pub values: [f32; 4],
    pub fog_color: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct MaterialUniform {
    pub base_color: [f32; 4],
    pub emissive: [f32; 4],
    pub params: [f32; 4],
    pub flags: [u32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SkeletonUniform {
    pub joints: [[[f32; 4]; 4]; MAX_JOINTS],
}

impl SkeletonUniform {
    pub fn identity() -> Self {
        Self {
            joints: [glam::Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS],
        }
    }

    pub fn from_matrices(mats: &[glam::Mat4]) -> Self {
        let mut joints = [glam::Mat4::IDENTITY.to_cols_array_2d(); MAX_JOINTS];
        for (i, m) in mats.iter().take(MAX_JOINTS).enumerate() {
            joints[i] = m.to_cols_array_2d();
        }
        Self { joints }
    }
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct SsaoUniform {
    pub proj_scale: [f32; 4],
    pub params: [f32; 4],
    pub time: [f32; 4],
    pub view: [[f32; 4]; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct TaaParams {
    pub values: [f32; 4],
    pub screen: [f32; 4],
    pub prev_jitter: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct DebugParams {
    pub mode: [u32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct FxaaParams {
    pub values: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Pod, Zeroable)]
pub struct ParticleInstance {
    pub position_size: [f32; 4],
    pub color: [f32; 4],
}

impl ParticleInstance {
    const ATTRS: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![
        0 => Float32x4,
        1 => Float32x4,
    ];

    pub fn layout() -> wgpu::VertexBufferLayout<'static> {
        wgpu::VertexBufferLayout {
            array_stride: std::mem::size_of::<Self>() as u64,
            step_mode: wgpu::VertexStepMode::Instance,
            attributes: &Self::ATTRS,
        }
    }
}

pub struct MeshDraw {
    pub mesh: String,
    pub instances: Vec<InstanceData>,
    pub texture: Option<String>,
    pub blend: bool,
    pub double_sided: bool,
}

// ============================================================
// Дефолты для PostFx
// ============================================================

fn default_volumetric_density() -> f32 { 0.001 }
fn default_volumetric_scattering() -> f32 { 0.4 }
fn default_volumetric_phase_g() -> f32 { 0.6 }

fn default_one() -> f32 { 1.0 }
fn default_zero_rgb() -> [f32; 3] { [0.0, 0.0, 0.0] }
fn default_one_rgb() -> [f32; 3] { [1.0, 1.0, 1.0] }

fn default_lens_flare_threshold() -> f32 { 1.5 }
fn default_lens_flare_ghosts() -> u32 { 6 }
fn default_lens_flare_streak() -> f32 { 0.15 }

fn default_dof_focus_distance() -> f32 { 10.0 }
fn default_dof_focus_range() -> f32 { 3.0 }
fn default_dof_max_blur() -> f32 { 8.0 }
fn default_dof_blur_falloff() -> f32 { 4.0 }

// Спринт 2.2: Motion blur defaults
fn default_motion_blur_intensity() -> f32 { 0.5 }
fn default_motion_blur_max() -> f32 { 32.0 }
fn default_motion_blur_samples() -> u32 { 12 }

#[derive(Debug, Copy, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum Tonemapper {
    AgX = 0,
    ACESFilmic = 1,
    Reinhard = 2,
    Uncharted2 = 3,
    None = 4,
}

impl Default for Tonemapper {
    fn default() -> Self { Self::AgX }
}

#[derive(Debug, Copy, Clone, serde::Serialize, serde::Deserialize)]
pub struct PostFx {
    pub bloom_threshold: f32,
    pub bloom_strength: f32,
    pub bloom_knee: f32,
    pub bloom_radius: f32,
    pub exposure: f32,
    pub ssao_strength: f32,
    pub ssao_radius: f32,
    pub ibl_strength: f32,
    pub debug_view: DebugView,
    pub fxaa_strength: f32,
    pub fog_color: [f32; 3],
    pub fog_density: f32,
    pub fog_height_base: f32,
    pub fog_height_falloff: f32,
    pub vignette_strength: f32,
    pub film_grain: f32,
    pub chromatic_aberration: f32,
    pub shadow_bias: f32,
    pub shadow_normal_bias: f32,
    pub shadow_fade_start: f32,
    pub shadow_fade_end: f32,
    pub lod_bias: f32,
    pub lod_distances: [f32; 4],
    pub taa_strength: f32,
    pub taa_sharpening: f32,

    #[serde(default = "default_volumetric_density")]
    pub volumetric_density: f32,
    #[serde(default = "default_volumetric_scattering")]
    pub volumetric_scattering: f32,
    #[serde(default = "default_volumetric_phase_g")]
    pub volumetric_phase_g: f32,

    // Спринт 1.1 + 1.2
    #[serde(default)]
    pub tonemapper: Tonemapper,
    #[serde(default = "default_one")]
    pub exposure_bias: f32,
    #[serde(default)]
    pub color_temperature: f32,
    #[serde(default)]
    pub color_tint: f32,
    #[serde(default = "default_one")]
    pub color_contrast: f32,
    #[serde(default = "default_one")]
    pub color_saturation: f32,
    #[serde(default = "default_zero_rgb")]
    pub color_lift: [f32; 3],
    #[serde(default = "default_one_rgb")]
    pub color_gain: [f32; 3],
    #[serde(default = "default_one_rgb")]
    pub color_gamma: [f32; 3],

    // Спринт 1.4
    #[serde(default)]
    pub lens_flare_intensity: f32,
    #[serde(default = "default_lens_flare_threshold")]
    pub lens_flare_threshold: f32,
    #[serde(default = "default_lens_flare_ghosts")]
    pub lens_flare_ghosts: u32,
    #[serde(default = "default_lens_flare_streak")]
    pub lens_flare_streak: f32,

    // Спринт 2.1
    #[serde(default)]
    pub dof_enabled: f32,
    #[serde(default = "default_dof_focus_distance")]
    pub dof_focus_distance: f32,
    #[serde(default = "default_dof_focus_range")]
    pub dof_focus_range: f32,
    #[serde(default = "default_dof_max_blur")]
    pub dof_max_blur: f32,
    #[serde(default = "default_dof_blur_falloff")]
    pub dof_blur_falloff: f32,

    // === Спринт 2.2: Motion Blur ===
    #[serde(default)]
    pub motion_blur_enabled: f32,
    #[serde(default = "default_motion_blur_intensity")]
    pub motion_blur_intensity: f32,
    #[serde(default = "default_motion_blur_max")]
    pub motion_blur_max_px: f32,
    #[serde(default = "default_motion_blur_samples")]
    pub motion_blur_samples: u32,
}

impl Default for PostFx {
    fn default() -> Self {
        Self {
            bloom_threshold: 1.2,
            bloom_strength: 0.6,
            bloom_knee: 0.5,
            bloom_radius: 1.0,
            exposure: 0.6,
            ssao_strength: 0.8,
            ssao_radius: 0.6,
            ibl_strength: 0.15,
            debug_view: DebugView::Final,
            fxaa_strength: 1.0,
            fog_color: [0.55, 0.62, 0.72],
            fog_density: 0.0,
            fog_height_base: 0.0,
            fog_height_falloff: 0.05,
            vignette_strength: 0.0,
            film_grain: 0.0,
            chromatic_aberration: 0.0,
            shadow_bias: 0.0015,
            shadow_normal_bias: 3.0,
            shadow_fade_start: 150.0,
            shadow_fade_end: 200.0,
            lod_bias: 1.0,
            lod_distances: [30.0, 80.0, 200.0, 500.0],
            taa_strength: 1.0,
            taa_sharpening: 0.1,
            volumetric_density: 0.001,
            volumetric_scattering: 0.4,
            volumetric_phase_g: 0.6,

            tonemapper: Tonemapper::AgX,
            exposure_bias: 1.0,
            color_temperature: 0.0,
            color_tint: 0.0,
            color_contrast: 1.0,
            color_saturation: 1.0,
            color_lift: [0.0, 0.0, 0.0],
            color_gain: [1.0, 1.0, 1.0],
            color_gamma: [1.0, 1.0, 1.0],

            lens_flare_intensity: 0.0,
            lens_flare_threshold: 1.5,
            lens_flare_ghosts: 6,
            lens_flare_streak: 0.15,

            dof_enabled: 0.0,
            dof_focus_distance: 10.0,
            dof_focus_range: 3.0,
            dof_max_blur: 8.0,
            dof_blur_falloff: 4.0,

            motion_blur_enabled: 0.0,
            motion_blur_intensity: 0.5,
            motion_blur_max_px: 32.0,
            motion_blur_samples: 12,
        }
    }
}

// ============================================================
// Helper-функции
// ============================================================

pub struct MaterialGpu {
    pub bind_group: wgpu::BindGroup,
    pub _material_uniform: wgpu::Buffer,
    pub skeleton_uniform: Arc<wgpu::Buffer>,
}

pub fn create_depth_view(
    device: &wgpu::Device,
    width: u32,
    height: u32,
    sample_count: u32,
) -> wgpu::TextureView {
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("depth_texture"),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format: DEPTH_FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

pub fn create_color_target(
    device: &wgpu::Device,
    label: &str,
    width: u32,
    height: u32,
    format: wgpu::TextureFormat,
    sample_count: u32,
    with_binding: bool,
) -> wgpu::TextureView {
    let mut usage = wgpu::TextureUsages::RENDER_ATTACHMENT;
    if with_binding {
        usage |= wgpu::TextureUsages::TEXTURE_BINDING;
    }
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some(label),
        size: wgpu::Extent3d { width, height, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage,
        view_formats: &[],
    });
    texture.create_view(&wgpu::TextureViewDescriptor::default())
}

pub fn create_linear_sampler(device: &wgpu::Device, label: &str) -> wgpu::Sampler {
    device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some(label),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        ..Default::default()
    })
}

pub fn create_noise_texture(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::Texture, wgpu::TextureView) {
    let size = 4u32;

    fn f32_to_f16(v: f32) -> u16 {
        let bits = v.to_bits();
        let sign = ((bits >> 31) & 0x1) as u16;
        let exp = ((bits >> 23) & 0xFF) as i32 - 127 + 15;
        let mant = (bits >> 13) & 0x3FF;
        if exp <= 0 { return sign << 15; }
        if exp >= 31 { return (sign << 15) | 0x7C00; }
        (sign << 15) | ((exp as u16) << 10) | (mant as u16)
    }

    let mut seed = 12345678u32;
    let mut data = vec![0u8; (size * size * 4 * 2) as usize];
    for y in 0..size {
        for x in 0..size {
            let idx = ((y * size + x) * 4) as usize * 2;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let nx = ((seed >> 8) & 0xFFFF) as f32 / 65535.0 * 2.0 - 1.0;
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            let ny = ((seed >> 8) & 0xFFFF) as f32 / 65535.0 * 2.0 - 1.0;
            let vals = [nx, ny, 0.0f32, 1.0f32];
            for (i, v) in vals.iter().enumerate() {
                let h = f32_to_f16(*v);
                data[idx + i * 2] = (h & 0xFF) as u8;
                data[idx + i * 2 + 1] = (h >> 8) as u8;
            }
        }
    }

    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("ssao_noise"),
        size: wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba16Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture: &texture,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(size * 8),
            rows_per_image: Some(size),
        },
        wgpu::Extent3d { width: size, height: size, depth_or_array_layers: 1 },
    );

    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

pub fn create_identity_skeleton_buffer(device: &wgpu::Device) -> Arc<wgpu::Buffer> {
    let data = SkeletonUniform::identity();
    Arc::new(
        device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("skeleton_uniform_identity"),
            contents: bytemuck::bytes_of(&data),
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        }),
    )
}

pub fn build_object_bind_group(
    device: &wgpu::Device,
    layout: &wgpu::BindGroupLayout,
    material: &Material,
    base_tex: &Texture,
    mr_tex: &Texture,
    normal_tex: &Texture,
    emissive_tex: &Texture,
    sampler: &wgpu::Sampler,
    skeleton_uniform: Arc<wgpu::Buffer>,
) -> MaterialGpu {
    let alpha_mode_u32: u32 = match material.alpha_mode {
        crate::render::material::AlphaMode::Opaque => 0,
        crate::render::material::AlphaMode::Mask => 1,
        crate::render::material::AlphaMode::Blend => 2,
    };

    let uniform = MaterialUniform {
        base_color: material.base_color,
        emissive: [
            material.emissive[0], material.emissive[1], material.emissive[2], 1.0,
        ],
        params: [
            material.metallic,
            material.roughness,
            material.normal_scale,
            material.alpha_cutoff,
        ],
        flags: [alpha_mode_u32, 0, 0, 0],
    };

    let material_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("material_uniform"),
        contents: bytemuck::bytes_of(&uniform),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    });

    let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("object_bind_group"),
        layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&base_tex.view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&mr_tex.view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&normal_tex.view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&emissive_tex.view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: material_uniform.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: skeleton_uniform.as_entire_binding(),
            },
        ],
    });

    MaterialGpu {
        bind_group,
        _material_uniform: material_uniform,
        skeleton_uniform,
    }
}