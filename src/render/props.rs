//! Процедурные детализированные модели-пропсы: ящики с досками,
//! бочки с обручами, рифлёные колонны, деревья, скалы, надгробия,
//! стойки для оружия и т.д.
//!
//! Все генераторы — чистые функции `(&Device, params...) -> Mesh`.
//! Юнит-размер: где это важно — пропс вписан в box [-0.5, +0.5],
//! чтобы demo мог масштабировать через `Transform.scale`.

use glam::Vec3;
use std::f32::consts::TAU;

use crate::render::mesh::{Mesh, Vertex3D};

// ============================================================
// Builder
// ============================================================

struct Builder {
    verts: Vec<Vertex3D>,
    idx: Vec<u32>,
}

impl Builder {
    fn new() -> Self {
        Self { verts: Vec::new(), idx: Vec::new() }
    }

    fn push_vertex(&mut self, p: Vec3, n: Vec3, uv: [f32; 2]) -> u32 {
        let i = self.verts.len() as u32;
        self.verts.push(Vertex3D::static_vertex(
            p.to_array(), n.to_array(), uv, [1.0; 4],
        ));
        i
    }

    fn quad(&mut self, a: Vec3, b: Vec3, c: Vec3, d: Vec3, n: Vec3, uv_scale: f32) {
        let i0 = self.push_vertex(a, n, [0.0, 0.0]);
        let i1 = self.push_vertex(b, n, [uv_scale, 0.0]);
        let i2 = self.push_vertex(c, n, [uv_scale, uv_scale]);
        let i3 = self.push_vertex(d, n, [0.0, uv_scale]);
        self.idx.extend_from_slice(&[i0, i1, i2, i0, i2, i3]);
    }

    fn box_centered(&mut self, c: Vec3, h: Vec3, uv: f32) {
        // +X
        self.quad(
            c + Vec3::new(h.x, -h.y, h.z),
            c + Vec3::new(h.x, -h.y, -h.z),
            c + Vec3::new(h.x, h.y, -h.z),
            c + Vec3::new(h.x, h.y, h.z),
            Vec3::X, uv,
        );
        // -X
        self.quad(
            c + Vec3::new(-h.x, -h.y, -h.z),
            c + Vec3::new(-h.x, -h.y, h.z),
            c + Vec3::new(-h.x, h.y, h.z),
            c + Vec3::new(-h.x, h.y, -h.z),
            Vec3::NEG_X, uv,
        );
        // +Y
        self.quad(
            c + Vec3::new(-h.x, h.y, h.z),
            c + Vec3::new(h.x, h.y, h.z),
            c + Vec3::new(h.x, h.y, -h.z),
            c + Vec3::new(-h.x, h.y, -h.z),
            Vec3::Y, uv,
        );
        // -Y
        self.quad(
            c + Vec3::new(-h.x, -h.y, -h.z),
            c + Vec3::new(h.x, -h.y, -h.z),
            c + Vec3::new(h.x, -h.y, h.z),
            c + Vec3::new(-h.x, -h.y, h.z),
            Vec3::NEG_Y, uv,
        );
        // +Z
        self.quad(
            c + Vec3::new(-h.x, -h.y, h.z),
            c + Vec3::new(h.x, -h.y, h.z),
            c + Vec3::new(h.x, h.y, h.z),
            c + Vec3::new(-h.x, h.y, h.z),
            Vec3::Z, uv,
        );
        // -Z
        self.quad(
            c + Vec3::new(h.x, -h.y, -h.z),
            c + Vec3::new(-h.x, -h.y, -h.z),
            c + Vec3::new(-h.x, h.y, -h.z),
            c + Vec3::new(h.x, h.y, -h.z),
            Vec3::NEG_Z, uv,
        );
    }

    /// Кольцо вокруг оси Y.
    fn ring_y(&mut self, y: f32, r: f32, seg: u32, uv_y: f32) -> Vec<u32> {
        let mut v = Vec::with_capacity(seg as usize);
        for i in 0..seg {
            let a = i as f32 / seg as f32 * TAU;
            let (s, c) = a.sin_cos();
            v.push(self.push_vertex(
                Vec3::new(c * r, y, s * r),
                Vec3::Y,
                [i as f32 / seg as f32, uv_y],
            ));
        }
        v
    }

    fn bridge(&mut self, a: &[u32], b: &[u32]) {
        let n = a.len();
        for i in 0..n {
            let j = (i + 1) % n;
            self.idx.extend_from_slice(&[a[i], b[i], b[j], a[i], b[j], a[j]]);
        }
    }

    fn cone(&mut self, base_y: f32, base_r: f32, apex_y: f32, seg: u32) {
        let ring = self.ring_y(base_y, base_r, seg, 0.0);
        let apex = self.push_vertex(Vec3::new(0.0, apex_y, 0.0), Vec3::Y, [0.5, 1.0]);
        let cap = self.push_vertex(Vec3::new(0.0, base_y, 0.0), Vec3::NEG_Y, [0.5, 0.0]);
        let n = seg as usize;
        for i in 0..n {
            let j = (i + 1) % n;
            self.idx.extend_from_slice(&[ring[i], apex, ring[j]]);
            self.idx.extend_from_slice(&[cap, ring[i], ring[j]]);
        }
    }

    fn sphere(&mut self, center: Vec3, radius: f32, rings: u32, segs: u32) {
        let mut prev: Vec<u32> = Vec::new();
        for ring in 0..=rings {
            let phi = std::f32::consts::PI * ring as f32 / rings as f32;
            let (sp, cp) = phi.sin_cos();
            let mut curr = Vec::with_capacity(segs as usize);
            for seg in 0..segs {
                let theta = TAU * seg as f32 / segs as f32;
                let (st, ct) = theta.sin_cos();
                let n = Vec3::new(ct * sp, cp, st * sp);
                curr.push(self.push_vertex(
                    center + n * radius, n,
                    [seg as f32 / segs as f32, ring as f32 / rings as f32],
                ));
            }
            if !prev.is_empty() {
                let n = segs as usize;
                for i in 0..n {
                    let j = (i + 1) % n;
                    self.idx.extend_from_slice(&[prev[i], curr[j], curr[i], prev[i], prev[j], curr[j]]);
                }
            }
            prev = curr;
        }
    }

    fn sphere_deformed(&mut self, center: Vec3, radius: f32, seed: &mut u32, amp: f32) {
        let rings = 6u32;
        let segs = 10u32;
        let mut prev: Vec<u32> = Vec::new();
        for ring in 0..=rings {
            let phi = std::f32::consts::PI * ring as f32 / rings as f32;
            let (sp, cp) = phi.sin_cos();
            let mut curr = Vec::with_capacity(segs as usize);
            for seg in 0..segs {
                let theta = TAU * seg as f32 / segs as f32;
                let (st, ct) = theta.sin_cos();
                let n = Vec3::new(ct * sp, cp, st * sp);
                *seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
                let jitter = 1.0 + (((*seed >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5) * amp;
                curr.push(self.push_vertex(
                    center + n * radius * jitter, n,
                    [seg as f32 / segs as f32, ring as f32 / rings as f32],
                ));
            }
            if !prev.is_empty() {
                let n = segs as usize;
                for i in 0..n {
                    let j = (i + 1) % n;
                    self.idx.extend_from_slice(&[prev[i], curr[j], curr[i], prev[i], prev[j], curr[j]]);
                }
            }
            prev = curr;
        }
    }

    fn build(self, device: &wgpu::Device, name: &str) -> Mesh {
        Mesh::new(device, &self.verts, &self.idx, name)
    }
}

// ============================================================
// Crate — ящик с досками и угловыми стойками
// ============================================================

pub fn crate_detail(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    let h = 0.5;
    // корпус
    b.box_centered(Vec3::ZERO, Vec3::splat(h), 1.0);
    // вертикальные доски по всем 4 стенкам
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            for k in 0..3 {
                let off = (k as f32 - 1.0) * 0.30;
                b.box_centered(
                    Vec3::new(sx * (h + 0.005), 0.0, off),
                    Vec3::new(0.008, h * 0.9, h * 0.12),
                    1.0,
                );
                b.box_centered(
                    Vec3::new(off, 0.0, sz * (h + 0.005)),
                    Vec3::new(h * 0.12, h * 0.9, 0.008),
                    1.0,
                );
            }
        }
    }
    // угловые стойки
    for sx in [-1.0f32, 1.0] {
        for sz in [-1.0f32, 1.0] {
            b.box_centered(
                Vec3::new(sx * (h - 0.04), 0.0, sz * (h - 0.04)),
                Vec3::new(0.06, h + 0.02, 0.06),
                1.0,
            );
        }
    }
    // металлические полосы сверху и снизу
    for y in [-h + 0.06, h - 0.06] {
        b.box_centered(Vec3::new(0.0, y, h + 0.01), Vec3::new(h*0.98, 0.03, 0.01), 1.0);
        b.box_centered(Vec3::new(0.0, y, -(h + 0.01)), Vec3::new(h*0.98, 0.03, 0.01), 1.0);
        b.box_centered(Vec3::new(h + 0.01, y, 0.0), Vec3::new(0.01, 0.03, h*0.98), 1.0);
        b.box_centered(Vec3::new(-(h + 0.01), y, 0.0), Vec3::new(0.01, 0.03, h*0.98), 1.0);
    }
    b.build(device, "crate_detail")
}

// ============================================================
// Barrel — бочка с обручами
// ============================================================

pub fn barrel_detail(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    let seg = 20u32;
    let r = 0.5;
    let h2 = 0.5;
    // профиль: узкие концы, пузо в середине
    let profile = [
        (-h2, r * 0.72),
        (-h2 * 0.55, r * 0.94),
        (0.0, r),
        (h2 * 0.55, r * 0.94),
        (h2, r * 0.72),
    ];
    let mut rings: Vec<Vec<u32>> = Vec::new();
    for (y, rr) in profile.iter() {
        rings.push(b.ring_y(*y, *rr, seg, (*y + h2) / 1.0));
    }
    for i in 0..rings.len() - 1 {
        b.bridge(&rings[i], &rings[i + 1]);
    }
    // крышки
    let top_c = b.push_vertex(Vec3::new(0.0, h2, 0.0), Vec3::Y, [0.5, 0.5]);
    let bot_c = b.push_vertex(Vec3::new(0.0, -h2, 0.0), Vec3::NEG_Y, [0.5, 0.5]);
    let top = &rings[rings.len() - 1];
    let bot = &rings[0];
    for i in 0..seg as usize {
        let j = (i + 1) % seg as usize;
        // Верх: флип обхода, чтобы нормаль была +Y (крышка «сверху»).
        b.idx.extend_from_slice(&[top_c, top[j], top[i]]);
        // Низ: флип, чтобы нормаль была −Y (крышка «снизу»).
        b.idx.extend_from_slice(&[bot_c, bot[i], bot[j]]);
    }
    // обручи
    for y_frac in [-0.62f32, 0.0, 0.62] {
        let y = y_frac * h2;
        let rr = r * (1.0 - (y_frac * y_frac) * 0.25) + 0.025;
        let a = b.ring_y(y - 0.035, rr, seg, 0.0);
        let c = b.ring_y(y + 0.035, rr, seg, 0.0);
        b.bridge(&a, &c);
        // закрываем внешние кольца — маленькие
        let _ = (a, c);
    }
    b.build(device, "barrel_detail")
}

// ============================================================
// Column — рифлёная колонна с базой и капителью
// ============================================================

pub fn column_fluted(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    let h2 = 0.5;
    let seg = 24u32;
    let r = 0.42;

    // база
    b.box_centered(Vec3::new(0.0, -h2 + 0.04, 0.0), Vec3::new(0.5, 0.04, 0.5), 1.0);
    b.box_centered(Vec3::new(0.0, -h2 + 0.11, 0.0), Vec3::new(0.47, 0.03, 0.47), 1.0);
    let base_ring = b.ring_y(-h2 + 0.14, r, seg, 0.0);

    // рифление: чередуем чуть больший/меньший радиус
    let mut profile: Vec<(f32, f32)> = Vec::new();
    let steps = 6;
    for k in 0..=steps {
        let t = k as f32 / steps as f32;
        let y = -h2 + 0.14 + t * (1.0 - 0.28);
        let rr = if k % 2 == 0 { r } else { r * 0.94 };
        profile.push((y, rr));
    }
    let mut rings: Vec<Vec<u32>> = vec![base_ring.clone()];
    for (y, rr) in profile.iter().skip(1) {
        rings.push(b.ring_y(*y, *rr, seg, (y + h2) * 1.0));
    }
    for i in 0..rings.len() - 1 {
        b.bridge(&rings[i], &rings[i + 1]);
    }
    let top_ring = rings.last().unwrap().clone();

    // верхняя крышка
    let top_c = b.push_vertex(Vec3::new(0.0, h2 - 0.14, 0.0), Vec3::Y, [0.5, 0.5]);
    for i in 0..seg as usize {
        let j = (i + 1) % seg as usize;
        b.idx.extend_from_slice(&[top_c, top_ring[j], top_ring[i]]);
    }

    // капитель
    b.box_centered(Vec3::new(0.0, h2 - 0.09, 0.0), Vec3::new(0.47, 0.03, 0.47), 1.0);
    b.box_centered(Vec3::new(0.0, h2 - 0.03, 0.0), Vec3::new(0.5, 0.04, 0.5), 1.0);
    b.build(device, "column_fluted")
}

// ============================================================
// Ruined pillar — сломанная колонна (для подземелья)
// ============================================================

pub fn pillar_ruined(device: &wgpu::Device, seed: u32) -> Mesh {
    let mut b = Builder::new();
    let seg = 12u32;
    let h2 = 0.5;
    let r = 0.42;
    // база
    b.box_centered(Vec3::new(0.0, -h2 + 0.05, 0.0), Vec3::new(0.5, 0.05, 0.5), 1.0);
    // shaft — обрезанный сверху, с шумом
    let bot_ring = b.ring_y(-h2 + 0.1, r, seg, 0.0);
    let mut s = seed;
    let mut top_ring: Vec<u32> = Vec::with_capacity(seg as usize);
    for i in 0..seg {
        let a = i as f32 / seg as f32 * TAU;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let jitter = ((s >> 8) & 0xFFFF) as f32 / 65535.0;
        let y = h2 * (0.4 + jitter * 0.5);
        let (sa, ca) = a.sin_cos();
        top_ring.push(b.push_vertex(
            Vec3::new(ca * r * 0.95, y, sa * r * 0.95),
            Vec3::Y, [i as f32 / seg as f32, 0.8],
        ));
    }
    b.bridge(&bot_ring, &top_ring);
    // неровная верхушка
    let cap = b.push_vertex(Vec3::new(0.0, h2 * 0.62, 0.0), Vec3::Y, [0.5, 0.5]);
    for i in 0..seg as usize {
        let j = (i + 1) % seg as usize;
        b.idx.extend_from_slice(&[cap, top_ring[j], top_ring[i]]);
    }
    b.build(device, "pillar_ruined")
}

// ============================================================
// Chest — сундук с крышкой и металлическими полосами
// ============================================================

pub fn chest_detail(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    let h = 0.5;
    // корпус: [-0.5, +0.5] по X, [-0.5, 0.0] по Y, [-0.4, +0.4] по Z
    b.box_centered(Vec3::new(0.0, -h * 0.5, 0.0), Vec3::new(h, h * 0.5, 0.4), 1.0);
    // крышка
    b.box_centered(Vec3::new(0.0, h * 0.15, 0.0), Vec3::new(h + 0.01, h * 0.15, 0.41), 1.0);
    // полосы (по бокам)
    for sz in [-0.4f32, 0.4] {
        b.box_centered(Vec3::new(0.0, 0.0, sz), Vec3::new(h + 0.02, h * 0.32, 0.02), 1.0);
    }
    // центральная полоса поперёк
    b.box_centered(Vec3::new(0.0, 0.0, 0.0), Vec3::new(0.03, h * 0.62, 0.42), 1.0);
    // замок
    b.box_centered(Vec3::new(0.0, -h * 0.1, 0.42), Vec3::new(0.06, 0.06, 0.02), 1.0);
    b.build(device, "chest_detail")
}

// ============================================================
// Torch stand — стойка с факелом
// ============================================================

pub fn torch_stand(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    // основание
    b.box_centered(Vec3::new(0.0, 0.02, 0.0), Vec3::new(0.15, 0.02, 0.15), 1.0);
    b.box_centered(Vec3::new(0.0, 0.08, 0.0), Vec3::new(0.12, 0.04, 0.12), 1.0);
    // стойка
    let seg = 10u32;
    let pole_r = 0.035;
    let pole_h = 1.4;
    let a = b.ring_y(0.12, pole_r, seg, 0.0);
    let c = b.ring_y(pole_h, pole_r, seg, 1.0);
    b.bridge(&a, &c);
    // кольцо-держатель
    let ring_a = b.ring_y(pole_h - 0.08, pole_r + 0.02, seg, 0.0);
    let ring_b = b.ring_y(pole_h - 0.04, pole_r + 0.02, seg, 0.0);
    b.bridge(&ring_a, &ring_b);
    // чаша
    let cup_bot = b.ring_y(pole_h, 0.06, seg, 0.0);
    let cup_mid = b.ring_y(pole_h + 0.06, 0.11, seg, 0.5);
    let cup_top = b.ring_y(pole_h + 0.16, 0.13, seg, 1.0);
    b.bridge(&cup_bot, &cup_mid);
    b.bridge(&cup_mid, &cup_top);
    b.build(device, "torch_stand")
}

// ============================================================
// Brazier — жаровня с ножками
// ============================================================

pub fn brazier_detail(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    // 3 ножки
    for i in 0..3 {
        let a = i as f32 / 3.0 * TAU;
        let (s, c) = a.sin_cos();
        b.box_centered(
            Vec3::new(c * 0.22, 0.22, s * 0.22),
            Vec3::new(0.035, 0.22, 0.035),
            1.0,
        );
    }
    // чаша (3 уровня сужения)
    let seg = 16u32;
    let bowl_bot = b.ring_y(0.44, 0.10, seg, 0.0);
    let bowl_mid = b.ring_y(0.55, 0.28, seg, 0.5);
    let bowl_top = b.ring_y(0.60, 0.32, seg, 1.0);
    b.bridge(&bowl_bot, &bowl_mid);
    b.bridge(&bowl_mid, &bowl_top);
    // верхнее кольцо (утолщение)
    let rim_a = b.ring_y(0.62, 0.33, seg, 0.0);
    let rim_b = b.ring_y(0.64, 0.32, seg, 0.0);
    b.bridge(&bowl_top, &rim_a);
    b.bridge(&rim_a, &rim_b);
    b.build(device, "brazier_detail")
}

// ============================================================
// Tree — ёлка и дуб
// ============================================================

pub fn tree_pine(device: &wgpu::Device, height: f32) -> Mesh {
    let mut b = Builder::new();
    let trunk_r = height * 0.035;
    let trunk_h = height * 0.30;
    let seg = 8u32;
    // ствол
    let bot = b.ring_y(0.0, trunk_r, seg, 0.0);
    let top = b.ring_y(trunk_h, trunk_r * 0.55, seg, 1.0);
    b.bridge(&bot, &top);
    // 4 слоя ёлки
    let layers = 4;
    for i in 0..layers {
        let t = i as f32 / layers as f32;
        let base_y = trunk_h * 0.55 + t * height * 0.20;
        let layer_r = height * 0.24 * (1.0 - t * 0.55);
        let layer_h = height * 0.30;
        b.cone(base_y, layer_r, base_y + layer_h, seg * 3);
    }
    b.build(device, "tree_pine")
}

pub fn tree_oak(device: &wgpu::Device, height: f32, seed: u32) -> Mesh {
    let mut b = Builder::new();
    let trunk_r = height * 0.05;
    let trunk_h = height * 0.55;
    let seg = 10u32;
    let bot = b.ring_y(0.0, trunk_r * 1.2, seg, 0.0);
    let top = b.ring_y(trunk_h, trunk_r * 0.5, seg, 1.0);
    b.bridge(&bot, &top);
    // ветки
    let mut s = seed;
    for i in 0..4 {
        let a = i as f32 / 4.0 * TAU + 0.7;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let tilt = ((s >> 8) & 0xFF) as f32 / 255.0;
        let (sa, ca) = a.sin_cos();
        let bx = ca * height * 0.15 * tilt;
        let bz = sa * height * 0.15 * tilt;
        let by = trunk_h * (0.55 + tilt * 0.3);
        // прямоугольный "прут"
        let dir = Vec3::new(bx, by, bz) - Vec3::new(0.0, trunk_h * 0.4, 0.0);
        let dn = dir.normalize_or(Vec3::Y);
        let side = Vec3::new(-dn.z, 0.0, dn.x).normalize_or(Vec3::X) * trunk_r * 0.4;
        let p0 = Vec3::new(0.0, trunk_h * 0.4, 0.0);
        let p1 = p0 + dir;
        let i0 = b.push_vertex(p0 - side, Vec3::Y, [0.0, 0.0]);
        let i1 = b.push_vertex(p0 + side, Vec3::Y, [1.0, 0.0]);
        let i2 = b.push_vertex(p1 - side * 0.35, Vec3::Y, [0.0, 1.0]);
        let i3 = b.push_vertex(p1 + side * 0.35, Vec3::Y, [1.0, 1.0]);
        b.idx.extend_from_slice(&[i0, i1, i3, i0, i3, i2]);
    }
    // крона — 6 шаров
    let fol = Vec3::new(0.0, trunk_h + height * 0.20, 0.0);
    for _ in 0..6 {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let rx = ((s >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let ry = ((s >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let rz = ((s >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
        let r = height * (0.12 + (((s >> 8) & 0xFF) as f32 / 255.0) * 0.10);
        let off = Vec3::new(rx * height * 0.30, ry * height * 0.18, rz * height * 0.30);
        b.sphere(fol + off, r, 5, 8);
    }
    b.build(device, "tree_oak")
}

// ============================================================
// Rock cluster — 3 деформированных камня
// ============================================================

pub fn rock_cluster(device: &wgpu::Device, size: f32, seed: u32) -> Mesh {
    let mut b = Builder::new();
    let mut s = seed;
    for i in 0..3 {
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let dx = ((s >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
        s = s.wrapping_mul(1664525).wrapping_add(1013904223);
        let dz = ((s >> 8) & 0xFFFF) as f32 / 65535.0 - 0.5;
        let r = size * (0.55 + i as f32 * 0.15);
        let center = Vec3::new(dx * size, r * 0.55, dz * size);
        b.sphere_deformed(center, r, &mut s, 0.55);
    }
    b.build(device, "rock_cluster")
}

// ============================================================
// Прочие декорации
// ============================================================

pub fn bench(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    // сиденье
    b.box_centered(Vec3::new(0.0, 0.45, 0.0), Vec3::new(0.9, 0.04, 0.22), 1.0);
    // спинка
    b.box_centered(Vec3::new(0.0, 0.75, -0.20), Vec3::new(0.9, 0.30, 0.03), 1.0);
    // ножки
    for sx in [-0.78f32, 0.78] {
        b.box_centered(Vec3::new(sx, 0.22, 0.16), Vec3::new(0.03, 0.22, 0.03), 1.0);
        b.box_centered(Vec3::new(sx, 0.22, -0.16), Vec3::new(0.03, 0.22, 0.03), 1.0);
        b.box_centered(Vec3::new(sx, 0.65, -0.20), Vec3::new(0.03, 0.30, 0.03), 1.0);
    }
    b.build(device, "bench")
}

pub fn table(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    b.box_centered(Vec3::new(0.0, 0.72, 0.0), Vec3::new(0.9, 0.05, 0.55), 1.0);
    for sx in [-0.8f32, 0.8] {
        for sz in [-0.45f32, 0.45] {
            b.box_centered(Vec3::new(sx, 0.36, sz), Vec3::new(0.05, 0.36, 0.05), 1.0);
        }
    }
    // поперечные балки
    b.box_centered(Vec3::new(0.0, 0.15, 0.0), Vec3::new(0.8, 0.03, 0.03), 1.0);
    b.build(device, "table")
}

pub fn fence_post(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    b.box_centered(Vec3::new(0.0, 0.5, 0.0), Vec3::new(0.06, 0.5, 0.06), 1.0);
    b.box_centered(Vec3::new(0.0, 0.75, 0.0), Vec3::new(0.5, 0.03, 0.03), 1.0);
    b.box_centered(Vec3::new(0.0, 0.35, 0.0), Vec3::new(0.5, 0.03, 0.03), 1.0);
    b.build(device, "fence_post")
}

pub fn gravestone(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    // плита
    b.box_centered(Vec3::new(0.0, 0.42, 0.0), Vec3::new(0.22, 0.42, 0.06), 1.0);
    // скруглённый верх (упрощённо — полушарие)
    b.sphere(Vec3::new(0.0, 0.84, 0.0), 0.22, 5, 10);
    // основание
    b.box_centered(Vec3::new(0.0, 0.03, 0.0), Vec3::new(0.30, 0.03, 0.12), 1.0);
    b.build(device, "gravestone")
}

pub fn weapon_rack(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    // столбы
    for sx in [-0.4f32, 0.4] {
        b.box_centered(Vec3::new(sx, 0.75, 0.0), Vec3::new(0.05, 0.75, 0.05), 1.0);
    }
    // поперечины
    b.box_centered(Vec3::new(0.0, 1.4, 0.0), Vec3::new(0.45, 0.04, 0.06), 1.0);
    b.box_centered(Vec3::new(0.0, 0.25, 0.0), Vec3::new(0.45, 0.04, 0.06), 1.0);
    // 3 меча
    for i in 0..3 {
        let x = -0.22 + i as f32 * 0.22;
        b.box_centered(Vec3::new(x, 0.95, 0.04), Vec3::new(0.02, 0.45, 0.008), 1.0);
        b.box_centered(Vec3::new(x, 0.45, 0.04), Vec3::new(0.07, 0.02, 0.02), 1.0);
    }
    b.build(device, "weapon_rack")
}

pub fn skull(device: &wgpu::Device) -> Mesh {
    let mut b = Builder::new();
    b.sphere(Vec3::new(0.0, 0.08, 0.0), 0.09, 5, 8);
    b.box_centered(Vec3::new(0.0, 0.00, 0.06), Vec3::new(0.05, 0.025, 0.035), 1.0);
    b.build(device, "skull")
}