use serde::{Deserialize, Serialize};

fn one() -> [f32; 2] {
    [1.0, 1.0]
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct TransformDef {
    #[serde(default)]
    pub position: [f32; 2],
    #[serde(default)]
    pub rotation: f32,
    #[serde(default = "one")]
    pub scale: [f32; 2],
}

impl Default for TransformDef {
    fn default() -> Self {
        Self { position: [0.0; 2], rotation: 0.0, scale: [1.0; 2] }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpriteDef {
    pub texture: String,
    pub width: f32,
    pub height: f32,
    #[serde(default)]
    pub color: Option<[f32; 4]>,
    #[serde(default)]
    pub z: Option<f32>,
    #[serde(default)]
    pub uv_rect: Option<[f32; 4]>,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct VelocityDef {
    pub x: f32,
    pub y: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct SpinnerDef {
    pub speed: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct EntityDef {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub transform: Option<TransformDef>,
    #[serde(default)]
    pub sprite: Option<SpriteDef>,
    #[serde(default)]
    pub velocity: Option<VelocityDef>,
    #[serde(default)]
    pub spinner: Option<SpinnerDef>,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default)]
pub struct SceneDef {
    #[serde(default)]
    pub entities: Vec<EntityDef>,
}