use glam::Vec3;

use crate::render::terrain::{
    ray_heightmap, BrushRegion, Falloff, Heightmap, PALETTE_NAMES,
};

// ============================================================
// TerrainBrush
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TerrainBrush {
    Raise,
    Lower,
    Smooth,
    Flatten,
    Paint(u8),
}

impl TerrainBrush {
    pub fn label(self) -> &'static str {
        match self {
            Self::Raise => "⬆ Raise",
            Self::Lower => "⬇ Lower",
            Self::Smooth => "〰 Smooth",
            Self::Flatten => "▬ Flatten",
            Self::Paint(0) => "🌿 Grass",
            Self::Paint(1) => "🪨 Rock",
            Self::Paint(2) => "❄ Snow",
            Self::Paint(3) => "🟫 Dirt",
            Self::Paint(_) => "🎨 Paint",
        }
    }
}

// ============================================================
// TerrainTool
// ============================================================

pub struct TerrainTool {
    pub active: bool,
    pub brush: TerrainBrush,
    pub radius: f32,
    pub strength: f32,
    pub falloff: Falloff,
    pub flatten_target: f32,
    pub texture_size: u32,
    pub dragging: bool,
    pub cursor_world: Option<Vec3>,
    pub last_apply_pos: Option<Vec3>,
    /// Аккумулированная dirty-область за текущий кадр.
    /// `App::redraw` забирает её через `take()` и применяет один раз.
    pub dirty_region: Option<BrushRegion>,
}

impl Default for TerrainTool {
    fn default() -> Self {
        Self {
            active: false,
            brush: TerrainBrush::Raise,
            radius: 8.0,
            strength: 0.8,
            falloff: Falloff::Smooth,
            flatten_target: 0.0,
            texture_size: 2048,
            dragging: false,
            cursor_world: None,
            last_apply_pos: None,
            dirty_region: None,
        }
    }
}

impl TerrainTool {
    /// Обновляет `cursor_world` из ray'а. `true`, если попал.
    pub fn update_cursor(
        &mut self,
        heightmap: &Heightmap,
        ray_origin: Vec3,
        ray_dir: Vec3,
    ) -> bool {
        self.cursor_world = ray_heightmap(heightmap, ray_origin, ray_dir, 5000.0);
        self.cursor_world.is_some()
    }

    /// Применяет кисть. Аккумулирует dirty-region.
    pub fn apply(&mut self, heightmap: &mut Heightmap, pos: Vec3) -> Option<BrushRegion> {
        let reg = match self.brush {
            TerrainBrush::Raise => heightmap.sculpt(
                pos.x, pos.z, self.radius,
                self.strength * self.radius * 0.05,
                self.falloff,
            ),
            TerrainBrush::Lower => heightmap.sculpt(
                pos.x, pos.z, self.radius,
                -self.strength * self.radius * 0.05,
                self.falloff,
            ),
            TerrainBrush::Smooth => heightmap.smooth(
                pos.x, pos.z, self.radius, self.strength, self.falloff,
            ),
            TerrainBrush::Flatten => heightmap.flatten(
                pos.x, pos.z, self.radius,
                self.flatten_target, self.strength, self.falloff,
            ),
            TerrainBrush::Paint(layer) => heightmap.paint(
                pos.x, pos.z, self.radius, layer, self.strength, self.falloff,
            ),
        };
        if let Some(r) = reg {
            self.dirty_region = Some(match self.dirty_region {
                Some(prev) => BrushRegion {
                    x0: prev.x0.min(r.x0),
                    z0: prev.z0.min(r.z0),
                    x1: prev.x1.max(r.x1),
                    z1: prev.z1.max(r.z1),
                },
                None => r,
            });
        }
        self.last_apply_pos = Some(pos);
        reg
    }

    pub fn palette_names(&self) -> &'static [&'static str; 4] {
        &PALETTE_NAMES
    }

    pub fn should_throttle(&self, pos: Vec3) -> bool {
        match self.last_apply_pos {
            Some(p) => (p - pos).length() < self.radius * 0.1,
            None => false,
        }
    }
}