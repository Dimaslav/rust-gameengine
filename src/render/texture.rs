use anyhow::Result;

pub struct Texture {
    pub texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub bind_group: wgpu::BindGroup,
    pub size: (u32, u32),
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

    pub fn from_file(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        path: &str,
    ) -> Result<Self> {
        let bytes = std::fs::read(path)?;
        Self::from_bytes(device, queue, layout, &bytes, path)
    }

    /// sRGB-текстура (base color, emissive). Аппаратное декодирование в linear.
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

        let size = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };

        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size,
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        // wgpu 25: `ImageCopyTexture` → `TexelCopyTextureInfo`,
        //          `ImageDataLayout` → `TexelCopyBufferLayout`.
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                texture: &texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            data,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(4 * width),
                rows_per_image: Some(height),
            },
            size,
        );

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("texture_sampler"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest,
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
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
        })
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