//! Простой 2D spatial hash (сетка в плоскости XZ).
//!
//! # FIX #1 (spatial hash: AABB insert)
//!
//! Раньше `insert` вставлял payload только в **одну клетку** по центру
//! объекта. Для больших статических коллайдеров (пол 20×20 м, стены,
//! террейн) это означало, что объект находится в клетке «где-то в
//! центре», а `query_aabb` вокруг игрока эту клетку не задевает →
//! игрок проваливался сквозь пол у края, враги проходили сквозь стены.
//!
//! Теперь:
//!   * `insert(payload, pos)` — точечная вставка, для маленьких
//!     объектов (агенты, снаряды). Осталась для `AiSystem`.
//!   * `insert_aabb(payload, min, max)` — вставляет payload во **все**
//!     клетки, которые пересекает AABB. Для объектов, чей AABB
//!     занимает > `MAX_CELLS_PER_INSERT` клеток, payload идёт в
//!     `always_check` и возвращается при любом `query_aabb`.
//!   * `query_aabb` и `query_radius` возвращают **уникальные**
//!     payload'ы (`sort_unstable + dedup`).
//!
//! # FIX #1.1 (dedup unconditional)
//!
//! Первый вариант фикса делал `sort_unstable + dedup` **только** если
//! `always_check` непуст. Тест `query_aabb_returns_nearby_entries`
//! поймал это. Теперь dedup выполняется всегда.
//!
//! # FIX #1.2 (query_aabb: normalize reversed min/max)
//!
//! `insert_aabb` уже нормализует `min > max` (объекты с отрицательным
//! scale). `query_aabb` тоже должен — для симметрии и защитной
//! устойчивости к reversed-запросам из вызывающего кода.

use std::collections::HashMap;

use glam::Vec3;

/// Порог «объект слишком большой для сетки». 1024 клетки = сетка
/// 32×32, что при `cell_size = 4.0` покрывает AABB до 128×128 м.
/// Всё, что больше (полы подземелий, террейн 2000×2000), уходит в
/// `always_check` — иначе `HashMap` раздувается до 10^5+ ячеек на
/// один объект, и вставка занимает десятки миллисекунд.
const MAX_CELLS_PER_INSERT: u64 = 1024;

pub struct SpatialHash2D {
    cell_size: f32,
    inv_cell: f32,
    cells: HashMap<(i32, i32), Vec<u32>>,
    /// Payload'ы, чей AABB слишком велик для сетки. Проверяются
    /// при каждом `query_aabb` / `query_radius` независимо от
    /// запрошенной области.
    always_check: Vec<u32>,
    /// Переиспользуемый скретч для `query_aabb` и `query_radius`.
    /// Не используется в текущей реализации (оставлен для будущего
    /// multi-cell traversal с кэшем ключей).
    scratch_keys: Vec<(i32, i32)>,
}

impl SpatialHash2D {
    pub fn new(cell_size: f32) -> Self {
        let cs = cell_size.max(0.01);
        Self {
            cell_size: cs,
            inv_cell: 1.0 / cs,
            cells: HashMap::new(),
            always_check: Vec::new(),
            scratch_keys: Vec::with_capacity(64),
        }
    }

    pub fn cell_size(&self) -> f32 {
        self.cell_size
    }

    /// Очистить содержимое. `HashMap::clear` освобождает и ключи
    /// (пустые Vec внутри ячеек уходят в аллокатор).
    pub fn clear(&mut self) {
        self.cells.clear();
        self.always_check.clear();
        self.scratch_keys.clear();
    }

    #[inline]
    fn cell_of(&self, pos: Vec3) -> (i32, i32) {
        (
            (pos.x * self.inv_cell).floor() as i32,
            (pos.z * self.inv_cell).floor() as i32,
        )
    }

    /// Точечная вставка. Используется `AiSystem` для агентов
    /// (маленькие объекты, центр ≈ AABB).
    pub fn insert(&mut self, payload: u32, pos: Vec3) {
        let key = self.cell_of(pos);
        self.cells.entry(key).or_default().push(payload);
    }

    /// Вставка по AABB. Payload попадает во **все** клетки, которые
    /// пересекает AABB. Если клеток больше `MAX_CELLS_PER_INSERT`,
    /// payload уходит в `always_check` — он будет возвращён при
    /// любом query, независимо от того, какие клетки запрошены.
    pub fn insert_aabb(&mut self, payload: u32, min: Vec3, max: Vec3) {
        // Защита от невалидного AABB (NaN/Inf). Молча пропускаем —
        // такой объект не должен участвовать в broad-phase.
        if !min.is_finite() || !max.is_finite() {
            return;
        }

        let cx0 = (min.x * self.inv_cell).floor() as i32;
        let cx1 = (max.x * self.inv_cell).floor() as i32;
        let cz0 = (min.z * self.inv_cell).floor() as i32;
        let cz1 = (max.z * self.inv_cell).floor() as i32;

        // Если min > max (например, объект с отрицательным масштабом),
        // нормализуем. `f32::floor` даёт -1.0 для -0.001, что может
        // дать cx0 > cx1.
        let (cx0, cx1) = if cx0 <= cx1 { (cx0, cx1) } else { (cx1, cx0) };
        let (cz0, cz1) = if cz0 <= cz1 { (cz0, cz1) } else { (cz1, cz0) };

        let span_x = (cx1 - cx0 + 1).max(0) as u64;
        let span_z = (cz1 - cz0 + 1).max(0) as u64;
        let total_cells = span_x.saturating_mul(span_z);

        if total_cells > MAX_CELLS_PER_INSERT {
            self.always_check.push(payload);
            return;
        }

        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                self.cells.entry((cx, cz)).or_default().push(payload);
            }
        }
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
        if !self.always_check.is_empty() {
            out.extend_from_slice(&self.always_check);
        }
        // FIX #1.1: dedup обязателен всегда.
        out.sort_unstable();
        out.dedup();
    }

    /// Возвращает **уникальные** payload'ы (sort + dedup).
    ///
    /// FIX #1.2: `min > max` нормализуется (как в `insert_aabb`).
    /// Без этого reversed-запрос давал пустой результат
    /// (`cz0..=cz1` с `cz0 > cz1` — пустой range в Rust).
    pub fn query_aabb(&self, min: Vec3, max: Vec3, out: &mut Vec<u32>) {
        out.clear();
        let cx0 = (min.x * self.inv_cell).floor() as i32;
        let cx1 = (max.x * self.inv_cell).floor() as i32;
        let cz0 = (min.z * self.inv_cell).floor() as i32;
        let cz1 = (max.z * self.inv_cell).floor() as i32;
        let (cx0, cx1) = if cx0 <= cx1 { (cx0, cx1) } else { (cx1, cx0) };
        let (cz0, cz1) = if cz0 <= cz1 { (cz0, cz1) } else { (cz1, cz0) };
        for cz in cz0..=cz1 {
            for cx in cx0..=cx1 {
                if let Some(list) = self.cells.get(&(cx, cz)) {
                    out.extend_from_slice(list);
                }
            }
        }
        if !self.always_check.is_empty() {
            out.extend_from_slice(&self.always_check);
        }
        out.sort_unstable();
        out.dedup();
    }

    pub fn occupied_cells(&self) -> usize {
        self.cells.values().filter(|v| !v.is_empty()).count()
    }

    pub fn len(&self) -> usize {
        self.cells.values().map(|v| v.len()).sum::<usize>() + self.always_check.len()
    }

    pub fn is_empty(&self) -> bool {
        self.cells.values().all(|v| v.is_empty()) && self.always_check.is_empty()
    }
}

impl Default for SpatialHash2D {
    fn default() -> Self {
        Self::new(2.5)
    }
}

// ============================================================
// Тесты (45)
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // Construction / cell_size
    // ============================================================

    #[test]
    fn cell_size_zero_clamped_to_min() {
        let h = SpatialHash2D::new(0.0);
        assert_eq!(h.cell_size(), 0.01);
    }

    #[test]
    fn cell_size_negative_clamped_to_min() {
        let h = SpatialHash2D::new(-5.0);
        assert_eq!(h.cell_size(), 0.01);
    }

    #[test]
    fn cell_size_below_min_clamped() {
        let h = SpatialHash2D::new(0.001);
        assert_eq!(h.cell_size(), 0.01);
    }

    #[test]
    fn cell_size_valid_preserved() {
        let h = SpatialHash2D::new(3.7);
        assert_eq!(h.cell_size(), 3.7);
    }

    #[test]
    fn default_uses_2_5_cell() {
        assert_eq!(SpatialHash2D::default().cell_size(), 2.5);
    }

    // ============================================================
    // Basic
    // ============================================================

    #[test]
    fn empty_hash_returns_nothing() {
        let h = SpatialHash2D::new(4.0);
        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-100.0), Vec3::splat(100.0), &mut out);
        assert!(out.is_empty());
        h.query_radius(Vec3::ZERO, 10.0, &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn empty_hash_is_empty() {
        let h = SpatialHash2D::new(4.0);
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.occupied_cells(), 0);
    }

    #[test]
    fn point_insert_visible_in_radius() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::new(0.0, 0.0, 0.0));
        h.insert(1, Vec3::new(1.0, 0.0, 0.0));
        h.insert(2, Vec3::new(100.0, 0.0, 0.0));

        let mut out = Vec::new();
        h.query_radius(Vec3::ZERO, 2.0, &mut out);
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn point_insert_same_position_dedups() {
        // FIX #1.1: один payload вставлен дважды в ту же точку —
        // query вернёт его один раз (payload уникален в entries).
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);
        h.insert(0, Vec3::ZERO);

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(1.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn point_insert_two_payloads_same_cell() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);
        h.insert(1, Vec3::new(1.0, 0.0, 1.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(2.0), &mut out);
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn max_payload_value_works() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(u32::MAX, Vec3::ZERO);
        h.insert(0, Vec3::new(100.0, 0.0, 0.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(1.0), &mut out);
        assert_eq!(out, vec![u32::MAX]);
    }

    // ============================================================
    // Dedup
    // ============================================================

    #[test]
    fn aabb_insert_dedup() {
        // AABB пересекает 4 клетки, payload не должен дублироваться.
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-0.5), Vec3::splat(0.5));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(1.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn aabb_insert_dedup_in_radius() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-0.5), Vec3::splat(0.5));

        let mut out = Vec::new();
        h.query_radius(Vec3::ZERO, 2.0, &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn multiple_overlapping_aabbs_all_returned() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-5.0), Vec3::splat(5.0));
        h.insert_aabb(1, Vec3::splat(-3.0), Vec3::splat(3.0));
        h.insert_aabb(2, Vec3::splat(-1.0), Vec3::splat(1.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-10.0), Vec3::splat(10.0), &mut out);
        assert_eq!(out, vec![0, 1, 2]);
    }

    // ============================================================
    // Cell boundaries
    // ============================================================

    #[test]
    fn exact_cell_boundary() {
        // cell_size = 4. Точка (4.0, 4.0) уходит в клетку (1, 1),
        // а не в (0, 0).
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::new(4.0, 0.0, 4.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::new(4.0, 0.0, 4.0), Vec3::new(5.0, 0.0, 5.0), &mut out);
        assert_eq!(out, vec![0]);

        out.clear();
        h.query_aabb(Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.9, 0.0, 3.9), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn just_below_cell_boundary() {
        // 3.999 → клетка 0.
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::new(3.999, 0.0, 3.999));

        let mut out = Vec::new();
        h.query_aabb(Vec3::new(0.0, 0.0, 0.0), Vec3::new(3.999, 0.0, 3.999), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn negative_cell_boundary() {
        // cell_size = 4, inv_cell = 0.25.
        //   -4.0   * 0.25 = -1.000 → floor = -1  (клетка -1)
        //   -4.001 * 0.25 = -1.00025 → floor = -2 (клетка -2)
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::new(-4.0, 0.0, -4.0));
        h.insert(1, Vec3::new(-4.001, 0.0, -4.001));

        // [-5, -3] покрывает клетки -2..=-1 → оба payload'а.
        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-5.0), Vec3::splat(-3.0), &mut out);
        assert_eq!(out, vec![0, 1]);

        // [-3.9, -0.1]:
        //   floor(-3.9 * 0.25) = floor(-0.975) = -1
        //   floor(-0.1 * 0.25) = floor(-0.025) = -1
        // Только клетка -1 → только payload 0.
        out.clear();
        h.query_aabb(Vec3::splat(-3.9), Vec3::splat(-0.1), &mut out);
        assert_eq!(out, vec![0]);

        // [-8, -4.1]:
        //   floor(-8.0 * 0.25) = -2
        //   floor(-4.1 * 0.25) = floor(-1.025) = -2
        // Только клетка -2 → только payload 1.
        out.clear();
        h.query_aabb(Vec3::splat(-8.0), Vec3::splat(-4.1), &mut out);
        assert_eq!(out, vec![1]);
    }

    #[test]
    fn negative_coordinates_aabb() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::new(-10.0, 0.0, -10.0), Vec3::new(-5.0, 0.0, -5.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::new(-11.0, 0.0, -11.0), Vec3::new(-4.0, 0.0, -4.0), &mut out);
        assert_eq!(out, vec![0]);

        out.clear();
        h.query_aabb(Vec3::new(0.0, 0.0, 0.0), Vec3::new(10.0, 0.0, 10.0), &mut out);
        assert!(out.is_empty());
    }

    // ============================================================
    // AABB size edge cases
    // ============================================================

    #[test]
    fn aabb_zero_size_behaves_as_point() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::new(2.0, 0.0, 2.0), Vec3::new(2.0, 0.0, 2.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(0.0), Vec3::splat(4.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn aabb_inside_one_cell() {
        // AABB 0.5×0.5 в клетке 0 (cell_size 4).
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::ZERO, Vec3::new(0.5, 0.0, 0.5));

        assert_eq!(h.occupied_cells(), 1);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn aabb_spanning_two_cells_x() {
        // AABB 0..4.0 по X — клетки 0 и 1 (floor(4/4)=1).
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::ZERO, Vec3::new(4.0, 0.0, 0.1));

        assert_eq!(h.occupied_cells(), 2);
        assert_eq!(h.len(), 2); // payload в 2 клетках
    }

    #[test]
    fn aabb_just_inside_one_cell() {
        // AABB 0..3.999 — только клетка 0.
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::ZERO, Vec3::new(3.999, 0.0, 3.999));

        assert_eq!(h.occupied_cells(), 1);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn reversed_aabb_normalized() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::new(5.0, 0.0, 5.0), Vec3::new(-5.0, 0.0, -5.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-10.0), Vec3::splat(10.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn nan_aabb_ignored() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(f32::NAN), Vec3::splat(1.0));
        assert!(h.is_empty());
    }

    #[test]
    fn inf_aabb_ignored() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-1.0), Vec3::splat(f32::INFINITY));
        assert!(h.is_empty());
    }

    // ============================================================
    // always_check / MAX_CELLS_PER_INSERT
    // ============================================================

    #[test]
    fn huge_floor_goes_to_always_check() {
        // 2000×2000 / 4.0 = 500×500 = 250 000 клеток > 1024.
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-1000.0), Vec3::splat(1000.0));

        let mut out = Vec::new();
        h.query_aabb(
            Vec3::new(999.0, 0.0, 999.0),
            Vec3::new(1001.0, 0.0, 1001.0),
            &mut out,
        );
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn exact_max_cells_goes_to_cells_not_always_check() {
        // cell_size = 1.0, AABB [0, 31] × [0, 31]:
        //   cx0 = floor(0/1) = 0
        //   cx1 = floor(31/1) = 31
        //   span_x = 31 - 0 + 1 = 32
        //   32 × 32 = 1024 — ровно порог.
        let mut h = SpatialHash2D::new(1.0);
        h.insert_aabb(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(31.0, 0.0, 31.0),
        );

        assert_eq!(h.occupied_cells(), 1024);
        assert_eq!(h.len(), 1024); // payload в 1024 клетках
    }

    #[test]
    fn just_over_max_cells_goes_to_always_check() {
        // AABB [0, 32] × [0, 32] — span 33×33 = 1089 > 1024.
        let mut h = SpatialHash2D::new(1.0);
        h.insert_aabb(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(32.0, 0.0, 32.0),
        );

        assert_eq!(h.occupied_cells(), 0);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn span_1x1024_goes_to_cells() {
        // 1 клетка по X, 1024 по Z. Ровно 1024.
        let mut h = SpatialHash2D::new(1.0);
        h.insert_aabb(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(0.5, 0.0, 1023.0),
        );

        assert_eq!(h.occupied_cells(), 1024);
        assert_eq!(h.len(), 1024);
    }

    #[test]
    fn span_1024x1_goes_to_cells() {
        let mut h = SpatialHash2D::new(1.0);
        h.insert_aabb(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1023.0, 0.0, 0.5),
        );

        assert_eq!(h.occupied_cells(), 1024);
        assert_eq!(h.len(), 1024);
    }

    #[test]
    fn span_1025x1_goes_to_always_check() {
        let mut h = SpatialHash2D::new(1.0);
        h.insert_aabb(
            0,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(1024.0, 0.0, 0.5),
        );

        assert_eq!(h.occupied_cells(), 0);
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn always_check_multiple_payloads() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-1000.0), Vec3::splat(1000.0));
        h.insert_aabb(1, Vec3::splat(-2000.0), Vec3::splat(2000.0));
        h.insert_aabb(2, Vec3::splat(-3000.0), Vec3::splat(3000.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(1.0), &mut out);
        assert_eq!(out, vec![0, 1, 2]);
    }

    #[test]
    fn always_check_returned_in_radius() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-1000.0), Vec3::splat(1000.0));

        let mut out = Vec::new();
        h.query_radius(Vec3::ZERO, 5.0, &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn always_check_dedup_on_reinsert() {
        // Один и тот же payload дважды в always_check — вернётся
        // один раз после dedup.
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-1000.0), Vec3::splat(1000.0));
        h.insert_aabb(0, Vec3::splat(-1000.0), Vec3::splat(1000.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(1.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn mixed_normal_and_always_check() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-0.5), Vec3::splat(0.5));
        h.insert_aabb(1, Vec3::splat(-1000.0), Vec3::splat(1000.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-1.0), Vec3::splat(1.0), &mut out);
        assert_eq!(out, vec![0, 1]);
    }

    // ============================================================
    // query_radius / query_aabb edge cases
    // ============================================================

    #[test]
    fn query_radius_zero_same_cell() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);

        let mut out = Vec::new();
        h.query_radius(Vec3::new(1.0, 0.0, 1.0), 0.0, &mut out);
        assert_eq!(out, vec![0]); // (1,1) в той же клетке (0,0)
    }

    #[test]
    fn query_radius_zero_different_cell() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);

        let mut out = Vec::new();
        h.query_radius(Vec3::new(4.0, 0.0, 0.0), 0.0, &mut out);
        assert!(out.is_empty()); // (4,0) в клетке (1,0)
    }

    #[test]
    fn query_radius_negative_treated_as_zero() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);

        let mut out = Vec::new();
        h.query_radius(Vec3::ZERO, -10.0, &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn query_radius_in_negative_region() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);

        let mut out = Vec::new();
        h.query_radius(Vec3::new(-10.0, 0.0, 0.0), 15.0, &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn query_aabb_empty_region() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);

        let mut out = Vec::new();
        h.query_aabb(
            Vec3::new(100.0, 0.0, 100.0),
            Vec3::new(200.0, 0.0, 200.0),
            &mut out,
        );
        assert!(out.is_empty());
    }

    #[test]
    fn query_aabb_reversed_normalized() {
        // min > max в запросе — нормализуется (FIX #1.2).
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(0, Vec3::splat(-0.5), Vec3::splat(0.5));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(1.0), Vec3::splat(-1.0), &mut out);
        assert_eq!(out, vec![0]);
    }

    #[test]
    fn query_aabb_covers_whole_world() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::new(-100.0, 0.0, -100.0));
        h.insert(1, Vec3::ZERO);
        h.insert(2, Vec3::new(100.0, 0.0, 100.0));

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-200.0), Vec3::splat(200.0), &mut out);
        assert_eq!(out, vec![0, 1, 2]);
    }

    // ============================================================
    // Clear / len / occupied_cells
    // ============================================================

    #[test]
    fn clear_resets_cells_and_always_check() {
        let mut h = SpatialHash2D::new(4.0);
        h.insert(0, Vec3::ZERO);
        h.insert_aabb(1, Vec3::splat(-10.0), Vec3::splat(10.0));
        h.insert_aabb(2, Vec3::splat(-1000.0), Vec3::splat(1000.0));
        assert!(!h.is_empty());

        h.clear();
        assert!(h.is_empty());
        assert_eq!(h.len(), 0);
        assert_eq!(h.occupied_cells(), 0);

        let mut out = Vec::new();
        h.query_aabb(Vec3::splat(-2000.0), Vec3::splat(2000.0), &mut out);
        assert!(out.is_empty());
    }

    #[test]
    fn len_counts_cells_and_always_check() {
        let mut h = SpatialHash2D::new(4.0);
        // 1 payload в 1 клетке.
        h.insert(0, Vec3::ZERO);
        // 1 payload в always_check.
        h.insert_aabb(1, Vec3::splat(-1000.0), Vec3::splat(1000.0));
        // 1 payload в 2 клетках (AABB 0..4).
        h.insert_aabb(2, Vec3::new(10.0, 0.0, 10.0), Vec3::new(14.0, 0.0, 10.5));

        // 1 (cell insert) + 1 (always) + 2 (2 клетки) = 4.
        assert_eq!(h.len(), 4);
        // 1 + 0 + 2 = 3 непустых клетки (always_check не входит).
        assert_eq!(h.occupied_cells(), 3);
    }

    // ============================================================
    // Регрессионные (реальные сценарии движка)
    // ============================================================

    #[test]
    fn large_floor_visible_from_edge() {
        // Act1_Floor: 20×20×0.5, центр в ZERO. Капсула у восточного
        // края (x=9) должна видеть пол.
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(
            0,
            Vec3::new(-10.0, -0.25, -10.0),
            Vec3::new(10.0, 0.25, 10.0),
        );

        let mut out = Vec::new();
        h.query_aabb(
            Vec3::new(8.65, 0.1, -0.35),
            Vec3::new(9.35, 1.45, 0.35),
            &mut out,
        );
        assert!(out.contains(&0), "edge capsule must see 20×20 floor");
    }

    #[test]
    fn query_aabb_returns_nearby_entries_no_duplicates() {
        // Регрессия на баг: 3 бокса (0,0), (2,0), (100,0), AABB
        // [-1..3] должен вернуть ровно [0, 1], не [0,0,0,0,1,1].
        let mut h = SpatialHash2D::new(4.0);
        let half = Vec3::splat(0.5);
        h.insert_aabb(0, Vec3::ZERO - half, Vec3::ZERO + half);
        h.insert_aabb(
            1,
            Vec3::new(2.0, 0.0, 0.0) - half,
            Vec3::new(2.0, 0.0, 0.0) + half,
        );
        h.insert_aabb(
            2,
            Vec3::new(100.0, 0.0, 0.0) - half,
            Vec3::new(100.0, 0.0, 0.0) + half,
        );

        let mut out = Vec::new();
        h.query_aabb(Vec3::new(-1.0, -1.0, -1.0), Vec3::new(3.0, 1.0, 1.0), &mut out);
        assert_eq!(out, vec![0, 1]);
    }

    #[test]
    fn wall_20m_visible_to_capsule_at_any_x() {
        // Длинная стена 20 м (как Act1_Wall_S). Капсула проходит
        // вдоль неё в разных точках — всегда должна видеть стену.
        let mut h = SpatialHash2D::new(4.0);
        h.insert_aabb(
            0,
            Vec3::new(-10.0, 0.0, -0.3),
            Vec3::new( 10.0, 5.0,  0.3),
        );

        for x in [-9.0, -5.0, 0.0, 5.0, 9.0] {
            let mut out = Vec::new();
            h.query_aabb(
                Vec3::new(x - 0.35, 0.0, -0.35),
                Vec3::new(x + 0.35, 1.8, 0.35),
                &mut out,
            );
            assert!(out.contains(&0), "wall missing at x = {}", x);
        }
    }
}