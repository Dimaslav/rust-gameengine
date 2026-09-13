//! Атлас — набор UV-прямоугольников в одной текстуре.
//!
//! Хранит имя текстуры (в реестре Renderer) и словарь кадров:
//! frame_name -> [u0, v0, u1, v1].
//!
//! Атлас описывается в RON-файле рядом с игрой, саму текстуру грузит
//! Renderer::load_texture.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct FrameDef {
    /// UV-координаты кадра: [u0, v0, u1, v1]
    pub uv_rect: [f32; 4],
    /// Размер кадра в пикселях (необязательно)
    #[serde(default)]
    pub size: Option<[f32; 2]>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct AtlasDef {
    /// Имя текстуры в реестре Renderer.
    pub texture: String,
    pub frames: HashMap<String, FrameDef>,
}

pub struct Atlas {
    pub texture: String,
    pub frames: HashMap<String, FrameDef>,
}

impl Atlas {
    pub fn from_def(def: AtlasDef) -> Self {
        Self {
            texture: def.texture,
            frames: def.frames,
        }
    }

    pub fn frame(&self, name: &str) -> Option<&FrameDef> {
        self.frames.get(name)
    }

    /// UV-rect кадра или полный [0,0,1,1], если кадра нет.
    pub fn uv(&self, name: &str) -> [f32; 4] {
        self.frames
            .get(name)
            .map(|f| f.uv_rect)
            .unwrap_or([0.0, 0.0, 1.0, 1.0])
    }

    /// Размер кадра, если задан.
    pub fn size(&self, name: &str) -> Option<[f32; 2]> {
        self.frames.get(name).and_then(|f| f.size)
    }
}

#[derive(Default)]
pub struct AtlasRegistry {
    atlases: HashMap<String, Atlas>,
}

impl AtlasRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, name: impl Into<String>, atlas: Atlas) {
        self.atlases.insert(name.into(), atlas);
    }

    pub fn load_from_str(&mut self, name: impl Into<String>, text: &str) -> anyhow::Result<()> {
        let def: AtlasDef = ron::from_str(text)?;
        self.insert(name, Atlas::from_def(def));
        Ok(())
    }

    pub fn get(&self, name: &str) -> Option<&Atlas> {
        self.atlases.get(name)
    }
}