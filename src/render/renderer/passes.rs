//! Функции кодирования рендер-проходов.
//!
//! Deferred pipeline (14B):
//!   1. CSM (3 pass)
//!   2. Cube shadow (6 pass)
//!   3. G-buffer (3 MRT + depth_gray + depth)
//!   4. SSAO + blur
//!   5. Lighting (fullscreen, из G-buffer + sky на фоне)
//!   6. Forward (только lines)
//!   7. Bloom / Tonemap / Debug

use crate::render::csm::CASCADE_COUNT;
use crate::render::debug::DebugView;
use crate::render::mesh::InstanceData;

use super::gpu_types::*;
use super::Renderer;

pub(super) fn encode_csm_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
    cascade: usize,
    slot_offset: u32,
) {
    let instance_stride = std::mem::size_of::<InstanceData>() as u64;
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("csm_pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.csm_cascade_views[cascade],
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.shadow_pipeline);
    pass.set_bind_group(0, &r.shadow_pass_bind_group, &[slot_offset]);

    let mut offset_bytes: u64 = 0;
    for d in draws {
        if d.instances.is_empty() { continue; }
        let Some(mesh) = r.meshes.get(&d.mesh) else { continue };
        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, r.instance_buffer.slice(offset_bytes..));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..d.instances.len() as u32);
        offset_bytes += d.instances.len() as u64 * instance_stride;
    }
}

pub(super) fn encode_cube_shadow_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
    face: usize,
    slot_offset: u32,
) {
    let instance_stride = std::mem::size_of::<InstanceData>() as u64;
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("cube_shadow_pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.cube_shadow_face_views[face],
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.shadow_pipeline);
    pass.set_bind_group(0, &r.shadow_pass_bind_group, &[slot_offset]);

    let mut offset_bytes: u64 = 0;
    for d in draws {
        if d.instances.is_empty() { continue; }
        let Some(mesh) = r.meshes.get(&d.mesh) else { continue };
        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, r.instance_buffer.slice(offset_bytes..));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..d.instances.len() as u32);
        offset_bytes += d.instances.len() as u64 * instance_stride;
    }
}

/// G-buffer: пишет 3 MRT + depth_gray + depth.
pub(super) fn encode_gbuffer_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
) {
    let instance_stride = std::mem::size_of::<InstanceData>() as u64;
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("gbuffer_pass"),
        color_attachments: &[
            // 0: albedo + metallic
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_albedo_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            }),
            // 1: world normal + roughness
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_normal_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            }),
            // 2: emissive
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_emissive_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            }),
            // 3: view_depth / far (для SSAO)
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_depth_gray_view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color { r: 1.0, g: 0.0, b: 0.0, a: 1.0 }),
                    store: wgpu::StoreOp::Store,
                },
            }),
        ],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Clear(1.0),
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.gbuffer_pipeline);
    pass.set_bind_group(0, &r.camera_bind_group, &[]);

    let mut offset_bytes: u64 = 0;
    for d in draws {
        if d.instances.is_empty() { continue; }
        let Some(mesh) = r.meshes.get(&d.mesh) else { continue };

        let mat_gpu = d
            .texture
            .as_deref()
            .and_then(|n| r.material_bind_groups.get(n))
            .unwrap_or(&r.default_material_bind_group);

        pass.set_bind_group(1, &mat_gpu.bind_group, &[]);
        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, r.instance_buffer.slice(offset_bytes..));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..d.instances.len() as u32);
        offset_bytes += d.instances.len() as u64 * instance_stride;
    }
}

pub(super) fn encode_ssao_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ssao_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.ssao_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.ssao_pipeline);
    pass.set_bind_group(0, &r.sd.ssao_bind_group, &[]);
    pass.draw(0..3, 0..1);
}

pub(super) fn encode_ssao_blur_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ssao_blur_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.ssao_blur_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::WHITE),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.ssao_blur_pipeline);
    pass.set_bind_group(0, &r.sd.ssao_blur_bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Fullscreen lighting pass: читает G-buffer + IBL + тени + AO.
/// На фоне (depth == 1.0) рисует sky из HDRI.
pub(super) fn encode_lighting_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("lighting_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.lighting_pipeline);
    pass.set_bind_group(0, &r.sd.lighting_bind_group, &[]);
    pass.set_bind_group(1, &r.lights_bind_group, &[]);
    pass.set_bind_group(2, &r.sd.shadow2_bind_group, &[]);
    pass.draw(0..3, 0..1);
}

/// Forward pass: только lines поверх HDR.
/// Sky теперь рисуется в lighting pass (по depth == 1.0).
pub(super) fn encode_forward_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    if r.line_buffer.vertex_count == 0 {
        return;
    }

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("forward_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(wgpu::Operations {
                load: wgpu::LoadOp::Load,
                store: wgpu::StoreOp::Store,
            }),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.line_pipeline);
    pass.set_bind_group(0, &r.camera_bind_group, &[]);
    pass.set_vertex_buffer(0, r.line_buffer.buffer.slice(..));
    pass.draw(0..r.line_buffer.vertex_count, 0..1);
}

pub(super) fn encode_bright_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("bright_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.bloom_a_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.bright_pipeline);
    pass.set_bind_group(0, &r.sd.bright_bind_group, &[]);
    pass.draw(0..3, 0..1);
}

pub(super) fn encode_blur_h_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blur_h_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.bloom_b_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.blur_pipeline);
    pass.set_bind_group(0, &r.sd.blur_h_bind_group, &[]);
    pass.draw(0..3, 0..1);
}

pub(super) fn encode_blur_v_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blur_v_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.bloom_a_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.blur_pipeline);
    pass.set_bind_group(0, &r.sd.blur_v_bind_group, &[]);
    pass.draw(0..3, 0..1);
}

pub(super) fn encode_debug_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    swap_view: &wgpu::TextureView,
    debug_view: DebugView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("debug_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: swap_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    match debug_view {
        DebugView::Ssao => {
            pass.set_pipeline(&r.debug2d_pipeline);
            pass.set_bind_group(0, &r.sd.debug_bind_ssao, &[]);
        }
        DebugView::GbufferNormal => {
            pass.set_pipeline(&r.debug2d_pipeline);
            pass.set_bind_group(0, &r.sd.debug_bind_gbuffer, &[]);
        }
        DebugView::GbufferDepth => {
            pass.set_pipeline(&r.debug2d_pipeline);
            pass.set_bind_group(0, &r.sd.debug_bind_depth, &[]);
        }
        DebugView::HdrPreBloom => {
            pass.set_pipeline(&r.debug2d_pipeline);
            pass.set_bind_group(0, &r.sd.debug_bind_hdr, &[]);
        }
        DebugView::CsmCascade0 => {
            pass.set_pipeline(&r.debug_depth_pipeline);
            pass.set_bind_group(0, &r.csm_debug_bind_group, &[]);
        }
        DebugView::Final => unreachable!(),
    }
    pass.draw(0..3, 0..1);
}

pub(super) fn encode_composite_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    swap_view: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("composite_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: swap_view,
            resolve_target: None,
            ops: wgpu::Operations {
                load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                store: wgpu::StoreOp::Store,
            },
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.tonemap_pipeline);
    pass.set_bind_group(0, &r.sd.composite_bind_group, &[]);
    pass.draw(0..3, 0..1);
}