use glam::{Mat4, Vec3};

use super::gpu_types::*;
use crate::render::InstanceData;
use super::Renderer;

pub const MAX_REFLECTION_PROBES: usize = 4;
pub const PROBE_FACE_SIZE: u32 = 256;

pub struct ProbeCubemap {
    pub texture: wgpu::Texture,
    pub cube_view: wgpu::TextureView,
    /// 6 face-views для рендера каждой грани отдельно.
    pub face_views: [wgpu::TextureView; 6],
    pub sampler: wgpu::Sampler,
    pub baked: bool,
    /// World-space AABB (для falloff в шейдере).
    pub box_min: Vec3,
    pub box_max: Vec3,
}

impl ProbeCubemap {
    pub fn new(device: &wgpu::Device, label: &str) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width: PROBE_FACE_SIZE,
                height: PROBE_FACE_SIZE,
                depth_or_array_layers: 6,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });

        let face_views: [wgpu::TextureView; 6] = std::array::from_fn(|i| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                dimension: Some(wgpu::TextureViewDimension::D2),
                base_array_layer: i as u32,
                array_layer_count: Some(1),
                ..Default::default()
            })
        });
        let cube_view = texture.create_view(&wgpu::TextureViewDescriptor {
            dimension: Some(wgpu::TextureViewDimension::Cube),
            base_array_layer: 0,
            array_layer_count: Some(6),
            ..Default::default()
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("probe_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            ..Default::default()
        });

        Self {
            texture,
            cube_view,
            face_views,
            sampler,
            baked: false,
            box_min: Vec3::splat(-50.0),
            box_max: Vec3::splat(50.0),
        }
    }
}

pub fn cube_face_view_projs(pos: Vec3, near: f32, far: f32) -> [Mat4; 6] {
    let proj = crate::render::camera::perspective_rh(
        std::f32::consts::FRAC_PI_2,
        1.0,
        near,
        far,
    );
    let faces = [
        (Vec3::X, Vec3::NEG_Y),
        (Vec3::NEG_X, Vec3::NEG_Y),
        (Vec3::Y, Vec3::Z),
        (Vec3::NEG_Y, Vec3::NEG_Z),
        (Vec3::Z, Vec3::NEG_Y),
        (Vec3::NEG_Z, Vec3::NEG_Y),
    ];
    std::array::from_fn(|i| {
        let (forward, up) = faces[i];
        let view = crate::render::camera::look_at_rh(pos, pos + forward, up);
        proj * view
    })
}

#[repr(C)]
#[derive(Copy, Clone, bytemuck::Pod, bytemuck::Zeroable)]
pub struct ProbeBakeUniform {
    pub view_proj: [[f32; 4]; 4],
    pub probe_pos: [f32; 4],
    pub box_min: [f32; 4],
    pub box_max: [f32; 4],
}

#[allow(clippy::too_many_arguments)]
pub fn bake_probe(
    r: &mut Renderer,
    probe_idx: usize,
    draws: &[MeshDraw],
    probe_pos: Vec3,
    box_min: Vec3,
    box_max: Vec3,
) {
    if probe_idx >= r.probes.len() { return; }

    let total_instances: u64 = draws.iter().map(|d| d.instances.len() as u64).sum();
    if total_instances > 0 {
        r.ensure_instance_capacity(total_instances);
        let stride = std::mem::size_of::<InstanceData>() as u64;
        let mut offset_bytes: u64 = 0;
        for d in draws {
            if d.instances.is_empty() { continue; }
            r.queue.write_buffer(
                &r.instance_buffer,
                offset_bytes,
                bytemuck::cast_slice(&d.instances),
            );
            offset_bytes += d.instances.len() as u64 * stride;
        }
    }

    {
        let probe = &mut r.probes[probe_idx];
        probe.box_min = box_min;
        probe.box_max = box_max;
    }

    let face_vps = cube_face_view_projs(probe_pos, 0.1, 100.0);

    for face in 0..6 {
        let u = ProbeBakeUniform {
            view_proj: face_vps[face].to_cols_array_2d(),
            probe_pos: [probe_pos.x, probe_pos.y, probe_pos.z, 0.0],
            box_min: [box_min.x, box_min.y, box_min.z, 0.0],
            box_max: [box_max.x, box_max.y, box_max.z, 0.0],
        };
        r.queue.write_buffer(&r.probe_bake_uniform, 0, bytemuck::bytes_of(&u));

        let cam_uniform = CameraUniform {
            view_proj: face_vps[face].to_cols_array_2d(),
            inv_view_proj: face_vps[face].inverse().to_cols_array_2d(),
            view: Mat4::IDENTITY.to_cols_array_2d(),
            inv_view: Mat4::IDENTITY.to_cols_array_2d(),
            camera_pos: probe_pos.extend(1.0).to_array(),
            near_far: [0.1, 100.0, 0.0, 0.0],
            prev_view_proj: face_vps[face].to_cols_array_2d(),
            screen_size: [
                PROBE_FACE_SIZE as f32,
                PROBE_FACE_SIZE as f32,
                1.0 / PROBE_FACE_SIZE as f32,
                1.0 / PROBE_FACE_SIZE as f32,
            ],
        };
        r.queue.write_buffer(&r.camera_buffer, 0, bytemuck::bytes_of(&cam_uniform));

        let mut encoder = r.device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("probe_bake_encoder"),
        });

        {
            let probe = &r.probes[probe_idx];
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("probe_face_pass"),
                // FIX: два color attachment'а — HDR (cubemap face) и
                // dummy MOTION (в pipeline write_mask: empty, реально
                // не пишется, StoreOp::Discard — не сохраняется в память).
                color_attachments: &[
                    Some(wgpu::RenderPassColorAttachment {
                        view: &probe.face_views[face],
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color {
                                r: 0.05, g: 0.05, b: 0.08, a: 1.0,
                            }),
                            store: wgpu::StoreOp::Store,
                        },
                        depth_slice: None,
                    }),
                    Some(wgpu::RenderPassColorAttachment {
                        view: &r.probe_bake_motion_view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                            store: wgpu::StoreOp::Discard,
                        },
                        depth_slice: None,
                    }),
                ],
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: &r.probe_bake_depth_view,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(1.0),
                        store: wgpu::StoreOp::Store,
                    }),
                    stencil_ops: None,
                }),
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });

            pass.set_pipeline(&r.probe_bake_pipeline);
            pass.set_bind_group(0, &r.camera_bind_group, &[]);
            pass.set_bind_group(1, &r.lights_bind_group, &[]);
            pass.set_bind_group(2, &r.sd.shadow2_bind_group, &[]);

            let instance_stride = std::mem::size_of::<InstanceData>() as u64;
            let mut offset_bytes: u64 = 0;
            for d in draws {
                if d.instances.is_empty() || d.blend {
                    offset_bytes += d.instances.len() as u64 * instance_stride;
                    continue;
                }
                let Some(mesh) = r.meshes.get(&d.mesh) else {
                    offset_bytes += d.instances.len() as u64 * instance_stride;
                    continue;
                };
                if let Some(bg) = d
                    .texture
                    .as_deref()
                    .and_then(|n| r.material_bind_groups.get(n))
                {
                    pass.set_bind_group(3, &bg.bind_group, &[]);
                } else {
                    pass.set_bind_group(3, &r.default_material_bind_group.bind_group, &[]);
                }
                pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
                pass.set_vertex_buffer(1, r.instance_buffer.slice(offset_bytes..));
                pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..mesh.index_count, 0, 0..d.instances.len() as u32);

                offset_bytes += d.instances.len() as u64 * instance_stride;
            }
        }

        r.queue.submit(Some(encoder.finish()));
    }

    r.probes[probe_idx].baked = true;
    log::info!(
        "Probe {} baked at ({:.1}, {:.1}, {:.1})",
        probe_idx,
        probe_pos.x,
        probe_pos.y,
        probe_pos.z
    );
}

pub fn update_lighting_uniform(
    queue: &wgpu::Queue,
    uniform_buffer: &wgpu::Buffer,
    probes: &[ProbeCubemap],
) {
    let mut data: ProbeLightingUniform = bytemuck::Zeroable::zeroed();
    
    // Считаем количество активных проб ДО цикла, чтобы не ловить
    // warning unused_assignments от компилятора.
    let active_count = probes.iter().filter(|p| p.baked).count() as u32;

    for (i, p) in probes.iter().enumerate().take(MAX_REFLECTION_PROBES) {
        if !p.baked {
            continue;
        }
        let center = (p.box_min + p.box_max) * 0.5;
        data.probes[i] = ProbeInfo {
            center_enabled: [center.x, center.y, center.z, 1.0],
            box_min: [p.box_min.x, p.box_min.y, p.box_min.z, 0.0],
            box_max: [p.box_max.x, p.box_max.y, p.box_max.z, 0.0],
            params: [1.0, 8.0, 1.0, 0.0],
        };
    }
    
    data.counts = [active_count, 0, 0, 0];
    queue.write_buffer(uniform_buffer, 0, bytemuck::bytes_of(&data));
}