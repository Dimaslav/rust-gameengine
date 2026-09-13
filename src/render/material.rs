use std::collections::HashMap;

/// PBR-материал в стиле glTF metallic-roughness.
#[derive(Debug, Clone)]
pub struct Material {
    /// Базовый цвет. Умножается на base_color_texture.
    pub base_color: [f32; 4],
    /// Имя текстуры base color в реестре `Renderer.textures`.
    pub base_color_texture: Option<String>,

    /// Металличность (0 = диэлектрик, 1 = металл). Умножается на MR-текстуру (B).
    pub metallic: f32,
    /// Шероховатость (0 = зеркало, 1 = матовый). Умножается на MR-текстуру (G).
    pub roughness: f32,
    /// Имя текстуры metallic-roughness. R = occlusion, G = roughness, B = metallic.
    pub metallic_roughness_texture: Option<String>,

    /// Normal map. Тангент-спейс, RGB = XYZ.
    pub normal_texture: Option<String>,
    /// Множитель для normal map (1.0 — без изменений).
    pub normal_scale: f32,

    /// Эмиссия — добавляется в итог без освещения.
    pub emissive: [f32; 3],
    /// Имя текстуры эмиссии. Умножается на `emissive`.
    pub emissive_texture: Option<String>,
}

impl Default for Material {
    fn default() -> Self {
        Self {
            base_color: [1.0, 1.0, 1.0, 1.0],
            base_color_texture: None,
            metallic: 0.0,
            roughness: 0.8,
            metallic_roughness_texture: None,
            normal_texture: None,
            normal_scale: 1.0,
            emissive: [0.0, 0.0, 0.0],
            emissive_texture: None,
        }
    }
}

impl Material {
    /// Простой диэлектрик с заданным base_color.
    pub fn new(base_color: [f32; 4]) -> Self {
        Self { base_color, ..Default::default() }
    }

    pub fn with_texture(mut self, name: impl Into<String>) -> Self {
        self.base_color_texture = Some(name.into());
        self
    }

    pub fn with_metallic_roughness(mut self, metallic: f32, roughness: f32) -> Self {
        self.metallic = metallic;
        self.roughness = roughness;
        self
    }

    pub fn with_mr_texture(mut self, name: impl Into<String>) -> Self {
        self.metallic_roughness_texture = Some(name.into());
        self
    }

    pub fn with_normal_texture(mut self, name: impl Into<String>) -> Self {
        self.normal_texture = Some(name.into());
        self
    }

    pub fn with_normal_scale(mut self, scale: f32) -> Self {
        self.normal_scale = scale;
        self
    }

    pub fn with_emissive(mut self, rgb: [f32; 3]) -> Self {
        self.emissive = rgb;
        self
    }

    pub fn with_emissive_texture(mut self, name: impl Into<String>) -> Self {
        self.emissive_texture = Some(name.into());
        self
    }
}

/// Реестр `Material` — только данные. GPU-bind groups лежат в `Renderer`.
#[derive(Default)]
pub struct MaterialRegistry {
    materials: HashMap<String, Material>,
}

impl MaterialRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, mat: Material) {
        self.materials.insert(name.into(), mat);
    }

    pub fn get(&self, name: &str) -> Option<&Material> {
        self.materials.get(name)
    }

    pub fn get_or_default<'a>(&'a self, name: &str, default: &'a Material) -> &'a Material {
        self.materials.get(name).unwrap_or(default)
    }
}