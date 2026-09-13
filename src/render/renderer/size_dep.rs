//! Ресурсы, зависящие от размера окна.
//!
//! 14C-2: убран `gbuffer_depth_gray` — depth упакован в `gbuffer_normal.a`.
//! SSAO читает normal из `.rgb` и depth из `.a` одной текстуры.

use crate::render::ibl::IblResources;

use super::gpu_types::*;

pub struct SizeDependent {
    pub hdr_view: wgpu::TextureView,

    pub bloom_a_view: wgpu::TextureView,
    pub bloom_b_view: wgpu::TextureView,
    pub linear_sampler: wgpu::Sampler,

    pub bright_bind_group: wgpu::BindGroup,
    pub blur_h_bind_group: wgpu::BindGroup,
    pub blur_v_bind_group: wgpu::BindGroup,
    pub composite_bind_group: wgpu::BindGroup,

    // G-buffer: 3 MRT + depth
    pub gbuffer_albedo_view: wgpu::TextureView,
    pub gbuffer_normal_view: wgpu::TextureView,
    pub gbuffer_emissive_view: wgpu::TextureView,
    pub gbuffer_depth_view: wgpu::TextureView,

    pub ssao_view: wgpu::TextureView,
    pub ssao_blur_view: wgpu::TextureView,
    pub ssao_bind_group: wgpu::BindGroup,
    pub ssao_blur_bind_group: wgpu::BindGroup,

    pub shadow2_bind_group: wgpu::BindGroup,
    pub lighting_bind_group: wgpu::BindGroup,

    pub debug_uniform: wgpu::Buffer,
    pub debug_bind_ssao: wgpu::BindGroup,
    pub debug_bind_gbuffer: wgpu::BindGroup,
    pub debug_bind_depth: wgpu::BindGroup,
    pub debug_bind_hdr: wgpu::BindGroup,
}

#[allow(clippy::too_many_arguments)]
pub fn build_size_dependent(
    device: &wgpu::Device,
    config: &wgpu::SurfaceConfiguration,
    bloom_layout: &wgpu::BindGroupLayout,
    tonemap_layout: &wgpu::BindGroupLayout,
    ssao_layout: &wgpu::BindGroupLayout,
    shadow2_layout: &wgpu::BindGroupLayout,
    debug_layout: &wgpu::BindGroupLayout,
    lighting_layout: &wgpu::BindGroupLayout,
    bloom_uniform_h: &wgpu::Buffer,
    bloom_uniform_v: &wgpu::Buffer,
    bright_uniform: &wgpu::Buffer,
    tonemap_uniform: &wgpu::Buffer,
    ssao_uniform: &wgpu::Buffer,
    noise_view: &wgpu::TextureView,
    csm_array_view: &wgpu::TextureView,
    csm_sampler: &wgpu::Sampler,
    cube_shadow_cube_view: &wgpu::TextureView,
    cube_shadow_sampler: &wgpu::Sampler,
    camera_buffer: &wgpu::Buffer,
    ibl: &IblResources,
) -> SizeDependent {
    let w = config.width.max(1);
    let h = config.height.max(1);
    let bw = (w / 2).max(1);
    let bh = (h / 2).max(1);

    let hdr_view = create_color_target(device, "hdr", w, h, HDR_FORMAT, 1, true);
    let bloom_a_view = create_color_target(device, "bloom_a", bw, bh, HDR_FORMAT, 1, true);
    let bloom_b_view = create_color_target(device, "bloom_b", bw, bh, HDR_FORMAT, 1, true);
    let linear_sampler = create_linear_sampler(device, "post_linear");

    // G-buffer: 3 MRT + depth
    let gbuffer_albedo_view =
        create_color_target(device, "gbuffer_albedo", w, h, GBUFFER_FORMAT, 1, true);
    let gbuffer_normal_view =
        create_color_target(device, "gbuffer_normal", w, h, GBUFFER_FORMAT, 1, true);
    let gbuffer_emissive_view =
        create_color_target(device, "gbuffer_emissive", w, h, GBUFFER_FORMAT, 1, true);
    let gbuffer_depth_view = create_depth_view(device, w, h, 1);

    // SSAO
    let ssao_view = create_color_target(device, "ssao", w, h, SSAO_FORMAT, 1, true);
    let ssao_blur_view = create_color_target(device, "ssao_blur", w, h, SSAO_FORMAT, 1, true);

    // SSAO читает normal+depth из gbuffer_normal (rgb=normal, a=depth_norm).
    let ssao_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ssao_bind_group"),
        layout: ssao_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&gbuffer_normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(noise_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: ssao_uniform.as_entire_binding(),
            },
        ],
    });

    let ssao_blur_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("ssao_blur_bind_group"),
        layout: ssao_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&ssao_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(noise_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: ssao_uniform.as_entire_binding(),
            },
        ],
    });

    let shadow2_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("shadow2_bind_group"),
        layout: shadow2_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(csm_array_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(csm_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(cube_shadow_cube_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::Sampler(cube_shadow_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&ssao_blur_view),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 6,
                resource: wgpu::BindingResource::TextureView(&ibl.irradiance_view),
            },
            wgpu::BindGroupEntry {
                binding: 7,
                resource: wgpu::BindingResource::TextureView(&ibl.prefiltered_view),
            },
            wgpu::BindGroupEntry {
                binding: 8,
                resource: wgpu::BindingResource::TextureView(&ibl.brdf_lut_view),
            },
            wgpu::BindGroupEntry {
                binding: 9,
                resource: wgpu::BindingResource::Sampler(&ibl.env_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 10,
                resource: wgpu::BindingResource::TextureView(&ibl.env_cube_view),
            },
        ],
    });

    let lighting_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("lighting_bind_group"),
        layout: lighting_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&gbuffer_albedo_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&gbuffer_normal_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::TextureView(&gbuffer_emissive_view),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: wgpu::BindingResource::TextureView(&gbuffer_depth_view),
            },
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 5,
                resource: camera_buffer.as_entire_binding(),
            },
        ],
    });

    let bright_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("bright_bind_group"),
        layout: bloom_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&hdr_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: bright_uniform.as_entire_binding(),
            },
        ],
    });

    let blur_h_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur_h_bind_group"),
        layout: bloom_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&bloom_a_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: bloom_uniform_h.as_entire_binding(),
            },
        ],
    });

    let blur_v_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("blur_v_bind_group"),
        layout: bloom_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&bloom_b_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: bloom_uniform_v.as_entire_binding(),
            },
        ],
    });

    let composite_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("composite_bind_group"),
        layout: tonemap_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&hdr_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::TextureView(&bloom_a_view),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 3,
                resource: tonemap_uniform.as_entire_binding(),
            },
        ],
    });

    let debug_uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("debug_uniform"),
        size: std::mem::size_of::<DebugParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let make_debug_bg = |name: &str, view: &wgpu::TextureView| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some(name),
            layout: debug_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: debug_uniform.as_entire_binding(),
                },
            ],
        })
    };

    let debug_bind_ssao = make_debug_bg("debug_bg_ssao", &ssao_blur_view);
    let debug_bind_gbuffer = make_debug_bg("debug_bg_gbuffer", &gbuffer_normal_view);
    let debug_bind_depth = make_debug_bg("debug_bg_depth", &gbuffer_normal_view);
    let debug_bind_hdr = make_debug_bg("debug_bg_hdr", &hdr_view);

    SizeDependent {
        hdr_view,
        bloom_a_view, bloom_b_view, linear_sampler,
        bright_bind_group, blur_h_bind_group, blur_v_bind_group, composite_bind_group,
        gbuffer_albedo_view, gbuffer_normal_view, gbuffer_emissive_view, gbuffer_depth_view,
        ssao_view, ssao_blur_view, ssao_bind_group, ssao_blur_bind_group,
        shadow2_bind_group, lighting_bind_group,
        debug_uniform, debug_bind_ssao, debug_bind_gbuffer, debug_bind_depth, debug_bind_hdr,
    }
}