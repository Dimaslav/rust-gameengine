//! Функции кодирования рендер-проходов.

use crate::render::csm::CASCADE_COUNT;
use crate::render::debug::DebugView;
use crate::render::line::LineVertex;
use crate::render::mesh::InstanceData;

use super::gpu_types::*;
use super::Renderer;

// ============================================================
// Приватные helpers
// ============================================================

#[inline]
fn fullscreen_triangle(pass: &mut wgpu::RenderPass<'_>) {
    pass.draw(0..3, 0..1);
}

#[inline]
#[allow(clippy::too_many_arguments)]
fn draw_all_instances<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    r: &'a Renderer,
    draws: &[MeshDraw],
    bind_material_at: Option<u32>,
    include_blend: bool,
    pipeline_single: &'a wgpu::RenderPipeline,
    pipeline_double: &'a wgpu::RenderPipeline,
) {
    let instance_stride = std::mem::size_of::<InstanceData>() as u64;
    let mut offset_bytes: u64 = 0;

    for d in draws {
        if d.instances.is_empty() {
            continue;
        }

        if d.blend != include_blend {
            offset_bytes += d.instances.len() as u64 * instance_stride;
            continue;
        }

        let Some(mesh) = r.meshes.get(&d.mesh) else {
            offset_bytes += d.instances.len() as u64 * instance_stride;
            continue;
        };

        let pipeline = if d.double_sided {
            pipeline_double
        } else {
            pipeline_single
        };
        pass.set_pipeline(pipeline);

        if let Some(slot) = bind_material_at {
            let mat_gpu = d
                .texture
                .as_deref()
                .and_then(|n| r.material_bind_groups.get(n))
                .unwrap_or(&r.default_material_bind_group);
            pass.set_bind_group(slot, &mat_gpu.bind_group, &[]);
        }

        pass.set_vertex_buffer(0, mesh.vertex_buffer.slice(..));
        pass.set_vertex_buffer(1, r.instance_buffer.slice(offset_bytes..));
        pass.set_index_buffer(mesh.index_buffer.slice(..), wgpu::IndexFormat::Uint32);
        pass.draw_indexed(0..mesh.index_count, 0, 0..d.instances.len() as u32);

        offset_bytes += d.instances.len() as u64 * instance_stride;
    }
}

#[inline]
fn clear(color: wgpu::Color) -> wgpu::Operations<wgpu::Color> {
    wgpu::Operations {
        load: wgpu::LoadOp::Clear(color),
        store: wgpu::StoreOp::Store,
    }
}

#[inline]
fn load() -> wgpu::Operations<wgpu::Color> {
    wgpu::Operations {
        load: wgpu::LoadOp::Load,
        store: wgpu::StoreOp::Store,
    }
}

#[inline]
fn depth_clear(value: f32) -> wgpu::Operations<f32> {
    wgpu::Operations {
        load: wgpu::LoadOp::Clear(value),
        store: wgpu::StoreOp::Store,
    }
}

#[inline]
fn depth_load() -> wgpu::Operations<f32> {
    wgpu::Operations {
        load: wgpu::LoadOp::Load,
        store: wgpu::StoreOp::Store,
    }
}

// ============================================================
// Shadow pass
// ============================================================

pub(super) fn encode_shadow_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
    view: &wgpu::TextureView,
    slot_offset: u32,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("shadow_pass"),
        color_attachments: &[],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view,
            depth_ops: Some(depth_clear(1.0)),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_bind_group(0, &r.shadow_pass_bind_group, &[slot_offset]);
    draw_all_instances(
        &mut pass,
        r,
        draws,
        None,
        false,
        &r.shadow_pipeline,
        &r.shadow_pipeline_double_sided,
    );
}

pub(super) fn encode_csm_all(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
) {
    for cascade in 0..CASCADE_COUNT {
        let slot_offset = (cascade as u64 * r.shadow_pass_stride) as u32;
        encode_shadow_pass(r, encoder, draws, &r.csm_cascade_views[cascade], slot_offset);
    }
}

pub(super) fn encode_cube_shadow_all(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
) {
    for face in 0..6u64 {
        let slot_offset = ((3 + face) as u64 * r.shadow_pass_stride) as u32;
        encode_shadow_pass(
            r,
            encoder,
            draws,
            &r.cube_shadow_face_views[face as usize],
            slot_offset,
        );
    }
}

// ============================================================
// G-buffer
// ============================================================

pub(super) fn encode_gbuffer_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("gbuffer_pass"),
        color_attachments: &[
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_albedo_view,
                resolve_target: None,
                ops: clear(wgpu::Color::BLACK),
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_normal_view,
                resolve_target: None,
                ops: clear(wgpu::Color::BLACK),
            }),
            Some(wgpu::RenderPassColorAttachment {
                view: &r.sd.gbuffer_emissive_view,
                resolve_target: None,
                ops: clear(wgpu::Color::BLACK),
            }),
        ],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(depth_clear(1.0)),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_bind_group(0, &r.camera_bind_group, &[]);
    draw_all_instances(
        &mut pass,
        r,
        draws,
        Some(1),
        false,
        &r.gbuffer_pipeline,
        &r.gbuffer_pipeline_double_sided,
    );
}

// ============================================================
// SSAO
// ============================================================

pub(super) fn encode_ssao_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ssao_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.ssao_view,
            resolve_target: None,
            ops: clear(wgpu::Color::WHITE),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.ssao_pipeline);
    pass.set_bind_group(0, &r.sd.ssao_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

pub(super) fn encode_ssao_blur_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("ssao_blur_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.ssao_blur_view,
            resolve_target: None,
            ops: clear(wgpu::Color::WHITE),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.ssao_blur_pipeline);
    pass.set_bind_group(0, &r.sd.ssao_blur_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

// ============================================================
// Lighting
// ============================================================

pub(super) fn encode_lighting_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("lighting_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: clear(wgpu::Color::BLACK),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.lighting_pipeline);
    pass.set_bind_group(0, &r.sd.lighting_bind_group, &[]);
    pass.set_bind_group(1, &r.lights_bind_group, &[]);
    pass.set_bind_group(2, &r.sd.shadow2_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

// ============================================================
// Skybox
// ============================================================

/// Рисует env cubemap на far-plane (depth == 1.0). Depth-write отключён,
/// depth-compare = Equal → пишет только по небу, у которого G-buffer
/// depth = 1.0.
pub(super) fn encode_skybox_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("skybox_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: load(),
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(depth_load()),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.skybox_pipeline);
    pass.set_bind_group(0, &r.skybox_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

// ============================================================
// Forward (lines)
// ============================================================

pub(super) fn encode_forward_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    line_vertices: &[LineVertex],
) {
    if line_vertices.is_empty() && r.line_buffer.vertex_count == 0 {
        return;
    }
    if r.line_buffer.vertex_count == 0 {
        return;
    }

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("forward_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: load(),
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(depth_load()),
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

// ============================================================
// Particles (billboard)
// ============================================================

pub(super) fn encode_particles_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    particle_count: u32,
) {
    if particle_count == 0 {
        return;
    }

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("particles_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: load(),
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(depth_load()),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_pipeline(&r.particles_pipeline);
    pass.set_bind_group(0, &r.camera_bind_group, &[]);
    pass.set_vertex_buffer(0, r.particles_instance_buffer.slice(..));
    // 6 вершин (2 треугольника) на инстанс, N инстансов.
    pass.draw(0..6, 0..particle_count);
}

// ============================================================
// Transparent (forward, blend)
// ============================================================

pub(super) fn encode_transparent_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    draws: &[MeshDraw],
) {
    if !draws.iter().any(|d| d.blend && !d.instances.is_empty()) {
        return;
    }

    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("transparent_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.hdr_view,
            resolve_target: None,
            ops: load(),
        })],
        depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
            view: &r.sd.gbuffer_depth_view,
            depth_ops: Some(depth_load()),
            stencil_ops: None,
        }),
        timestamp_writes: None,
        occlusion_query_set: None,
    });

    pass.set_bind_group(0, &r.camera_bind_group, &[]);
    pass.set_bind_group(1, &r.lights_bind_group, &[]);
    pass.set_bind_group(2, &r.sd.shadow2_bind_group, &[]);
    draw_all_instances(
        &mut pass,
        r,
        draws,
        Some(3),
        true,
        &r.transparent_pipeline,
        &r.transparent_pipeline_double_sided,
    );
}

// ============================================================
// Post-processing
// ============================================================

pub(super) fn encode_bright_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("bright_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.bloom_a_view,
            resolve_target: None,
            ops: clear(wgpu::Color::BLACK),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.bright_pipeline);
    pass.set_bind_group(0, &r.sd.bright_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

pub(super) fn encode_blur_h_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blur_h_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.bloom_b_view,
            resolve_target: None,
            ops: clear(wgpu::Color::BLACK),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.blur_pipeline);
    pass.set_bind_group(0, &r.sd.blur_h_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

pub(super) fn encode_blur_v_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("blur_v_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.bloom_a_view,
            resolve_target: None,
            ops: clear(wgpu::Color::BLACK),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.blur_pipeline);
    pass.set_bind_group(0, &r.sd.blur_v_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

/// Tonemap HDR+bloom → LDR target (не swap).
pub(super) fn encode_composite_pass(r: &Renderer, encoder: &mut wgpu::CommandEncoder) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("composite_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: &r.sd.ldr_view,
            resolve_target: None,
            ops: clear(wgpu::Color::BLACK),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.tonemap_pipeline);
    pass.set_bind_group(0, &r.sd.composite_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

/// FXAA: LDR → swap.
pub(super) fn encode_fxaa_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    swap_view: &wgpu::TextureView,
) {
    let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("fxaa_pass"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: swap_view,
            resolve_target: None,
            ops: clear(wgpu::Color::BLACK),
        })],
        depth_stencil_attachment: None,
        timestamp_writes: None,
        occlusion_query_set: None,
    });
    pass.set_pipeline(&r.fxaa_pipeline);
    pass.set_bind_group(0, &r.sd.fxaa_bind_group, &[]);
    fullscreen_triangle(&mut pass);
}

pub(super) fn encode_post_processing(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    swap_view: &wgpu::TextureView,
) {
    encode_bright_pass(r, encoder);
    encode_blur_h_pass(r, encoder);
    encode_blur_v_pass(r, encoder);
    encode_composite_pass(r, encoder);
    encode_fxaa_pass(r, encoder, swap_view);
}

// ============================================================
// Debug
// ============================================================

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
            ops: clear(wgpu::Color::BLACK),
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
    fullscreen_triangle(&mut pass);
}