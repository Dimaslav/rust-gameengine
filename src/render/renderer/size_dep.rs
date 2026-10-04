//! Ресурсы, зависящие от размера окна.

use crate::render::ibl::IblResources;

use super::gpu_types::*;

/// Сколько уровней в bloom chain.
pub const BLOOM_MIP_COUNT: usize = 5;

pub struct BloomMip {
    pub view: wgpu::TextureView,
    pub _tex: wgpu::Texture,
    pub size: (u32, u32),
}

pub struct BloomChain {
    pub mips: Vec<BloomMip>,
    pub sampler: wgpu::Sampler,

    /// Два варианта prefilter BG — читают TAA resolved[0] или [1].
    /// Выбирается по `frame_index % 2`, чтобы bloom видел уже TAA-результат.
    pub prefilter_bgs: [wgpu::BindGroup; 2],
    pub downsample_bgs: Vec<wgpu::BindGroup>,
    pub upsample_bgs: Vec<wgpu::BindGroup>,

    pub prefilter_uniform: wgpu::Buffer,
    pub _downsample_uniforms: Vec<wgpu::Buffer>,
    pub _upsample_uniforms: Vec<wgpu::Buffer>,
}

pub struct SizeDependent {
    /// HDR после deferred lighting / skybox / forward. Вход TAA.
    pub hdr_view: wgpu::TextureView,
    /// Motion vectors (Rg16Float). Пишется G-buffer'ом и transparent-пассом.
    pub motion_view: wgpu::TextureView,

    /// Ping-pong resolved: TAA пишет в [write_idx], читает из [read_idx].
    pub taa_resolved_views: [wgpu::TextureView; 2],
    pub _taa_resolved_tex: [wgpu::Texture; 2],

    /// Бинд-группы TAA-пасса: [i] читает taa_resolved_views[i] как history.
    pub taa_read_bgs: [wgpu::BindGroup; 2],

    pub taa_uniform: wgpu::Buffer,

    pub bloom_chain: BloomChain,
    pub linear_sampler: wgpu::Sampler,

    /// Два варианта composite BG: [i] читает taa_resolved_views[i].
    pub composite_bgs: [wgpu::BindGroup; 2],

    pub ldr_view: wgpu::TextureView,
    pub fxaa_uniform: wgpu::Buffer,
    pub fxaa_bind_group: wgpu::BindGroup,

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
    taa_layout: &wgpu::BindGroupLayout,
    ssao_uniform: &wgpu::Buffer,
    noise_view: &wgpu::TextureView,
    csm_array_view: &wgpu::TextureView,
    csm_sampler: &wgpu::Sampler,
    cube_shadow_cube_view: &wgpu::TextureView,
    cube_shadow_sampler: &wgpu::Sampler,
    camera_buffer: &wgpu::Buffer,
    ibl: &IblResources,
    bloom_knee: f32,
    bloom_radius: f32,
    tonemap_uniform: &wgpu::Buffer,
) -> SizeDependent {
    let w = config.width.max(1);
    let h = config.height.max(1);

    let hdr_view = create_color_target(device, "hdr", w, h, HDR_FORMAT, 1, true);
    let motion_view = create_color_target(device, "motion", w, h, MOTION_FORMAT, 1, true);
    let ldr_view = create_color_target(device, "ldr", w, h, LDR_FORMAT, 1, true);
    let linear_sampler = create_linear_sampler(device, "post_linear");

    // ============================================================
    // TAA ping-pong resolved (2 HDR-текстуры)
    // ============================================================
    let taa_resolved_tex: [wgpu::Texture; 2] = std::array::from_fn(|i| {
        device.create_texture(&wgpu::TextureDescriptor {
            label: Some(&format!("taa_resolved_{}", i)),
            size: wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        })
    });
    let taa_resolved_views: [wgpu::TextureView; 2] = std::array::from_fn(|i| {
        taa_resolved_tex[i].create_view(&wgpu::TextureViewDescriptor::default())
    });

    let taa_uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("taa_uniform"),
        size: std::mem::size_of::<TaaParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let taa_read_bgs: [wgpu::BindGroup; 2] = std::array::from_fn(|i| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("taa_read_bg"),
            layout: taa_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&hdr_view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&taa_resolved_views[i]),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::TextureView(&motion_view),
                },
                wgpu::BindGroupEntry {
                    binding: 3,
                    resource: wgpu::BindingResource::Sampler(&linear_sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 4,
                    resource: taa_uniform.as_entire_binding(),
                },
            ],
        })
    });

    // ============================================================
    // Bloom chain — prefilter читает TAA resolved (2 варианта).
    // ============================================================
    let bloom_chain = build_bloom_chain(
        device,
        bloom_layout,
        &taa_resolved_views,
        w,
        h,
        bloom_knee,
        bloom_radius,
    );

    // ============================================================
    // G-buffer: 3 MRT + depth (+ motion отдельно)
    // ============================================================
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
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&gbuffer_normal_view),
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
            wgpu::BindGroupEntry {
                binding: 4,
                resource: wgpu::BindingResource::TextureView(&gbuffer_normal_view),
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

    let composite_bgs: [wgpu::BindGroup; 2] = std::array::from_fn(|i| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("composite_bind_group"),
            layout: tonemap_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&taa_resolved_views[i]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::TextureView(&bloom_chain.mips[0].view),
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
        })
    });

    let debug_uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("debug_uniform"),
        size: std::mem::size_of::<DebugParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let fxaa_uniform = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("fxaa_uniform"),
        size: std::mem::size_of::<FxaaParams>() as u64,
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        mapped_at_creation: false,
    });

    let fxaa_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("fxaa_bg"),
        layout: debug_layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: wgpu::BindingResource::TextureView(&ldr_view),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: wgpu::BindingResource::Sampler(&linear_sampler),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: fxaa_uniform.as_entire_binding(),
            },
        ],
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
        motion_view,
        taa_resolved_views,
        _taa_resolved_tex: taa_resolved_tex,
        taa_read_bgs,
        taa_uniform,
        bloom_chain,
        linear_sampler,
        composite_bgs,
        ldr_view,
        fxaa_uniform,
        fxaa_bind_group,
        gbuffer_albedo_view,
        gbuffer_normal_view,
        gbuffer_emissive_view,
        gbuffer_depth_view,
        ssao_view,
        ssao_blur_view,
        ssao_bind_group,
        ssao_blur_bind_group,
        shadow2_bind_group,
        lighting_bind_group,
        debug_uniform,
        debug_bind_ssao,
        debug_bind_gbuffer,
        debug_bind_depth,
        debug_bind_hdr,
    }
}

// ============================================================
// Bloom chain builder
// ============================================================

fn build_bloom_chain(
    device: &wgpu::Device,
    bloom_layout: &wgpu::BindGroupLayout,
    taa_resolved_views: &[wgpu::TextureView; 2],
    screen_w: u32,
    screen_h: u32,
    knee: f32,
    radius: f32,
) -> BloomChain {
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("bloom_sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        address_mode_w: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::FilterMode::Nearest,
        ..Default::default()
    });

    let mut mips: Vec<BloomMip> = Vec::with_capacity(BLOOM_MIP_COUNT);
    for level in 0..BLOOM_MIP_COUNT {
        let div = 1u32 << (level + 1);
        let w = (screen_w / div).max(1);
        let h = (screen_h / div).max(1);

        let tex = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("bloom_mip"),
            size: wgpu::Extent3d {
                width: w,
                height: h,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: HDR_FORMAT,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let view = tex.create_view(&wgpu::TextureViewDescriptor::default());
        mips.push(BloomMip {
            view,
            _tex: tex,
            size: (w, h),
        });
    }

    let prefilter_uniform = make_bloom_uniform(
        device,
        screen_w,
        screen_h,
        mips[0].size.0,
        mips[0].size.1,
        1.0,
        knee,
        radius,
    );

    let prefilter_bgs: [wgpu::BindGroup; 2] = std::array::from_fn(|i| {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom_prefilter_bg"),
            layout: bloom_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&taa_resolved_views[i]),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: prefilter_uniform.as_entire_binding(),
                },
            ],
        })
    });

    let mut downsample_bgs = Vec::with_capacity(BLOOM_MIP_COUNT - 1);
    let mut downsample_uniforms = Vec::with_capacity(BLOOM_MIP_COUNT - 1);
    for i in 1..BLOOM_MIP_COUNT {
        let src = &mips[i - 1];
        let dst = &mips[i];
        let u = make_bloom_uniform(
            device,
            src.size.0,
            src.size.1,
            dst.size.0,
            dst.size.1,
            1.0,
            knee,
            radius,
        );
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom_downsample_bg"),
            layout: bloom_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: u.as_entire_binding(),
                },
            ],
        });
        downsample_bgs.push(bg);
        downsample_uniforms.push(u);
    }

    let mut upsample_bgs = Vec::with_capacity(BLOOM_MIP_COUNT - 1);
    let mut upsample_uniforms = Vec::with_capacity(BLOOM_MIP_COUNT - 1);
    for src_level in (1..BLOOM_MIP_COUNT).rev() {
        let src = &mips[src_level];
        let dst = &mips[src_level - 1];
        let u = make_bloom_uniform(
            device,
            src.size.0,
            src.size.1,
            dst.size.0,
            dst.size.1,
            1.0,
            knee,
            radius,
        );
        let bg = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("bloom_upsample_bg"),
            layout: bloom_layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&src.view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: u.as_entire_binding(),
                },
            ],
        });
        upsample_bgs.push(bg);
        upsample_uniforms.push(u);
    }

    BloomChain {
        mips,
        sampler,
        prefilter_bgs,
        downsample_bgs,
        upsample_bgs,
        prefilter_uniform,
        _downsample_uniforms: downsample_uniforms,
        _upsample_uniforms: upsample_uniforms,
    }
}

fn make_bloom_uniform(
    device: &wgpu::Device,
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
    threshold: f32,
    knee: f32,
    radius: f32,
) -> wgpu::Buffer {
    use wgpu::util::DeviceExt;

    let data = BloomParams {
        texel: [
            1.0 / src_w.max(1) as f32,
            1.0 / src_h.max(1) as f32,
            1.0 / dst_w.max(1) as f32,
            1.0 / dst_h.max(1) as f32,
        ],
        params: [threshold, knee, radius, 0.0],
    };

    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("bloom_uniform"),
        contents: bytemuck::bytes_of(&data),
        usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
    })
}