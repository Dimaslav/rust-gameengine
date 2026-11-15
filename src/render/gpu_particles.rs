use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::engine::particles::BurstParams;

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;

pub const MAX_GPU_PARTICLES: u32 = 50_000;

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
pub struct GpuParticle {
    pub pos: [f32; 4],
    pub vel: [f32; 4],
    pub color_a: [f32; 4],
    pub color_b: [f32; 4],
    pub attrs: [f32; 4],
}

#[repr(C)]
#[derive(Copy, Clone, Debug, Pod, Zeroable)]
struct SimParams {
    dt: f32,
    time: f32,
    _pad0: f32,
    _pad1: f32,
}

pub struct GpuParticleSystem {
    pub buffer: wgpu::Buffer,
    pub compute_pipeline: wgpu::ComputePipeline,
    pub compute_bind_group: wgpu::BindGroup,
    pub render_pipeline: wgpu::RenderPipeline,
    pub render_bind_group: wgpu::BindGroup,
    sim_params_buffer: wgpu::Buffer,
    write_cursor: u32,
    pending_writes: Vec<(u32, GpuParticle)>,
    seed: u32,
}

impl GpuParticleSystem {
    pub fn new(device: &wgpu::Device, camera_layout: &wgpu::BindGroupLayout) -> Self {
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_particles_buffer"),
            size: (MAX_GPU_PARTICLES as u64) * std::mem::size_of::<GpuParticle>() as u64,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let sim_params_buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("gpu_particles_sim_params"),
            size: std::mem::size_of::<SimParams>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });

        let compute_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_particles_compute_bgl"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Storage { read_only: false },
                        has_dynamic_offset: false,
                        min_binding_size: None,
                    },
                    count: None,
                },
            ],
        });

        let compute_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_particles_compute_bg"),
            layout: &compute_bgl,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: sim_params_buffer.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: buffer.as_entire_binding(),
                },
            ],
        });

        let compute_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu_particles_sim_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/particles_sim.wgsl").into(),
            ),
        });

        let compute_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gpu_particles_compute_layout"),
                bind_group_layouts: &[Some(&compute_bgl)],
                immediate_size: 0,
            });

        let compute_pipeline =
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("gpu_particles_compute"),
                layout: Some(&compute_pipeline_layout),
                module: &compute_shader,
                entry_point: Some("cs_main"),
                compilation_options: Default::default(),
                cache: None,
            });

        // === Render bind group (group 1: storage read_only) ===
        let render_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("gpu_particles_render_bgl"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 0,
                visibility: wgpu::ShaderStages::VERTEX,
                ty: wgpu::BindingType::Buffer {
                    ty: wgpu::BufferBindingType::Storage { read_only: true },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            }],
        });

        let render_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("gpu_particles_render_bg"),
            layout: &render_bgl,
            entries: &[wgpu::BindGroupEntry {
                binding: 0,
                resource: buffer.as_entire_binding(),
            }],
        });

        let render_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu_particles_render_shader"),
            source: wgpu::ShaderSource::Wgsl(
                include_str!("shaders/particles_gpu.wgsl").into(),
            ),
        });

        let render_pipeline_layout =
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("gpu_particles_render_layout"),
                bind_group_layouts: &[Some(camera_layout), Some(&render_bgl)],
                immediate_size: 0,
            });

        let render_pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("gpu_particles_render"),
            layout: Some(&render_pipeline_layout),
            vertex: wgpu::VertexState {
                module: &render_shader,
                entry_point: Some("vs_main"),
                buffers: &[], // ← никаких vertex buffers, только storage
                compilation_options: Default::default(),
            },
            fragment: Some(wgpu::FragmentState {
                module: &render_shader,
                entry_point: Some("fs_main"),
                targets: &[Some(wgpu::ColorTargetState {
                    format: HDR_FORMAT,
                    blend: Some(wgpu::BlendState::ALPHA_BLENDING),
                    write_mask: wgpu::ColorWrites::ALL,
                })],
                compilation_options: Default::default(),
            }),
            primitive: wgpu::PrimitiveState {
                topology: wgpu::PrimitiveTopology::TriangleList,
                front_face: wgpu::FrontFace::Ccw,
                cull_mode: None,
                ..Default::default()
            },
            depth_stencil: Some(wgpu::DepthStencilState {
                format: DEPTH_FORMAT,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Less),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        });

        Self {
            buffer,
            compute_pipeline,
            compute_bind_group,
            render_pipeline,
            render_bind_group,
            sim_params_buffer,
            write_cursor: 0,
            pending_writes: Vec::with_capacity(1024),
            seed: 0xC0FFEE,
        }
    }

    pub fn spawn_burst(&mut self, origin: Vec3, dir: Vec3, params: &BurstParams) {
        use std::f32::consts::TAU;

        let dir_n = dir.normalize_or_zero();
        let dir_n = if dir_n.length_squared() < 1e-6 { Vec3::Y } else { dir_n };

        for _ in 0..params.count {
            let r1 = self.rand();
            let r2 = self.rand();
            let r3 = self.rand();
            let r4 = self.rand();

            let cos_spread = params.spread.cos();
            let cos_theta = 1.0 - r1 * (1.0 - cos_spread);
            let sin_theta = (1.0 - cos_theta * cos_theta).sqrt();
            let phi = r2 * TAU;

            let tangent = if dir_n.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
            let t1 = tangent.cross(dir_n).normalize_or_zero();
            let t2 = dir_n.cross(t1).normalize_or_zero();

            let local = Vec3::new(
                sin_theta * phi.cos(),
                sin_theta * phi.sin(),
                cos_theta,
            );
            let sample =
                (t1 * local.x + t2 * local.y + dir_n * local.z).normalize_or_zero();

            let speed = params.speed * (0.7 + r3 * 0.6);
            let lifetime = params.lifetime
                * (1.0 + (r4 * 2.0 - 1.0) * params.lifetime_variation);

            let p = GpuParticle {
                pos: [origin.x, origin.y, origin.z, 0.0],
                vel: [
                    sample.x * speed,
                    sample.y * speed,
                    sample.z * speed,
                    lifetime.max(0.05),
                ],
                color_a: params.color_start,
                color_b: params.color_end,
                attrs: [
                    params.size,
                    params.size * params.size_end_scale,
                    params.gravity,
                    0.5,
                ],
            };

            self.write_cursor = (self.write_cursor + 1) % MAX_GPU_PARTICLES;
            self.pending_writes.push((self.write_cursor, p));
        }
    }

    pub fn spawn(&mut self, particle: GpuParticle) {
        self.write_cursor = (self.write_cursor + 1) % MAX_GPU_PARTICLES;
        self.pending_writes.push((self.write_cursor, particle));
    }

    pub fn pending_count(&self) -> usize {
        self.pending_writes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.pending_writes.is_empty()
    }

    pub fn dispatch(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        dt: f32,
        time: f32,
    ) {
        if !self.pending_writes.is_empty() {
            let stride = std::mem::size_of::<GpuParticle>() as u64;
            for (idx, p) in self.pending_writes.drain(..) {
                let offset = idx as u64 * stride;
                queue.write_buffer(&self.buffer, offset, bytemuck::bytes_of(&p));
            }
        }

        let params = SimParams {
            dt: dt.clamp(0.0, 0.1),
            time,
            _pad0: 0.0,
            _pad1: 0.0,
        };
        queue.write_buffer(&self.sim_params_buffer, 0, bytemuck::bytes_of(&params));

        let mut cpass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: Some("gpu_particles_sim_pass"),
            timestamp_writes: None,
        });
        cpass.set_pipeline(&self.compute_pipeline);
        cpass.set_bind_group(0, &self.compute_bind_group, &[]);
        cpass.dispatch_workgroups((MAX_GPU_PARTICLES + 255) / 256, 1, 1);
    }

    pub fn draw<'a>(
        &'a self,
        pass: &mut wgpu::RenderPass<'a>,
        camera_bind_group: &'a wgpu::BindGroup,
    ) {
        pass.set_pipeline(&self.render_pipeline);
        pass.set_bind_group(0, camera_bind_group, &[]);
        pass.set_bind_group(1, &self.render_bind_group, &[]);
        pass.draw(0..6, 0..MAX_GPU_PARTICLES);
    }

    fn rand(&mut self) -> f32 {
        self.seed = self.seed.wrapping_mul(1664525).wrapping_add(1013904223);
        ((self.seed >> 8) & 0xFFFFFF) as f32 / 16777215.0
    }
}