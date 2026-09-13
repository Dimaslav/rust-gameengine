//! Функции кодирования рендер-проходов.
//!
//! Deferred pipeline:
//!   1. Shadow (3 CSM + 6 cube)      — encode_csm_all / encode_cube_shadow_all
//!   2. G-buffer (3 MRT + depth)     — encode_gbuffer_pass
//!   3. SSAO + blur                  — encode_ssao_pass / encode_ssao_blur_pass
//!   4. Lighting (fullscreen)        — encode_lighting_pass
//!   5. Forward (lines)              — encode_forward_pass
//!   6. Post-processing              — encode_post_processing
//!   7. Debug (F1–F6)                — encode_debug_pass

use crate::render::csm::CASCADE_COUNT;
use crate::render::debug::DebugView;
use crate::render::line::LineVertex;
use crate::render::mesh::InstanceData;

use super::gpu_types::*;
use super::Renderer;

// ============================================================
// Приватные helpers
// ============================================================

/// Fullscreen triangle: одна треугольная «простыня» на весь экран.
#[inline]
fn fullscreen_triangle(pass: &mut wgpu::RenderPass<'_>) {
    pass.draw(0..3, 0..1);
}

/// Пробегает по всем MeshDraw и рисует их инстансами.
/// `bind_material_at` — какой bind group slot занимает material.
/// `None` — material не биндится (shadow pass).
#[inline]
fn draw_all_instances<'a>(
    pass: &mut wgpu::RenderPass<'a>,
    r: &'a Renderer,
    draws: &[MeshDraw],
    bind_material_at: Option<u32>,
) {
    let instance_stride = std::mem::size_of::<InstanceData>() as u64;
    let mut offset_bytes: u64 = 0;

    for d in draws {
        if d.instances.is_empty() {
            continue;
        }
        let Some(mesh) = r.meshes.get(&d.mesh) else {
            continue;
        };

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

/// Рендерит один слой shadow map. Используется и для CSM, и для cube shadow.
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

    pass.set_pipeline(&r.shadow_pipeline);
    pass.set_bind_group(0, &r.shadow_pass_bind_group, &[slot_offset]);
    draw_all_instances(&mut pass, r, draws, None);
}

/// Прогон всех 3 CSM каскадов.
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

/// Прогон всех 6 граней cube shadow.
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

/// G-buffer: 3 MRT + depth.
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

    pass.set_pipeline(&r.gbuffer_pipeline);
    pass.set_bind_group(0, &r.camera_bind_group, &[]);
    draw_all_instances(&mut pass, r, draws, Some(1));
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
// Forward
// ============================================================

/// Forward pass: только lines поверх HDR.
pub(super) fn encode_forward_pass(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    line_vertices: &[LineVertex],
) {
    // Если у нас нет ни линий, ни vertex_count в буфере — рано выходим.
    if line_vertices.is_empty() && r.line_buffer.vertex_count == 0 {
        return;
    }
    // Если vertices были переданы — буфер уже загружен в render().
    // Если нет, но vertex_count > 0 — значит уже загружен ранее.
    // Защита: если буфер не заполнен, выходим.
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

/// Вся постобработка одним вызовом.
pub(super) fn encode_post_processing(
    r: &Renderer,
    encoder: &mut wgpu::CommandEncoder,
    swap_view: &wgpu::TextureView,
) {
    encode_bright_pass(r, encoder);
    encode_blur_h_pass(r, encoder);
    encode_blur_v_pass(r, encoder);
    encode_composite_pass(r, encoder, swap_view);
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