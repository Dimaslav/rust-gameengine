//! Простой 2D spatial hash (сетка в плоскости XZ).

use std::collections::HashMap;

use glam::Vec3;

pub struct SpatialHash2D {
    cell_size: f32,
    inv_cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Переиспользуемый скретч для `query_aabb` и `query_radius`.
    scratch_keys: Vec<(i32, i32)>,
}

impl SpatialHash2D {
    pub fn new(cell_size: f32) -> Self {
        let cs = cell_size.max(0.01);
        Self {
            cell_size: cs,
            inv_cell: 1.0 / cs,
            cells: HashMap::new(),
            scratch_keys: Vec::with_capacity(64),
        }
    }

    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Очистить содержимое. **ИЗМЕНЕНО:** раньше очищались только Vec
    /// внутри HashMap, но сами ключи оставались, и HashMap рос пустыми
    /// ячейками за долгую сессию. Теперь — `HashMap::clear`, что
    /// освобождает и ключи.
    ///
    /// Vec-аллокации при этом теряются, но их размер обычно мал
    /// (2-8 u32). Если профилирование покажет, что это горячая точка —
    /// можно вернуть «умный clear» с проверкой `occupied_cells() > N`.
    pub fn clear(&mut self) {
        self.cells.clear();
        self.scratch_keys.clear();
    }

    #[inline]
    fn cell_of(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x * self.inv_cell).floor() as i32,
            (pos.z * self.inv_cell).floor() as i32,
        )
    }

    pub fn insert(&mut self, payload: u32, pos: Vec3) {
        let key = self.cell_of(pos);
        self.cells.entry(key).or_default().push(payload);
    }

    pub fn query_radius(&self, pos: Vec3, radius: f32, out: &mut Vec<u32>) {
        out.clear();
        let r = radius.max(0.0);
        let cx0 = ((pos.x - r) * self.inv_cell).floor() as i32;
        let cx1 = ((pos.x + r) * self.inv_cell).floor() as i32;
        let cz0 = ((pos.z - r) * self.inv_cell).floor() as i32;
        let cz1 = ((pos.z + r) * self.inv_cell).floor() as i32;
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if let Some(list) = self.cells.get(&(cx, cz)) {
                    out.extend_from_slice(list);
                }
            }
        }
    }

    pub fn query_aabb(&self, min: Vec3, max: Vec3, out: &mut Vec<u32>) {
        out.clear();
        let cx0 = (min.x * self.inv_cell).floor() as i32;
        let cx1 = (max.x * self.inv_cell).floor() as i32;
        let cz0 = (min.z * self.inv_cell).floor() as i32;
        let cz1 = (max.z * self.inv_cell).floor() as i32;
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if let Some(list) = self.cells.get(&(cx, cz)) {
                    out.extend_from_slice(list);
                }
            }
        }
    }

    pub fn occupied_cells(&self) -> usize {
        self.cells.values().filter(|v| !v.is_empty()).count()
    }

    pub fn len(&self) -> usize {
        self.cells.values().map(|v| v.len()).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.values().all(|v| v.is_empty())
    }
}

impl Default for SpatialHash2D {
    fn default() -> Self {
        Self::new(2.5)
    }
}