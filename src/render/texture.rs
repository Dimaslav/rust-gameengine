use anyhow::Result;

pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub size: (u32, u32),
    /// Путь к файлу-источнику, если текстура загружена с диска.
    /// `None` для процедурных текстур (например, `checker`), потому
    /// что их не нужно перезагружать при load сцены — они создаются
    /// при старте движка.
    pub source_path: Option<String>,
    /// ИЗМЕНЕНО (#8): была ли текстура создана как sRGB (`Rgba8UnormSrgb`)
    /// или linear (`Rgba8Unorm`).
    pub is_srgb: bool,
    /// Количество мип-уровней (1, если текстура 1×1 или 2×2).
    /// См. `Sprint 1: mipmaps + anisotropy`.
    pub mip_count: u32,
}

impl Texture {
    pub fn from_bytes(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        bytes: &[u8],
        label: &str,
    ) -> Result<Self> {
        let img = image::load_from_memory(bytes)?.to_rgba8();
        let (width, height) = img.dimensions();
        Self::from_rgba(device, queue, layout, &img, width, height, label)
    }

    /// Загрузка sRGB-текстуры из файла (albedo, emissive).
    pub fn from_file(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        path: &str,
    ) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let img = image::load_from_memory(&bytes)?.to_rgba8();
        let (width, height) = img.dimensions();
        let mut tex = Self::from_rgba(device, queue, layout, &img, width, height, path)?;
        tex.source_path = Some(path.to_string());
        Ok(tex)
    }

    /// ИЗМЕНЕНО (#8): загрузка linear-текстуры из файла (normal map,
    /// metallic-roughness, AO, height, маски).
    pub fn from_file_linear(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        path: &str,
    ) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        let img = image::load_from_memory(&bytes)?.to_rgba8();
        let (width, height) = img.dimensions();
        let mut tex = Self::from_rgba_linear(device, queue, layout, &img, width, height, path)?;
        tex.source_path = Some(path.to_string());
        Ok(tex)
    }

    /// sRGB-текстура (base color, emissive). Аппаратное декодирование в linear.
    /// Мипмапы генерируются автоматически (кроме 1×1/2×2/3×3).
    pub fn from_rgba(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        data: &[u8],
        width: u32,
        height: u32,
        label: &str,
    ) -> Result<Self> {
        Self::from_rgba_with_format(device, queue, layout, data, width, height, label, true)
    }

    /// Linear-текстура (normal map, metallic-roughness, маски).
    pub fn from_rgba_linear(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        data: &[u8],
        width: u32,
        height: u32,
        label: &str,
    ) -> Result<Self> {
        Self::from_rgba_with_format(device, queue, layout, data, width, height, label, false)
    }

    pub fn from_rgba_with_format(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        data: &[u8],
        width: u32,
        height: u32,
        label: &str,
        srgb: bool,
    ) -> Result<Self> {
        let format = if srgb {
            wgpu::TextureFormat::Rgba8UnormSrgb
        } else {
            wgpu::TextureFormat::Rgba8Unorm
        };

        // ИЗМЕНЕНО (Sprint 1: mipmaps): генерируем мип-цепочку на CPU
        // (box filter 2×2) для всех текстур размером ≥ 4×4. Это самая
        // заметная визуальная проблема в любом PBR-движке: без мипмапов
        // PBR читается «шумно» на дистанции, а анизотропия бесполезна
        // (mip sampler фильтрует только между уровнями, а их нет).
        //
        // Для мелких (1×1, 2×2, 3×3) мипы бессмысленны: уже сами по себе
        // меньше одного теха.
        let mip_levels_data = Self::generate_mip_chain_rgba8(data, width, height);
        let mip_count = mip_levels_data.len() as u32;

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: mip_count,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // Загружаем все уровни (level 0 — исходный, 1..N — сгенерированные).
        for (level, level_data) in mip_levels_data.iter().enumerate() {
            let lw = (width >> level).max(1);
            let lh = (height >> level).max(1);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &texture,
                    mip_level: level as u32,
                    origin: wgpu::Origin3d::ZERO,
                    aspect: wgpu::TextureAspect::All,
                },
                level_data,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(4 * lw),
                    rows_per_image: Some(lh),
                },
                wgpu::Extent3d {
                    width: lw,
                    height: lh,
                    depth_or_array_layers: 1,
                },
            );
        }

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Анизотропия — 16× (клампится на устройстве, см.
        // `Renderer::get_sampler`). Работает только в связке с мипмапами.
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture_sampler"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 16,
            ..Default::default()
        });

        let bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("texture_bind_group"),
            layout,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::TextureView(&view),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: wgpu::BindingResource::Sampler(&sampler),
                },
            ],
        });

        Ok(Self {
            texture,
            view,
            sampler,
            bind_group,
            size: (width, height),
            source_path: None,
            is_srgb: srgb,
            mip_count,
        })
    }

    // ============================================================
    // Sprint 1: CPU-side mipmap generation (box filter 2×2)
    // ============================================================

    /// Возвращает Vec всех мип-уровней: `[0]` — level 0 (копия входа),
    /// `[i]` — level i. Для текстур < 4×4 по любой оси возвращает
    /// только level 0 (мипы бессмысленны).
    ///
    /// Алгоритм: box filter 2×2, крайние пиксели (при нечётных размерах)
    /// клампятся на последний ряд/столбец. Это не идеально (Lanczos или
    /// Kaiser лучше по качеству), но для игровых текстур при загрузке —
    /// достаточно и предсказуемо.
    ///
    /// Цена: O(width × height × 4/3) на CPU при загрузке. Для 2048² RGBA
    /// это ~5.6 MB данных и ~10 MB/с копирования — приемлемо. Для больших
    /// EXR/DDS со временем загрузки > 200ms стоит перейти на compute-shader
    /// downsample (Sprint 2), но для .png/.jpg этого хватает.
    fn generate_mip_chain_rgba8(data: &[u8], width: u32, height: u32) -> Vec<Vec<u8>> {
        debug_assert_eq!(data.len(), (width * height * 4) as usize);

        // Мипы бессмысленны для крошечных текстур (в т.ч. для 1×1 fallback).
        if width < 4 || height < 4 {
            return vec![data.to_vec()];
        }

        // Проверка кратности: если какая-то сторона не кратна 4,
        // box filter даст «мыло». Всё равно делаем — для нестандартных
        // разрешений (например, 720p UI) это лучше, чем ничего.
        let mut levels: Vec<Vec<u8>> = Vec::new();
        levels.push(data.to_vec());

        let mut cw = width;
        let mut ch = height;
        while cw > 1 || ch > 1 {
            let nw = (cw / 2).max(1);
            let nh = (ch / 2).max(1);
            let prev = levels.last().unwrap();
            let mut next = vec![0u8; (nw * nh * 4) as usize];

            for y in 0..nh {
                for x in 0..nw {
                    let sx0 = (x * 2).min(cw - 1);
                    let sx1 = (x * 2 + 1).min(cw - 1);
                    let sy0 = (y * 2).min(ch - 1);
                    let sy1 = (y * 2 + 1).min(ch - 1);

                    let idx00 = ((sy0 * cw + sx0) * 4) as usize;
                    let idx10 = ((sy0 * cw + sx1) * 4) as usize;
                    let idx01 = ((sy1 * cw + sx0) * 4) as usize;
                    let idx11 = ((sy1 * cw + sx1) * 4) as usize;

                    let out_idx = ((y * nw + x) * 4) as usize;

                    for c in 0..4 {
                        let s = prev[idx00 + c] as u32
                            + prev[idx10 + c] as u32
                            + prev[idx01 + c] as u32
                            + prev[idx11 + c] as u32;
                        next[out_idx + c] = (s / 4) as u8;
                    }
                }
            }

            levels.push(next);
            cw = nw;
            ch = nh;
        }

        levels
    }

    pub fn white(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        Self::from_rgba(device, queue, layout, &[255, 255, 255, 255], 1, 1, "white")
    }

    pub fn from_solid(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        rgba: [u8; 4],
        label: &str,
    ) -> Result<Self> {
        Self::from_rgba(device, queue, layout, &rgba, 1, 1, label)
    }

    pub fn from_solid_linear(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        rgba: [u8; 4],
        label: &str,
    ) -> Result<Self> {
        Self::from_rgba_linear(device, queue, layout, &rgba, 1, 1, label)
    }
}