//! Простой 2D spatial hash (сетка в плоскости XZ).
//!
//! Используется для:
//!   * AI — поиск соседей агента для расталкивания (O(n²) → O(n)).
//!   * Физика — broad-phase перед narrow-phase (O(n²) → O(n)).
//!   * Любые системы, где нужно «все объекты в радиусе R».
//!
//! # Как работает
//!
//! Мир делится на квадратные ячейки `cell_size × cell_size` в плоскости
//! XZ. Каждый объект кладётся в одну ячейку по своей (x, z). Для запроса
//! «кто в радиусе R от точки P» обходятся только те ячейки, что
//! пересекаются с кругом радиуса R.
//!
//! Payload — `u32`. Семантику определяет caller: в AI это индекс
//! агента в локальном `Vec`, в физике — индекс `BodyState`. Это
//! позволяет избежать generic-параметров и аллокаций.
//!
//! # Как выбрать `cell_size`
//!
//! Правило: `cell_size ≈ 2 × типичный_радиус_запроса`.
//!   * AI:    radius = 0.5 м (agent_radius), cell_size = 2.5
//!   * Phys:  radius ~ 1–2 м,               cell_size = 4.0
//!
//! Слишком большой — много объектов в ячейке, эффективность падает.
//! Слишком маленький — много пустых ячеек, overhead на обход.

use std::collections::HashMap;

use glam::Vec3;

pub struct SpatialHash2D {
    cell_size: f32,
    inv_cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Переиспользуемый scratch-буфер ключей ячеек.
    /// Нужен для `query_aabb`, чтобы не аллоцировать `Vec` каждый вызов.
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

    pub fn cell_size(&self) -> f32 { self.cell_size }

    /// Очистить содержимое, сохранив capacity всех ячеек и map.
    /// Вызывать в начале каждого кадра/итерации.
    pub fn clear(&mut self) {
        for v in self.cells.values_mut() {
            v.clear();
        }
    }

    #[inline]
    fn cell_of(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x * self.inv_cell).floor() as i32,
            (pos.z * self.inv_cell).floor() as i32,
        )
    }

    /// Вставить payload в ячейку по позиции.
    pub fn insert(&mut self, payload: u32, pos: Vec3) {
        let key = self.cell_of(pos);
        self.cells.entry(key).or_default().push(payload);
    }

    /// Кто в радиусе `radius` от `pos`? Пишет payload'ы в `out`,
    /// очищая его. Не фильтрует по фактическому расстоянию —
    /// caller сам это делает. Возвращает **кандидатов**, чьи ячейки
    /// пересекаются с кругом.
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

    /// Кто пересекается с AABB `[min, max]` в плоскости XZ?
    /// Пишет кандидатов в `out` (очищая).
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

    /// Сколько ячеек занято. Для дебага.
    pub fn occupied_cells(&self) -> usize {
        self.cells.values().filter(|v| !v.is_empty()).count()
    }

    /// Всего payload'ов. Для дебага.
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