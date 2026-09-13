//! Рендерер: публичный API, управление ресурсами, диспетчер проходов.
//!
//! Deferred pipeline (14B + 14C):
//!   1. Shadow (3 CSM + 6 cube)      — encode_csm_all / encode_cube_shadow_all
//!   2. G-buffer (3 MRT + depth)     — encode_gbuffer_pass
//!   3. SSAO + blur                  — encode_ssao_pass / encode_ssao_blur_pass
//!   4. Lighting (fullscreen)        — encode_lighting_pass
//!   5. Forward (sky + lines)        — encode_forward_pass
//!   6. Post-processing              — encode_post_processing
//!   7. Debug (F1–F6)                — encode_debug_pass

mod gpu_types;
mod passes;
mod size_dep;

pub use gpu_types::{GpuLight, GpuPointLight, MeshDraw, PostFx, MAX_DIR_LIGHTS, MAX_POINT_LIGHTS};

use std::collections::HashMap;
use std::sync::Arc;
use glam::{Mat4, Vec3};
use winit::window::Window;

use crate::render::camera::Camera3D;
use crate::render::csm::{self, CASCADE_COUNT};
use crate::render::ibl;
use crate::render::line::{LineBuffer, LineVertex};
use crate::render::material::{Material, MaterialRegistry};
use crate::render::mesh::{InstanceData, Mesh, Vertex3D};
use crate::render::shadow_cube;
use crate::render::texture::Texture;

use gpu_types::*;
use size_dep::{build_size_dependent, SizeDependent};

pub struct Renderer {
    pub surface: wgpu::Surface<'static>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub config: wgpu::SurfaceConfiguration,
    pub size: winit::dpi::PhysicalSize<u32>,

    // Camera
    camera_buffer: wgpu::Buffer,
    camera_bind_group: wgpu::BindGroup,

    // Lights (main pass)
    lights_buffer: wgpu::Buffer,
    lights_bind_group: wgpu::BindGroup,

    // Shadow pass: 9 слотов (3 CSM + 6 cube faces)
    shadow_pass_buffer: wgpu::Buffer,
    shadow_pass_bind_group: wgpu::BindGroup,
    shadow_pass_stride: u64,

    // CSM
    csm_array_view: wgpu::TextureView,
    csm_cascade_views: [wgpu::TextureView; CASCADE_COUNT],
    csm_sampler: wgpu::Sampler,

    // Cube shadow (для первого point-light)
    cube_shadow_cube_view: wgpu::TextureView,
    cube_shadow_face_views: [wgpu::TextureView; 6],
    cube_shadow_sampler: wgpu::Sampler,

    // Group 2: CSM + cube + SSAO + IBL (11 bindings)
    shadow2_layout: wgpu::BindGroupLayout,

    // Textures / materials
    texture_layout: wgpu::BindGroupLayout,
    material_layout: wgpu::BindGroupLayout,
    material_bind_groups: HashMap<String, MaterialGpu>,
    default_material_bind_group: MaterialGpu,

    /// Буферы скелетов: имя → GPU-буфер с 64 матрицами (14C).
    pub skeleton_buffers: HashMap<String, wgpu::Buffer>,

    // Fallback-текстуры
    fallback_texture: Texture,
    fallback_mr: Texture,
    fallback_normal: Texture,
    fallback_emissive: Texture,

    // Pipelines
    sky_pipeline: wgpu::RenderPipeline,
    gbuffer_pipeline: wgpu::RenderPipeline,
    shadow_pipeline: wgpu::RenderPipeline,
    line_pipeline: wgpu::RenderPipeline,
    lighting_pipeline: wgpu::RenderPipeline,
    ssao_pipeline: wgpu::RenderPipeline,
    ssao_blur_pipeline: wgpu::RenderPipeline,
    bright_pipeline: wgpu::RenderPipeline,
    blur_pipeline: wgpu::RenderPipeline,
    tonemap_pipeline: wgpu::RenderPipeline,

    // Debug pipelines
    debug_layout: wgpu::BindGroupLayout,
    debug_depth_layout: wgpu::BindGroupLayout,
    debug2d_pipeline: wgpu::RenderPipeline,
    debug_depth_pipeline: wgpu::RenderPipeline,
    csm_debug_bind_group: wgpu::BindGroup,

    // Layouts
    lighting_layout: wgpu::BindGroupLayout,
    bloom_layout: wgpu::BindGroupLayout,
    tonemap_layout: wgpu::BindGroupLayout,
    ssao_layout: wgpu::BindGroupLayout,

    // Uniforms
    bloom_uniform_h: wgpu::Buffer,
    bloom_uniform_v: wgpu::Buffer,
    bright_uniform: wgpu::Buffer,
    tonemap_uniform: wgpu::Buffer,
    ssao_uniform: wgpu::Buffer,

    // Noise (для SSAO)
    _ssao_noise_tex: wgpu::Texture,
    ssao_noise_view: wgpu::TextureView,

    // IBL
    ibl: Option<ibl::IblResources>,

    // Size-dependent
    sd: SizeDependent,

    // Instancing / lines
    instance_buffer: wgpu::Buffer,
    instance_capacity: u64,
    pub line_buffer: LineBuffer,

    // Реестры
    pub meshes: HashMap<String, Mesh>,
    pub materials: MaterialRegistry,
    pub textures: HashMap<String, Texture>,
    default_material: Material,
}

impl Renderer {
    pub async fn new(window: Arc<Window>) -> Self {
        let size = window.inner_size();

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::all(),
            ..Default::default()
        });
        let surface = instance.create_surface(window.clone()).unwrap();
        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            })
            .await
            .unwrap();
        let (device, queue) = adapter
            .request_device(
                &wgpu::DeviceDescriptor {
                    label: Some("device"),
                    required_features: wgpu::Features::empty(),
                    required_limits: wgpu::Limits::default(),
                },
                None,
            )
            .await
            .unwrap();

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .copied()
            .find(|f| f.is_srgb())
            .unwrap_or(caps.formats[0]);

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width: size.width.max(1),
            height: size.height.max(1),
            present_mode: wgpu::PresentMode::AutoVsync,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        // ============================================================
        // Layouts
        // ============================================================
        let camera_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("camera_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let lights_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lights_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let shadow_pass_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow_pass_layout"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Uniform,
                    has_dynamic_offset: true,
                    min_binding_size: wgpu::BufferSize::new(
                        std::mem::size_of::<LightsUniform>() as u64,
                    ),
                },
                count: None,
            }],
        });

        // Group 2: CSM + cube + SSAO + IBL (11 bindings)
        let shadow2_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("shadow2_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Comparison),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 7,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 8,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 9,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 10,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::Cube,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });

        let texture_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("texture_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // 7 bindings: 4 textures + sampler + material uniform + skeleton uniform
        let material_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("material_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: wgpu::BufferSize::new(
                            std::mem::size_of::<SkeletonUniform>() as u64,
                        ),
                    },
                    count: None,
                },
            ],
        });

        let ssao_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("ssao_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        // Lighting layout (group 0 в deferred_lighting.wgsl)
        let lighting_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("lighting_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 4,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let bloom_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("bloom_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let tonemap_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("tonemap_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 3,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let debug_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let debug_depth_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("debug_depth_layout"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Depth,
                        view_dimension: wgpu::TextureViewDimension::D2Array,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });

        // ============================================================
        // Uniforms
        // ============================================================
        let camera_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("camera_buffer"),
            size: std::mem::size_of::<CameraUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let camera_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("camera_bind_group"),
            layout: &camera_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: camera_buffer.as_entire_binding(),
            }],
        });

        let lights_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("lights_buffer"),
            size: std::mem::size_of::<LightsUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let lights_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("lights_bind_group"),
            layout: &lights_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: lights_buffer.as_entire_binding(),
            }],
        });

        let align = device.limits().min_uniform_buffer_offset_alignment as u64;
        let light_size = std::mem::size_of::<LightsUniform>() as u64;
        let shadow_pass_stride = light_size.div_ceil(align) * align;

        let shadow_pass_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("shadow_pass_buffer"),
            size: shadow_pass_stride * SHADOW_SLOT_COUNT,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let shadow_pass_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("shadow_pass_bind_group"),
            layout: &shadow_pass_layout,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                    buffer: &shadow_pass_buffer,
                    offset: 0,
                    size: wgpu::BufferSize::new(light_size),
                }),
            }],
        });

        let bloom_uniform_h = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom_uniform_h"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bloom_uniform_v = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bloom_uniform_v"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bright_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("bright_uniform"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let tonemap_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("tonemap_uniform"),
            size: std::mem::size_of::<PostParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let ssao_uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("ssao_uniform"),
            size: std::mem::size_of::<SsaoUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        // ============================================================
        // CSM
        // ============================================================
        let csm_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("csm_texture"),
            size: wgpu::Extent3d {
                width: csm::CASCADE_SIZE,
                height: csm::CASCADE_SIZE,
                depth_or_array_layers: CASCADE_COUNT as u32,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let csm_array_view = csm_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("csm_array_view"),
            dimension: Some(wgpu::TextureViewDimension::D2Array),
            base_array_layer: 0,
            array_layer_count: Some(CASCADE_COUNT as u32),
            ..Default::default()
        });
        let csm_cascade_views: [wgpu::TextureView; CASCADE_COUNT] = std::array::from_fn(|i| {
            csm_texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("csm_cascade_view"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let csm_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("csm_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        // ============================================================
        // Cube shadow
        // ============================================================
        let cube_texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("cube_shadow_texture"),
            size: wgpu::Extent3d {
                width: shadow_cube::CUBE_SIZE,
                height: shadow_cube::CUBE_SIZE,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: DEPTH_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let cube_shadow_cube_view = cube_texture.create_view(&wgpu::TextureViewDescriptor {
            label: Some("cube_shadow_cube_view"),
            dimension: Some(wgpu::TextureViewDimension::Cube),
            base_array_layer: 0,
            array_layer_count: Some(6),
            ..Default::default()
        });
        let cube_shadow_face_views: [wgpu::TextureView; 6] = std::array::from_fn(|i| {
            cube_texture.create_view(&wgpu::TextureViewDescriptor {
                label: Some("cube_shadow_face_view"),
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let cube_shadow_sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("cube_shadow_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            compare: Some(wgpu::CompareFunction::LessEqual),
            ..Default::default()
        });

        // ============================================================
        // IBL
        // ============================================================
        let ibl = ibl::IblResources::load_or_default(&device, &queue, "assets/sky.hdr")
            .expect("IBL init failed");

        // ============================================================
        // Pipelines
        // ============================================================
        let sky_pipeline = make_sky_pipeline(
            &device,
            &camera_layout,
            &lights_layout,
            &shadow2_layout,
        );

        let gbuffer_pipeline = make_gbuffer_pipeline(
            &device,
            &camera_layout,
            &material_layout,
        );

        let shadow_pipeline = make_shadow_pipeline(&device, &shadow_pass_layout);

        let line_pipeline = make_line_pipeline(&device, &camera_layout);

        let lighting_pipeline = make_lighting_pipeline(
            &device,
            &lighting_layout,
            &lights_layout,
            &shadow2_layout,
        );

        let (ssao_pipeline, ssao_blur_pipeline) = make_ssao_pipelines(&device, &ssao_layout);

        let (bright_pipeline, blur_pipeline) = make_bloom_pipelines(&device, &bloom_layout);

        let tonemap_pipeline = make_tonemap_pipeline(&device, &config, &tonemap_layout);

        let (debug2d_pipeline, debug_depth_pipeline) = make_debug_pipelines(
            &device,
            &config,
            &debug_layout,
            &debug_depth_layout,
        );

        // ============================================================
        // Noise
        // ============================================================
        let (ssao_noise_tex, ssao_noise_view) = create_noise_texture(&device, &queue);

        // ============================================================
        // Size-dependent
        // ============================================================
        let sd = build_size_dependent(
            &device,
            &config,
            &bloom_layout,
            &tonemap_layout,
            &ssao_layout,
            &shadow2_layout,
            &debug_layout,
            &lighting_layout,
            &bloom_uniform_h,
            &bloom_uniform_v,
            &bright_uniform,
            &tonemap_uniform,
            &ssao_uniform,
            &ssao_noise_view,
            &csm_array_view,
            &csm_sampler,
            &cube_shadow_cube_view,
            &cube_shadow_sampler,
            &camera_buffer,
            &ibl,
        );

        // CSM debug bind group
        let csm_debug_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("csm_debug_bg"),
            layout: &debug_depth_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&csm_array_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sd.linear_sampler),
                },
            ],
        });

        // === Buffers ===
        const INITIAL_INSTANCE_CAPACITY: u64 = 4096;
        let instance_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance_buffer"),
            size: INITIAL_INSTANCE_CAPACITY * std::mem::size_of::<InstanceData>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let fallback_texture = Texture::white(&device, &queue, &texture_layout).unwrap();
        let fallback_mr = Texture::from_solid(
            &device, &queue, &texture_layout, [255, 255, 0, 255], "fallback_mr",
        ).unwrap();
        let fallback_normal = Texture::from_solid(
            &device, &queue, &texture_layout, [128, 128, 255, 255], "fallback_normal",
        ).unwrap();
        let fallback_emissive = Texture::from_solid(
            &device, &queue, &texture_layout, [0, 0, 0, 255], "fallback_emissive",
        ).unwrap();

        let default_material = Material::default();
        let default_material_bind_group = build_object_bind_group(
            &device,
            &material_layout,
            &default_material,
            &fallback_texture,
            &fallback_mr,
            &fallback_normal,
            &fallback_emissive,
            &[],
        );

        let line_buffer = LineBuffer::new(&device, 4096);

        Self {
            surface,
            device,
            queue,
            config,
            size,
            camera_buffer,
            camera_bind_group,
            lights_buffer,
            lights_bind_group,
            shadow_pass_buffer,
            shadow_pass_bind_group,
            shadow_pass_stride,
            csm_array_view,
            csm_cascade_views,
            csm_sampler,
            cube_shadow_cube_view,
            cube_shadow_face_views,
            cube_shadow_sampler,
            shadow2_layout,
            texture_layout,
            material_layout,
            material_bind_groups: HashMap::new(),
            default_material_bind_group,
            skeleton_buffers: HashMap::new(),
            fallback_texture,
            fallback_mr,
            fallback_normal,
            fallback_emissive,
            sky_pipeline,
            gbuffer_pipeline,
            shadow_pipeline,
            line_pipeline,
            lighting_pipeline,
            ssao_pipeline,
            ssao_blur_pipeline,
            bright_pipeline,
            blur_pipeline,
            tonemap_pipeline,
            debug_layout,
            debug_depth_layout,
            debug2d_pipeline,
            debug_depth_pipeline,
            csm_debug_bind_group,
            lighting_layout,
            bloom_layout,
            tonemap_layout,
            ssao_layout,
            bloom_uniform_h,
            bloom_uniform_v,
            bright_uniform,
            tonemap_uniform,
            ssao_uniform,
            _ssao_noise_tex: ssao_noise_tex,
            ssao_noise_view,
            ibl: Some(ibl),
            sd,
            instance_buffer,
            instance_capacity: INITIAL_INSTANCE_CAPACITY,
            line_buffer,
            meshes: HashMap::new(),
            materials: MaterialRegistry::new(),
            textures: HashMap::new(),
            default_material,
        }
    }

    // ============================================================
    // Публичный API
    // ============================================================

    pub fn add_mesh(&mut self, name: impl Into<String>, mesh: Mesh) {
        self.meshes.insert(name.into(), mesh);
    }

    pub fn load_texture_rgba(
        &mut self,
        name: &str,
        data: &[u8],
        width: u32,
        height: u32,
    ) -> anyhow::Result<()> {
        let tex = Texture::from_rgba(
            &self.device,
            &self.queue,
            &self.texture_layout,
            data,
            width,
            height,
            name,
        )?;
        self.textures.insert(name.to_string(), tex);
        Ok(())
    }

    pub fn load_texture(&mut self, name: &str, path: &str) -> anyhow::Result<()> {
        let tex = Texture::from_file(&self.device, &self.queue, &self.texture_layout, path)?;
        self.textures.insert(name.to_string(), tex);
        Ok(())
    }

    pub fn load_texture_bytes(&mut self, name: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let tex =
            Texture::from_bytes(&self.device, &self.queue, &self.texture_layout, bytes, name)?;
        self.textures.insert(name.to_string(), tex);
        Ok(())
    }

    pub fn add_material(&mut self, name: impl Into<String>, mat: Material) {
        let name = name.into();
        let base_tex = mat.base_color_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_texture);
        let mr_tex = mat.metallic_roughness_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_mr);
        let normal_tex = mat.normal_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_normal);
        let emissive_tex = mat.emissive_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_emissive);

        // Статичный материал — identity-скелет.
        let gpu = build_object_bind_group(
            &self.device,
            &self.material_layout,
            &mat,
            base_tex,
            mr_tex,
            normal_tex,
            emissive_tex,
            &[],
        );

        self.materials.insert(name.clone(), mat);
        self.material_bind_groups.insert(name, gpu);
    }

    pub fn has_material(&self, name: &str) -> bool {
        self.material_bind_groups.contains_key(name)
    }

    /// Создать материал с привязкой к скелету (14C).
    /// Пока использует identity-матрицы; актуальная поза приходит через
    /// `update_skeleton` каждый кадр.
    pub fn add_material_with_skeleton(
        &mut self,
        name: &str,
        mat: Material,
        _skeleton_name: &str,
    ) {
        let base_tex = mat.base_color_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_texture);
        let mr_tex = mat.metallic_roughness_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_mr);
        let normal_tex = mat.normal_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_normal);
        let emissive_tex = mat.emissive_texture.as_deref()
            .and_then(|n| self.textures.get(n))
            .unwrap_or(&self.fallback_emissive);

        let gpu = build_object_bind_group(
            &self.device,
            &self.material_layout,
            &mat,
            base_tex,
            mr_tex,
            normal_tex,
            emissive_tex,
            &[],
        );

        self.materials.insert(name.to_string(), mat);
        self.material_bind_groups.insert(name.to_string(), gpu);
    }

    pub fn materials_default(&self) -> &Material {
        &self.default_material
    }

    pub fn add_skeleton(&mut self, name: impl Into<String>, matrices: &[Mat4]) {
        let name = name.into();
        let data = if matrices.is_empty() {
            SkeletonUniform::identity()
        } else {
            SkeletonUniform::from_matrices(matrices)
        };
        let buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("skeleton_buffer"),
            size: std::mem::size_of::<SkeletonUniform>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.queue.write_buffer(&buffer, 0, bytemuck::bytes_of(&data));
        self.skeleton_buffers.insert(name, buffer);
    }

    pub fn update_skeleton(&mut self, name: &str, matrices: &[Mat4]) {
        let Some(buffer) = self.skeleton_buffers.get(name) else {
            return;
        };
        let data = if matrices.is_empty() {
            SkeletonUniform::identity()
        } else {
            SkeletonUniform::from_matrices(matrices)
        };
        self.queue.write_buffer(buffer, 0, bytemuck::bytes_of(&data));
    }

    pub fn resize(&mut self, new_size: winit::dpi::PhysicalSize<u32>) {
        if new_size.width == 0 || new_size.height == 0 {
            return;
        }
        self.size = new_size;
        self.config.width = new_size.width;
        self.config.height = new_size.height;
        self.surface.configure(&self.device, &self.config);

        let ibl_owned = std::mem::replace(&mut self.ibl, None).expect("IBL missing");

        self.sd = build_size_dependent(
            &self.device,
            &self.config,
            &self.bloom_layout,
            &self.tonemap_layout,
            &self.ssao_layout,
            &self.shadow2_layout,
            &self.debug_layout,
            &self.lighting_layout,
            &self.bloom_uniform_h,
            &self.bloom_uniform_v,
            &self.bright_uniform,
            &self.tonemap_uniform,
            &self.ssao_uniform,
            &self.ssao_noise_view,
            &self.csm_array_view,
            &self.csm_sampler,
            &self.cube_shadow_cube_view,
            &self.cube_shadow_sampler,
            &self.camera_buffer,
            &ibl_owned,
        );

        self.ibl = Some(ibl_owned);

        // Пересоздаём CSM debug bind group с новым linear_sampler
        self.csm_debug_bind_group = self.device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("csm_debug_bg"),
            layout: &self.debug_depth_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&self.csm_array_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&self.sd.linear_sampler),
                },
            ],
        });
    }

    fn ensure_instance_capacity(&mut self, needed: u64) {
        if needed <= self.instance_capacity {
            return;
        }
        let new_cap = (self.instance_capacity * 2).max(needed);
        self.instance_buffer = self.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("instance_buffer"),
            size: new_cap * std::mem::size_of::<InstanceData>() as u64,
            usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        self.instance_capacity = new_cap;
    }

    // ============================================================
    // Render
    // ============================================================

    pub fn render(
        &mut self,
        camera: &Camera3D,
        draws: &[MeshDraw],
        line_vertices: &[LineVertex],
        dir_lights: &[GpuLight],
        point_lights: &[GpuPointLight],
        _ambient: [f32; 3],
        postfx: PostFx,
    ) -> Result<(), wgpu::SurfaceError> {
        // === Camera ===
        let view = camera.view_matrix();
        let proj = camera.proj_matrix();
        let vp = proj * view;
        let camera_uniform = CameraUniform {
            view_proj: vp.to_cols_array_2d(),
            inv_view_proj: vp.inverse().to_cols_array_2d(),
            view: view.to_cols_array_2d(),
            inv_view: view.inverse().to_cols_array_2d(),
            camera_pos: camera.position().extend(1.0).to_array(),
            near_far: [camera.near, camera.far, 0.0, 0.0],
        };
        self.queue
            .write_buffer(&self.camera_buffer, 0, bytemuck::bytes_of(&camera_uniform));

        // === Lights ===
        let dir_light_dir = dir_lights
            .first()
            .map(|l| Vec3::new(l.direction[0], l.direction[1], l.direction[2]))
            .unwrap_or(Vec3::Y);

        let mut dir_packed = [[0.0f32; 4]; 8];
        let dir_count = dir_lights.len().min(MAX_DIR_LIGHTS);
        for i in 0..dir_count {
            dir_packed[i * 2] = dir_lights[i].direction;
            dir_packed[i * 2 + 1] = dir_lights[i].color;
        }
        let mut pt_packed = [[0.0f32; 4]; 32];
        let pt_count = point_lights.len().min(MAX_POINT_LIGHTS);
        for i in 0..pt_count {
            pt_packed[i * 2] = point_lights[i].position;
            pt_packed[i * 2 + 1] = point_lights[i].color;
        }
        let cube_count = if pt_count > 0 { 1 } else { 0 };

        let mut cube_pos_packed = [[0.0f32; 4]; 4];
        if pt_count > 0 {
            cube_pos_packed[0] = point_lights[0].position;
        }

        let splits = csm::split_distances(camera.near, camera.far.min(200.0), 0.5);
        let cascade_vp = csm::build_cascades(
            view,
            proj,
            camera.near,
            camera.far,
            dir_light_dir,
            &splits,
        );

        let mut csm_packed = [[[0.0f32; 4]; 4]; CASCADE_COUNT];
        for i in 0..CASCADE_COUNT {
            csm_packed[i] = cascade_vp[i].to_cols_array_2d();
        }

        let ambient_dummy = [0.0f32, 0.0, 0.0, 1.0];

        let lights_uniform = LightsUniform {
            cascade_vp: csm_packed,
            cascade_splits: [splits[1], splits[2], splits[3], 0.0],
            ambient_color: ambient_dummy,
            counts: [dir_count as u32, pt_count as u32, cube_count as u32, 0],
            light_view_proj: cascade_vp[0].to_cols_array_2d(),
            _pad0: [0.0; 4],
            _pad1: [0.0; 4],
            dir_lights: dir_packed,
            point_lights: pt_packed,
            cube_shadow_pos: cube_pos_packed,
        };
        self.queue
            .write_buffer(&self.lights_buffer, 0, bytemuck::bytes_of(&lights_uniform));

        // Shadow pass uniforms — 9 слотов
        {
            let stride = self.shadow_pass_stride as usize;
            let light_size = std::mem::size_of::<LightsUniform>();
            let mut bytes = vec![0u8; stride * SHADOW_SLOT_COUNT as usize];

            let write_slot = |bytes: &mut Vec<u8>, slot: usize, mat: Mat4| {
                let u = LightsUniform {
                    cascade_vp: csm_packed,
                    cascade_splits: [splits[1], splits[2], splits[3], 0.0],
                    ambient_color: ambient_dummy,
                    counts: [dir_count as u32, pt_count as u32, cube_count as u32, 0],
                    light_view_proj: mat.to_cols_array_2d(),
                    _pad0: [0.0; 4],
                    _pad1: [0.0; 4],
                    dir_lights: dir_packed,
                    point_lights: pt_packed,
                    cube_shadow_pos: cube_pos_packed,
                };
                let src = bytemuck::bytes_of(&u);
                let off = slot * stride;
                bytes[off..off + light_size].copy_from_slice(src);
            };

            for c in 0..CASCADE_COUNT {
                write_slot(&mut bytes, c, cascade_vp[c]);
            }

            if pt_count > 0 {
                let light_pos = Vec3::new(
                    point_lights[0].position[0],
                    point_lights[0].position[1],
                    point_lights[0].position[2],
                );
                let range = point_lights[0].position[3];
                let faces = shadow_cube::cube_face_matrices(light_pos, range);
                for (i, m) in faces.iter().enumerate() {
                    write_slot(&mut bytes, 3 + i, *m);
                }
            } else {
                for i in 0..6 {
                    write_slot(&mut bytes, 3 + i, Mat4::IDENTITY);
                }
            }

            self.queue.write_buffer(&self.shadow_pass_buffer, 0, &bytes);
        }

        // === Post uniforms ===
        let bloom_w = self.config.width / 2;
        let bloom_h = self.config.height / 2;
        let texel = [1.0 / bloom_w as f32, 1.0 / bloom_h as f32];

        let bright_params = PostParams { values: [postfx.bloom_threshold, 0.0, 0.0, 0.0] };
        self.queue.write_buffer(&self.bright_uniform, 0, bytemuck::bytes_of(&bright_params));

        let blur_h_params = PostParams { values: [1.0, 0.0, texel[0], texel[1]] };
        self.queue.write_buffer(&self.bloom_uniform_h, 0, bytemuck::bytes_of(&blur_h_params));

        let blur_v_params = PostParams { values: [0.0, 1.0, texel[0], texel[1]] };
        self.queue.write_buffer(&self.bloom_uniform_v, 0, bytemuck::bytes_of(&blur_v_params));

        let tonemap_params = PostParams {
            values: [postfx.bloom_strength, postfx.exposure, 0.0, 0.0],
        };
        self.queue.write_buffer(&self.tonemap_uniform, 0, bytemuck::bytes_of(&tonemap_params));

        let ssao_data = SsaoUniform {
            proj_scale: [proj.x_axis.x, proj.y_axis.y, camera.far, postfx.ssao_radius],
            params: [0.025, postfx.ssao_strength, 1.0 / 4.0, 1.0 / 4.0],
        };
        self.queue.write_buffer(&self.ssao_uniform, 0, bytemuck::bytes_of(&ssao_data));

        let debug_params = DebugParams { mode: [postfx.debug_view as u32, 0, 0, 0] };
        self.queue
            .write_buffer(&self.sd.debug_uniform, 0, bytemuck::bytes_of(&debug_params));

        // === Instances ===
        let total_instances: u64 = draws.iter().map(|d| d.instances.len() as u64).sum();
        if total_instances > 0 {
            self.ensure_instance_capacity(total_instances);
            let stride = std::mem::size_of::<InstanceData>() as u64;
            let mut offset_bytes: u64 = 0;
            for d in draws {
                if d.instances.is_empty() {
                    continue;
                }
                self.queue.write_buffer(
                    &self.instance_buffer,
                    offset_bytes,
                    bytemuck::cast_slice(&d.instances),
                );
                offset_bytes += d.instances.len() as u64 * stride;
            }
        }

        // === Lines ===
        self.line_buffer
            .upload(&self.device, &self.queue, line_vertices);

        // === Frame ===
        let frame = self.surface.get_current_texture()?;
        let swap_view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("encoder"),
            });

        // 1. CSM (3 каскада)
        passes::encode_csm_all(self, &mut encoder, draws);

        // 2. Cube shadow (6 граней)
        if cube_count > 0 {
            passes::encode_cube_shadow_all(self, &mut encoder, draws);
        }

        // 3. G-buffer
        passes::encode_gbuffer_pass(self, &mut encoder, draws);

        // 4. SSAO
        passes::encode_ssao_pass(self, &mut encoder);
        passes::encode_ssao_blur_pass(self, &mut encoder);

        // 5. Lighting (включая sky на фоне)
        passes::encode_lighting_pass(self, &mut encoder);

        // 6. Forward (lines)
        passes::encode_forward_pass(self, &mut encoder, line_vertices);

        // 7. Post-processing или debug view
        if postfx.debug_view.is_debug() {
            passes::encode_debug_pass(self, &mut encoder, &swap_view, postfx.debug_view);
        } else {
            passes::encode_post_processing(self, &mut encoder, &swap_view);
        }

        self.queue.submit(Some(encoder.finish()));
        frame.present();
        Ok(())
    }
}

// ============================================================
// Pipeline builders
// ============================================================

fn make_gbuffer_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    material_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("gbuffer_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/gbuffer.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("gbuffer_pipeline_layout"),
        bind_group_layouts: &[camera_layout, material_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("gbuffer_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[Vertex3D::layout(), InstanceData::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
                Some(wgpu::ColorTargetState {
                    format: GBUFFER_FORMAT,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                }),
            ],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn make_sky_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
    lights_layout: &wgpu::BindGroupLayout,
    shadow2_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("sky_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/sky.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("sky_pipeline_layout"),
        bind_group_layouts: &[camera_layout, lights_layout, shadow2_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("sky_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: false,
            depth_compare: wgpu::CompareFunction::LessEqual,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn make_shadow_pipeline(
    device: &wgpu::Device,
    shadow_pass_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("shadow_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shadow.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("shadow_pipeline_layout"),
        bind_group_layouts: &[shadow_pass_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("shadow_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[Vertex3D::layout(), InstanceData::layout()],
            compilation_options: Default::default(),
        },
        fragment: None,
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::TriangleList,
            front_face: wgpu::FrontFace::Ccw,
            cull_mode: Some(wgpu::Face::Back),
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState {
                constant: 4,
                slope_scale: 2.0,
                clamp: 0.0,
            },
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn make_line_pipeline(
    device: &wgpu::Device,
    camera_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("line_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/lines.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("line_pipeline_layout"),
        bind_group_layouts: &[camera_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("line_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[LineVertex::layout()],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState {
            topology: wgpu::PrimitiveTopology::LineList,
            front_face: wgpu::FrontFace::Ccw,
            ..Default::default()
        },
        depth_stencil: Some(wgpu::DepthStencilState {
            format: DEPTH_FORMAT,
            depth_write_enabled: true,
            depth_compare: wgpu::CompareFunction::Less,
            stencil: wgpu::StencilState::default(),
            bias: wgpu::DepthBiasState::default(),
        }),
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn make_lighting_pipeline(
    device: &wgpu::Device,
    lighting_layout: &wgpu::BindGroupLayout,
    lights_layout: &wgpu::BindGroupLayout,
    shadow2_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("lighting_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/deferred_lighting.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("lighting_pipeline_layout"),
        bind_group_layouts: &[lighting_layout, lights_layout, shadow2_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("lighting_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn make_ssao_pipelines(
    device: &wgpu::Device,
    ssao_layout: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("ssao_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../ssao.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("ssao_pipeline_layout"),
        bind_group_layouts: &[ssao_layout],
        push_constant_ranges: &[],
    });
    let main = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ssao_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_ssao",
            targets: &[Some(wgpu::ColorTargetState {
                format: SSAO_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("ssao_blur_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_blur",
            targets: &[Some(wgpu::ColorTargetState {
                format: SSAO_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    (main, blur)
}

fn make_bloom_pipelines(
    device: &wgpu::Device,
    bloom_layout: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("bloom_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../bloom.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("bloom_pipeline_layout"),
        bind_group_layouts: &[bloom_layout],
        push_constant_ranges: &[],
    });
    let bright = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("bright_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_bright",
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    let blur = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("blur_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_blur",
            targets: &[Some(wgpu::ColorTargetState {
                format: HDR_FORMAT,
                blend: None,
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });
    (bright, blur)
}

fn make_tonemap_pipeline(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    tonemap_layout: &wgpu::BindGroupLayout,
) -> wgpu::RenderPipeline {
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("tonemap_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../tonemap.wgsl").into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("tonemap_pipeline_layout"),
        bind_group_layouts: &[tonemap_layout],
        push_constant_ranges: &[],
    });
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("tonemap_pipeline"),
        layout: Some(&layout),
        vertex: wgpu::VertexState {
            module: &shader,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    })
}

fn make_debug_pipelines(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    debug_layout: &wgpu::BindGroupLayout,
    debug_depth_layout: &wgpu::BindGroupLayout,
) -> (wgpu::RenderPipeline, wgpu::RenderPipeline) {
    let shader2d = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("debug2d_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/debug2d.wgsl").into()),
    });
    let shader_depth = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some("debug_depth_shader"),
        source: wgpu::ShaderSource::Wgsl(include_str!("../shaders/debug_depth.wgsl").into()),
    });

    let layout2d = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("debug2d_layout"),
        bind_group_layouts: &[debug_layout],
        push_constant_ranges: &[],
    });
    let layout_depth = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some("debug_depth_layout"),
        bind_group_layouts: &[debug_depth_layout],
        push_constant_ranges: &[],
    });

    let pipeline2d = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("debug2d_pipeline"),
        layout: Some(&layout2d),
        vertex: wgpu::VertexState {
            module: &shader2d,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader2d,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });

    let pipeline_depth = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some("debug_depth_pipeline"),
        layout: Some(&layout_depth),
        vertex: wgpu::VertexState {
            module: &shader_depth,
            entry_point: "vs_main",
            buffers: &[],
            compilation_options: Default::default(),
        },
        fragment: Some(wgpu::FragmentState {
            module: &shader_depth,
            entry_point: "fs_main",
            targets: &[Some(wgpu::ColorTargetState {
                format: config.format,
                blend: Some(wgpu::BlendState::REPLACE),
                write_mask: wgpu::ColorWrites::ALL,
            })],
            compilation_options: Default::default(),
        }),
        primitive: wgpu::PrimitiveState::default(),
        depth_stencil: None,
        multisample: wgpu::MultisampleState::default(),
        multiview: None,
    });

    (pipeline2d, pipeline_depth)
}