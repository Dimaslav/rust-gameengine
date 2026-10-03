//! Палитра примитивов и пресетов для click-to-place.

use glam::Vec3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteItem {
    // Примитивы
    Cube,
    Sphere,
    Cylinder,
    Cone,
    Capsule,
    Plane,
    // Пресеты (несколько компонентов сразу)
    Enemy,
    Pickup,
    Switch,
    TriggerCube,
    Ground,
}

impl PaletteItem {
    /// Все элементы в порядке отображения в палитре.
    pub fn all_primitives() -> &'static [PaletteItem] {
        &[
            PaletteItem::Cube,
            PaletteItem::Sphere,
            PaletteItem::Cylinder,
            PaletteItem::Cone,
            PaletteItem::Capsule,
            PaletteItem::Plane,
        ]
    }

    pub fn all_presets() -> &'static [PaletteItem] {
        &[
            PaletteItem::Enemy,
            PaletteItem::Pickup,
            PaletteItem::Switch,
            PaletteItem::TriggerCube,
            PaletteItem::Ground,
        ]
    }

    pub fn label(&self) -> &'static str {
        match self {
            PaletteItem::Cube => "Cube",
            PaletteItem::Sphere => "Sphere",
            PaletteItem::Cylinder => "Cylinder",
            PaletteItem::Cone => "Cone",
            PaletteItem::Capsule => "Capsule",
            PaletteItem::Plane => "Plane",
            PaletteItem::Enemy => "Enemy",
            PaletteItem::Pickup => "Pickup",
            PaletteItem::Switch => "Switch",
            PaletteItem::TriggerCube => "Trigger",
            PaletteItem::Ground => "Ground",
        }
    }

    /// Меш, который будет назначен при спавне.
    pub fn mesh(&self) -> &'static str {
        match self {
            PaletteItem::Cube => "cube",
            PaletteItem::Sphere => "sphere",
            PaletteItem::Cylinder => "cylinder",
            PaletteItem::Cone => "cone",
            PaletteItem::Capsule => "capsule",
            PaletteItem::Plane => "quad",
            PaletteItem::Enemy => "sphere",
            PaletteItem::Pickup => "sphere",
            PaletteItem::Switch => "cube",
            PaletteItem::TriggerCube => "cube",
            PaletteItem::Ground => "ground",
        }
    }

    /// Материал без текстуры: плоский цвет + metallic/roughness.
    /// Текстурированные (checker) материалы для палитры не используются.
    pub fn material(&self) -> &'static str {
        match self {
            PaletteItem::Cube => "flat_blue",
            PaletteItem::Sphere => "gold",
            PaletteItem::Cylinder => "flat_red",
            PaletteItem::Cone => "flat_red",
            PaletteItem::Capsule => "flat_blue",
            PaletteItem::Plane => "ground",
            PaletteItem::Enemy => "flat_red",
            PaletteItem::Pickup => "emissive",
            PaletteItem::Switch => "flat_blue",
            PaletteItem::TriggerCube => "glass",
            PaletteItem::Ground => "ground",
        }
    }

    /// Смещение по Y при спавне: чтобы примитив стоял на земле,
    /// а не тонул в неё. Возвращает половину высоты AABB.
    pub fn half_height(&self) -> f32 {
        match self {
            PaletteItem::Cube => 0.5,
            PaletteItem::Sphere => 0.5,
            PaletteItem::Cylinder => 0.5,
            PaletteItem::Cone => 0.5,
            PaletteItem::Capsule => 0.75,
            PaletteItem::Plane => 0.0,
            PaletteItem::Enemy => 0.4,
            PaletteItem::Pickup => 0.2,
            PaletteItem::Switch => 0.5,
            PaletteItem::TriggerCube => 0.5,
            PaletteItem::Ground => 0.0,
        }
    }

    pub fn default_scale(&self) -> f32 {
        match self {
            PaletteItem::Pickup => 0.4,
            PaletteItem::Enemy => 0.8,
            _ => 1.0,
        }
    }

    /// Размер одного тайла текстуры в мировых единицах по умолчанию.
    ///
    /// Значение отвечает на вопрос: «сколько метров (world units) занимает
    /// одна копия текстуры». Меньше — текстура чаще повторяется (мельче),
    /// больше — крупнее.
    ///
    /// - Ground — меш 200×200, тайл крупный (8 м), иначе текстура была бы
    ///   слишком мелкой и превратилась бы в шум.
    /// - TriggerCube — заметно крупнее обычного, чтобы куб выделялся.
    /// - Прочее — стандартный 1.0 (один тайл на 1 м).
    pub fn default_tiling_size(&self) -> f32 {
        match self {
            PaletteItem::Ground => 8.0,
            PaletteItem::TriggerCube => 2.0,
            _ => 1.0,
        }
    }

    /// Нужно ли применить snap к Y (для наземных примитивов).
    pub fn snaps_to_ground(&self) -> bool {
        !matches!(self, PaletteItem::Ground)
    }
}

pub struct PaletteState {
    /// Активная «кисть». `None` — обычный режим редактирования.
    pub active: Option<PaletteItem>,
    /// Позиция ghost-превью (обновляется App каждый кадр).
    pub preview_pos: Option<Vec3>,
    /// Snap к сетке при клике (Ctrl).
    pub snap_to_grid: bool,
    pub grid_step: f32,
    /// Если true — кисть не сбрасывается после спавна (можно ставить много).
    pub keep_active: bool,
}

impl Default for PaletteState {
    fn default() -> Self {
        Self {
            active: None,
            preview_pos: None,
            snap_to_grid: true,
            grid_step: 0.5,
            keep_active: true,
        }
    }
}