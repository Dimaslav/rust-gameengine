//! Grid-based navmesh generation + A* pathfinding.
//!
//! # Что это
//!
//! Не triangle-mesh (Delaunay/Recast), а **2D-сетка** с высотами
//! и признаком walkable. Для типичных игровых сцен (пол + препятствия)
//! этого достаточно: строит быстро, работает за O(1) на lookup,
//! легко визуализируется.
//!
//! # Как бейкается
//!
//! 1. Собираем AABB всех физических коллайдеров.
//! 2. Строим сетку с `cell_size` (по умолчанию 0.5 м).
//! 3. Для каждой ячейки ищем **самый высокий верх** коллайдера,
//!    покрывающего ячейку по XZ → это ground_y.
//! 4. Ячейка блокируется, если какой-то коллайдер пересекает
//!    вертикальный интервал [ground_y + step_height, ground_y + agent_height].
//! 5. 8-соседняя связность: диагональ разрешена, только если оба
//!    ортогональных соседа walkable (защита от срезания углов).
//!
//! # Ограничения v1
//!
//! * Многоуровневые сцены (мост над полом) не поддерживаются:
//!   берётся самый верхний ground.
//! * Криволинейные поверхности (сферы) аппроксимируются верхней
//!   точкой AABB.
//! * Нет "off-mesh links" (прыжки через пропасти).
//!
//! # Debug visualization
//!
//! ИЗМЕНЕНО (Фаза 6.5): добавлен `walkable_edges` — возвращает
//! пары `Vec3` (начало, конец) для граничных рёбер walkable-ячеек.
//! Рёбра рисуются в `LineBatch` в `App::redraw` при включённом
//! `EditorSettings::show_navmesh`. Обходятся только те рёбра,
//! где сосед (слева/справа/сверху/снизу) — не walkable, чтобы
//! не плодить по 4 линии на каждую ячейку.

use std::cmp::Ordering;
use std::collections::BinaryHeap;

use glam::Vec3;

use crate::ecs::World;
use crate::game::components::Visible;
use crate::physics::{BodyType, Collider, RigidBody};

pub const DEFAULT_CELL_SIZE: f32 = 0.5;
pub const DEFAULT_AGENT_RADIUS: f32 = 0.4;
pub const DEFAULT_AGENT_HEIGHT: f32 = 1.8;
pub const DEFAULT_STEP_HEIGHT: f32 = 0.4;
pub const DEFAULT_SLOPE_LIMIT_COS: f32 = 0.7071;
pub const DEFAULT_PADDING: f32 = 4.0;

#[derive(Debug, Clone)]
pub struct BakeOpts {
    pub cell_size: f32,
    pub agent_radius: f32,
    pub agent_height: f32,
    pub step_height: f32,
    pub slope_limit_cos: f32,
    pub padding: f32,
}

impl Default for BakeOpts {
    fn default() -> Self {
        Self {
            cell_size: DEFAULT_CELL_SIZE,
            agent_radius: DEFAULT_AGENT_RADIUS,
            agent_height: DEFAULT_AGENT_HEIGHT,
            step_height: DEFAULT_STEP_HEIGHT,
            slope_limit_cos: DEFAULT_SLOPE_LIMIT_COS,
            padding: DEFAULT_PADDING,
        }
    }
}

#[derive(Clone, Copy)]
struct Cell {
    walkable: bool,
    y: f32,
    occupied: bool,
}

impl Default for Cell {
    fn default() -> Self {
        Self { walkable: false, y: 0.0, occupied: false }
    }
}

pub struct Navmesh {
    origin: Vec3,
    cell_size: f32,
    grid_w: u32,
    grid_h: u32,
    step_height: f32,
    cells: Vec<Cell>,
}

impl Navmesh {
    pub fn cell_size(&self) -> f32 { self.cell_size }
    pub fn grid_size(&self) -> (u32, u32) { (self.grid_w, self.grid_h) }
    pub fn origin(&self) -> Vec3 { self.origin }

    pub fn walkable_count(&self) -> usize {
        self.cells.iter().filter(|c| c.walkable).count()
    }

    fn idx(&self, cx: u32, cz: u32) -> usize {
        (cz * self.grid_w + cx) as usize
    }

    fn cell_center(&self, cx: u32, cz: u32) -> Vec3 {
        let c = &self.cells[self.idx(cx, cz)];
        Vec3::new(
            self.origin.x + (cx as f32 + 0.5) * self.cell_size,
            c.y,
            self.origin.z + (cz as f32 + 0.5) * self.cell_size,
        )
    }

    /// Ближайшая walkable ячейка к `p` (поиск по расширяющемуся
    /// радиусу). Возвращает `(cx, cz)`.
    pub fn nearest_walkable(&self, p: Vec3) -> Option<(u32, u32)> {
        let fx = (p.x - self.origin.x) / self.cell_size - 0.5;
        let fz = (p.z - self.origin.z) / self.cell_size - 0.5;
        let cx0 = fx.round().max(0.0).min((self.grid_w - 1) as f32) as i32;
        let cz0 = fz.round().max(0.0).min((self.grid_h - 1) as f32) as i32;

        if self.is_walkable_at(cx0, cz0) {
            return Some((cx0 as u32, cz0 as u32));
        }

        for r in 1..=12i32 {
            for dy in -r..=r {
                for dx in -r..=r {
                    if dx.abs() != r && dy.abs() != r { continue; }
                    let cx = cx0 + dx;
                    let cz = cz0 + dy;
                    if self.is_walkable_at(cx, cz) {
                        return Some((cx as u32, cz as u32));
                    }
                }
            }
        }
        None
    }

    fn is_walkable_at(&self, cx: i32, cz: i32) -> bool {
        if cx < 0 || cz < 0 { return false; }
        if cx >= self.grid_w as i32 || cz >= self.grid_h as i32 { return false; }
        self.cells[self.idx(cx as u32, cz as u32)].walkable
    }

    // ============================================================
    // A*
    // ============================================================

    pub fn find_path(&self, start: Vec3, end: Vec3) -> Option<Vec<Vec3>> {
        let (sx, sz) = self.nearest_walkable(start)?;
        let (ex, ez) = self.nearest_walkable(end)?;
        if (sx, sz) == (ex, ez) {
            return Some(vec![self.cell_center(ex, ez)]);
        }

        let n = (self.grid_w * self.grid_h) as usize;
        let start_i = self.idx(sx, sz);
        let end_i = self.idx(ex, ez);

        let mut g_score: Vec<f32> = vec![f32::INFINITY; n];
        let mut came_from: Vec<u32> = vec![u32::MAX; n];
        let mut open: BinaryHeap<HeapNode> = BinaryHeap::new();

        g_score[start_i] = 0.0;
        open.push(HeapNode {
            f: self.heuristic(sx, sz, ex, ez),
            idx: start_i as u32,
        });

        while let Some(node) = open.pop() {
            let cur = node.idx as usize;
            if cur == end_i { break; }

            let cx = (cur as u32) % self.grid_w;
            let cz = (cur as u32) / self.grid_w;
            let cur_g = g_score[cur];

            for (dx, dz) in NEIGHBOURS_8 {
                let nx = cx as i32 + dx;
                let nz = cz as i32 + dz;
                if !self.is_walkable_at(nx, nz) { continue; }
                if dx != 0 && dz != 0 {
                    if !self.is_walkable_at(cx as i32 + dx, cz as i32) { continue; }
                    if !self.is_walkable_at(cx as i32, cz as i32 + dz) { continue; }
                }

                let ni = self.idx(nx as u32, nz as u32);
                let step_cost = if dx != 0 && dz != 0 { 1.41421356 } else { 1.0 };
                let tentative = cur_g + step_cost;

                if tentative < g_score[ni] {
                    g_score[ni] = tentative;
                    came_from[ni] = cur as u32;
                    let h = self.heuristic(nx as u32, nz as u32, ex, ez);
                    open.push(HeapNode { f: tentative + h, idx: ni as u32 });
                }
            }
        }

        if came_from[end_i] == u32::MAX && end_i != start_i {
            return None;
        }

        let mut raw: Vec<(u32, u32)> = Vec::new();
        let mut cur = end_i as u32;
        loop {
            let cx = cur % self.grid_w;
            let cz = cur / self.grid_w;
            raw.push((cx, cz));
            if cur == start_i as u32 { break; }
            cur = came_from[cur as usize];
            if cur == u32::MAX { return None; }
        }
        raw.reverse();

        let points: Vec<Vec3> = raw.iter().map(|&(x, z)| self.cell_center(x, z)).collect();
        Some(points)
    }

    fn heuristic(&self, ax: u32, az: u32, bx: u32, bz: u32) -> f32 {
        let dx = (ax as f32 - bx as f32).abs();
        let dz = (az as f32 - bz as f32).abs();
        let (min, max) = if dx < dz { (dx, dz) } else { (dz, dx) };
        min * 1.41421356 + (max - min)
    }

    // ============================================================
    // Path smoothing (string pulling)
    // ============================================================

    pub fn smooth_path<F>(&self, path: Vec<Vec3>, mut los_clear: F) -> Vec<Vec3>
    where
        F: FnMut(Vec3, Vec3) -> bool,
    {
        if path.len() <= 2 { return path; }

        let mut out: Vec<Vec3> = Vec::with_capacity(path.len());
        out.push(path[0]);

        let mut anchor = 0usize;
        while anchor < path.len() - 1 {
            let mut far = anchor + 1;
            for j in (anchor + 2)..path.len() {
                if los_clear(path[anchor], path[j]) {
                    far = j;
                } else {
                    break;
                }
            }
            out.push(path[far]);
            anchor = far;
        }
        out
    }

    // ============================================================
    // Debug visualization (Фаза 6.5)
    // ============================================================

    /// Граничные рёбра walkable-ячеек.
    ///
    /// Возвращает пары `(a, b)` — начало и конец отрезка. Для каждой
    /// walkable-ячейки рисуется только те стороны, где сосед — не
    /// walkable (граница проходимой зоны). Это даёт «контур» navmesh
    /// без внутренних линий.
    ///
    /// Результат: `Vec<(Vec3, Vec3)>`. Вызывающая сторона сама решает,
    /// как рисовать (обычно `LineBatch::line` в цикле).
    ///
    /// Y берётся из `cell.y + 0.05`, чтобы линии не z-fight'ились
    /// с полом.
    pub fn walkable_edges(&self) -> Vec<(Vec3, Vec3)> {
        let mut out = Vec::new();
        if self.grid_w == 0 || self.grid_h == 0 {
            return out;
        }

        let y_offset = 0.05;
        let cs = self.cell_size;

        for cz in 0..self.grid_h {
            for cx in 0..self.grid_w {
                let idx = self.idx(cx, cz);
                let cell = self.cells[idx];
                if !cell.walkable { continue; }

                let x0 = self.origin.x + cx as f32 * cs;
                let x1 = x0 + cs;
                let z0 = self.origin.z + cz as f32 * cs;
                let z1 = z0 + cs;
                let y = cell.y + y_offset;

                let left_ok = cx > 0 && self.cells[self.idx(cx - 1, cz)].walkable;
                let right_ok = cx + 1 < self.grid_w && self.cells[self.idx(cx + 1, cz)].walkable;
                let down_ok = cz > 0 && self.cells[self.idx(cx, cz - 1)].walkable;
                let up_ok = cz + 1 < self.grid_h && self.cells[self.idx(cx, cz + 1)].walkable;

                if !left_ok {
                    out.push((Vec3::new(x0, y, z0), Vec3::new(x0, y, z1)));
                }
                if !right_ok {
                    out.push((Vec3::new(x1, y, z0), Vec3::new(x1, y, z1)));
                }
                if !down_ok {
                    out.push((Vec3::new(x0, y, z0), Vec3::new(x1, y, z0)));
                }
                if !up_ok {
                    out.push((Vec3::new(x0, y, z1), Vec3::new(x1, y, z1)));
                }
            }
        }
        out
    }

    // ============================================================
    // Bake
    // ============================================================

    pub fn bake(world: &World, opts: &BakeOpts) -> Self {
        let mut min = Vec3::splat(f32::INFINITY);
        let mut max = Vec3::splat(f32::NEG_INFINITY);
        let mut any = false;

        for &e in world.entities() {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }
            let Some(col) = world.get::<Collider>(e) else { continue };
            if let Some(rb) = world.get::<RigidBody>(e) {
                if rb.body_type == BodyType::Dynamic { continue; }
            }
            let Some(t) = world.get::<crate::game::components::Transform>(e) else { continue };
            let (bmin, bmax) = collider_aabb(col, t.position, t.rotation, t.scale.abs());
            min = min.min(bmin);
            max = max.max(bmax);
            any = true;
        }

        if !any {
            log::warn!("Navmesh::bake: no physical colliders — returning empty mesh");
            return Self {
                origin: Vec3::ZERO,
                cell_size: opts.cell_size,
                grid_w: 0,
                grid_h: 0,
                step_height: opts.step_height,
                cells: Vec::new(),
            };
        }

        let origin = Vec3::new(
            min.x - opts.padding,
            min.y,
            min.z - opts.padding,
        );
        let size = max - min + Vec3::splat(opts.padding * 2.0);
        let grid_w = (size.x / opts.cell_size).ceil().max(1.0) as u32;
        let grid_h = (size.z / opts.cell_size).ceil().max(1.0) as u32;
        let n = (grid_w * grid_h) as usize;

        log::info!(
            "Navmesh::bake: origin=({:.1},{:.1},{:.1}), grid={}×{} ({} cells, cell={})",
            origin.x, origin.y, origin.z, grid_w, grid_h, n, opts.cell_size
        );

        let mut cells = vec![Cell::default(); n];

        for &e in world.entities() {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }
            let Some(col) = world.get::<Collider>(e) else { continue };
            if let Some(rb) = world.get::<RigidBody>(e) {
                if rb.body_type == BodyType::Dynamic { continue; }
            }
            let Some(t) = world.get::<crate::game::components::Transform>(e) else { continue };
            let (cmin, cmax) = collider_aabb(col, t.position, t.rotation, t.scale.abs());

            let x0 = ((cmin.x - origin.x) / opts.cell_size).floor().max(0.0) as i32;
            let z0 = ((cmin.z - origin.z) / opts.cell_size).floor().max(0.0) as i32;
            let x1 = ((cmax.x - origin.x) / opts.cell_size).ceil().min(grid_w as f32 - 0.001) as i32;
            let z1 = ((cmax.z - origin.z) / opts.cell_size).ceil().min(grid_h as f32 - 0.001) as i32;

            for cz in z0..=z1 {
                for cx in x0..=x1 {
                    if cx < 0 || cz < 0 { continue; }
                    if cx >= grid_w as i32 || cz >= grid_h as i32 { continue; }
                    let idx = (cz as u32 * grid_w + cx as u32) as usize;
                    let cell = &mut cells[idx];
                    if !cell.occupied || cmax.y > cell.y {
                        cell.y = cmax.y;
                        cell.walkable = true;
                        cell.occupied = true;
                    }
                }
            }
        }

        for &e in world.entities() {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }
            let Some(col) = world.get::<Collider>(e) else { continue };
            if let Some(rb) = world.get::<RigidBody>(e) {
                if rb.body_type == BodyType::Dynamic { continue; }
            }
            let Some(t) = world.get::<crate::game::components::Transform>(e) else { continue };
            let (cmin, cmax) = collider_aabb(col, t.position, t.rotation, t.scale.abs());

            let x0 = ((cmin.x - origin.x) / opts.cell_size).floor().max(0.0) as i32;
            let z0 = ((cmin.z - origin.z) / opts.cell_size).floor().max(0.0) as i32;
            let x1 = ((cmax.x - origin.x) / opts.cell_size).ceil().min(grid_w as f32 - 0.001) as i32;
            let z1 = ((cmax.z - origin.z) / opts.cell_size).ceil().min(grid_h as f32 - 0.001) as i32;

            for cz in z0..=z1 {
                for cx in x0..=x1 {
                    if cx < 0 || cz < 0 { continue; }
                    if cx >= grid_w as i32 || cz >= grid_h as i32 { continue; }
                    let idx = (cz as u32 * grid_w + cx as u32) as usize;
                    let cell = &mut cells[idx];
                    if !cell.occupied { continue; }
                    let lo = cell.y + opts.step_height;
                    let hi = cell.y + opts.agent_height;
                    if cmax.y > lo && cmin.y < hi {
                        cell.walkable = false;
                    }
                }
            }
        }

        let walkable = cells.iter().filter(|c| c.walkable).count();
        log::info!("Navmesh: {} / {} walkable cells", walkable, n);

        Self {
            origin,
            cell_size: opts.cell_size,
            grid_w,
            grid_h,
            step_height: opts.step_height,
            cells,
        }
    }
}

const NEIGHBOURS_8: [(i32, i32); 8] = [
    (-1, -1), (0, -1), (1, -1),
    (-1,  0),          (1,  0),
    (-1,  1), (0,  1), (1,  1),
];

#[derive(PartialEq)]
struct HeapNode {
    f: f32,
    idx: u32,
}
impl Eq for HeapNode {}
impl Ord for HeapNode {
    fn cmp(&self, other: &Self) -> Ordering {
        other.f.partial_cmp(&self.f).unwrap_or(Ordering::Equal)
    }
}
impl PartialOrd for HeapNode {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

fn collider_aabb(col: &Collider, pos: Vec3, rotation: glam::Quat, scale: Vec3) -> (Vec3, Vec3) {
    // ИЗМЕНЕНО (rotation fix): бейк navmesh теперь учитывает поворот
    // стен. Без этого navmesh имел «дыры» в повёрнутых стенах, и AI
    // ходил сквозь них.
    col.world_aabb(pos, rotation, scale)
}

/// Проверка видимости между двумя точками через физические коллайдеры.
pub fn segment_clear(world: &World, a: Vec3, b: Vec3) -> bool {
    let dir = b - a;
    let len = dir.length();
    if len < 1e-4 { return true; }
    let dir = dir / len;

    for &e in world.entities() {
        if let Some(v) = world.get::<Visible>(e) {
            if !v.0 { continue; }
        }
        let Some(col) = world.get::<Collider>(e) else { continue };
        let Some(rb) = world.get::<RigidBody>(e) else { continue };
        if rb.body_type != BodyType::Static { continue; }
        let Some(t) = world.get::<crate::game::components::Transform>(e) else { continue };
        let (cmin, cmax) = collider_aabb(col, t.position, t.rotation, t.scale.abs());
        let dy = cmax.y - cmin.y;
        if dy < 0.2 { continue; }
        if segment_hits_aabb(a, dir, len, cmin, cmax) {
            return false;
        }
    }
    true
}

fn segment_hits_aabb(origin: Vec3, dir: Vec3, max_t: f32, bmin: Vec3, bmax: Vec3) -> bool {
    let o = origin.to_array();
    let d = dir.to_array();
    let mn = bmin.to_array();
    let mx = bmax.to_array();

    let mut tmin = 0.0f32;
    let mut tmax = max_t;

    for i in 0..3 {
        if d[i].abs() < 1e-8 {
            if o[i] < mn[i] || o[i] > mx[i] { return false; }
        } else {
            let inv = 1.0 / d[i];
            let mut t1 = (mn[i] - o[i]) * inv;
            let mut t2 = (mx[i] - o[i]) * inv;
            if t1 > t2 { std::mem::swap(&mut t1, &mut t2); }
            if t1 > tmin { tmin = t1; }
            if t2 < tmax { tmax = t2; }
            if tmin > tmax { return false; }
        }
    }
    true
}