//! Procedural terrain + редактирование heightmap/splat.
//!
//! Sprint B1: heightmap + mesh + physics.
//! Sprint B2: baked terrain texture.
//! Sprint C:  sculpt/paint brush, PNG/R16 import, incremental update.

use anyhow::{bail, Context, Result};
use glam::Vec3;
use std::path::Path;

use crate::render::mesh::{Mesh, Vertex3D};

// ============================================================
// Falloff
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Falloff {
    Constant,
    Linear,
    Smooth,
}

impl Default for Falloff {
    fn default() -> Self { Self::Smooth }
}

impl Falloff {
    #[inline]
    pub fn weight(self, d: f32, radius: f32) -> f32 {
        if radius <= 1e-6 { return 0.0; }
        let t = (d / radius).clamp(0.0, 1.0);
        match self {
            Falloff::Constant => 1.0,
            Falloff::Linear => 1.0 - t,
            Falloff::Smooth => {
                let s = 1.0 - t;
                s * s * (3.0 - 2.0 * s)
            }
        }
    }
}

// ============================================================
// Brush region
// ============================================================

/// Прямоугольник в **индексном** пространстве heightmap (0..resolution-1).
#[derive(Debug, Clone, Copy)]
pub struct BrushRegion {
    pub x0: u32, pub z0: u32, pub x1: u32, pub z1: u32,
}

impl BrushRegion {
    /// Расширить в **вершинное** пространство mesh (0..resolution).
    /// Нужно, потому что vertex при билинейной интерполяции зависит
    /// от 2×2 соседних сэмплов.
    pub fn to_mesh_vertex_rect(self, resolution: u32) -> (u32, u32, u32, u32) {
        let r = resolution.max(2);
        let scale = r as f32 / (r - 1) as f32;
        let pad = 2.0;
        let x0 = ((self.x0 as f32 - pad) * scale).floor().max(0.0) as u32;
        let z0 = ((self.z0 as f32 - pad) * scale).floor().max(0.0) as u32;
        let x1 = ((self.x1 as f32 + pad) * scale).ceil().min(r as f32) as u32;
        let z1 = ((self.z1 as f32 + pad) * scale).ceil().min(r as f32) as u32;
        (x0.min(r), z0.min(r), x1.min(r), z1.min(r))
    }
}

// ============================================================
// Default palette
// ============================================================

pub const DEFAULT_PALETTE: [[u8; 4]; 4] = [
    [ 92, 138,  58, 255], // grass
    [140, 130, 116, 255], // rock
    [242, 247, 252, 255], // snow
    [138, 100,  60, 255], // dirt
];

pub const PALETTE_NAMES: [&str; 4] = ["Grass", "Rock", "Snow", "Dirt"];

#[inline]
pub fn blend_palette(palette: &[[u8; 4]; 4], splat: [u8; 4]) -> [u8; 4] {
    let total = (splat[0] as f32 + splat[1] as f32 + splat[2] as f32 + splat[3] as f32).max(1.0);
    let mut out = [0u8; 4];
    for c in 0..4 {
        let mut acc = 0.0f32;
        for i in 0..4 {
            acc += palette[i][c] as f32 * (splat[i] as f32 / total);
        }
        out[c] = acc.clamp(0.0, 255.0) as u8;
    }
    out
}

// ============================================================
// Heightmap
// ============================================================

pub struct Heightmap {
    pub resolution: u32,
    pub size: f32,
    pub heights: Vec<f32>,
    /// RGBA8 splat: R=grass, G=rock, B=snow, A=dirt.
    pub splat: Vec<u8>,
    pub palette: [[u8; 4]; 4],
    cached_min: f32,
    cached_max: f32,
}

impl Heightmap {
    // ============================================================
    // Конструкторы
    // ============================================================

    pub fn new_procedural(resolution: u32, size: f32, seed: u32) -> Self {
        Self::new_procedural_with_options(
            resolution, size, seed, size * 0.15, 50.0,
        )
    }

    pub fn new_procedural_with_options(
        resolution: u32,
        size: f32,
        seed: u32,
        flat_radius: f32,
        max_height: f32,
    ) -> Self {
        let res = resolution.max(2);
        let mut heights = vec![0.0f32; (res * res) as usize];
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;

        for y in 0..res {
            for x in 0..res {
                let u = x as f32 / (res - 1) as f32;
                let v = y as f32 / (res - 1) as f32;
                let wx = (u - 0.5) * size;
                let wz = (v - 0.5) * size;

                let dist = (wx * wx + wz * wz).sqrt();
                let fade = if dist <= flat_radius {
                    0.0
                } else {
                    let t = ((dist - flat_radius) / (size * 0.15)).clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };

                let mut n = 0.0;
                let mut amp = 1.0;
                let mut freq = 1.0;
                for octave in 0..4 {
                    let s = seed.wrapping_add(octave * 17);
                    n += value_noise_2d(wx * freq * 0.006, wz * freq * 0.006, s) * amp;
                    amp *= 0.5;
                    freq *= 2.0;
                }
                n /= 1.875;
                n = n.max(0.0);

                let h = n * max_height * fade;
                heights[(y * res + x) as usize] = h;
                if h < mn { mn = h; }
                if h > mx { mx = h; }
            }
        }

        let mut hm = Self {
            resolution: res,
            size,
            heights,
            splat: vec![0u8; (res * res * 4) as usize],
            palette: DEFAULT_PALETTE,
            cached_min: if mn.is_finite() { mn } else { 0.0 },
            cached_max: if mx.is_finite() { mx } else { 0.0 },
        };
        hm.auto_bake_splat();
        hm
    }

    /// Импорт из R16 (raw u16 little-endian, resolution² значений).
    ///
    /// `height_scale` — максимальная высота, на которую мапится 65535.
    pub fn from_r16(
        bytes: &[u8],
        resolution: u32,
        size: f32,
        height_scale: f32,
    ) -> Result<Self> {
        let res = resolution.max(2);
        let expected = (res * res * 2) as usize;
        if bytes.len() < expected {
            bail!(
                "R16: {} bytes < expected {} ({}×{} × 2)",
                bytes.len(), expected, res, res
            );
        }
        let mut heights = Vec::with_capacity((res * res) as usize);
        for i in 0..(res * res) as usize {
            let lo = bytes[i * 2] as u16;
            let hi = bytes[i * 2 + 1] as u16;
            let v = (hi << 8) | lo;
            heights.push((v as f32 / 65535.0) * height_scale);
        }

        let mut hm = Self {
            resolution: res,
            size,
            heights,
            splat: vec![0u8; (res * res * 4) as usize],
            palette: DEFAULT_PALETTE,
            cached_min: 0.0,
            cached_max: 0.0,
        };
        hm.recompute_min_max();
        hm.auto_bake_splat();
        Ok(hm)
    }

    pub fn to_r16(&self) -> Vec<u8> {
        let range = (self.cached_max - self.cached_min).max(1e-4);
        let mut out = Vec::with_capacity(self.heights.len() * 2);
        for &h in &self.heights {
            let n = ((h - self.cached_min) / range).clamp(0.0, 1.0);
            let v = (n * 65535.0) as u16;
            out.push((v & 0xFF) as u8);
            out.push((v >> 8) as u8);
        }
        out
    }

    /// Импорт из PNG (16-bit luma или 8-bit luma — оба поддерживаются).
    /// Значения [0, 65535] → [0, height_scale].
    pub fn from_png_bytes(bytes: &[u8], size: f32, height_scale: f32) -> Result<Self> {
        let img = image::load_from_memory(bytes)
            .context("PNG decode for heightmap")?;

        let (w, h) = (img.width(), img.height());
        if w != h {
            // Ресемплим в квадрат через `to_luma16` + nearest.
            // Для точности используем `thumbnail`.
            let s = w.min(h);
            let img = img.thumbnail(s, s);
            return Self::from_luma16(img.to_luma16(), size, height_scale);
        }
        Self::from_luma16(img.to_luma16(), size, height_scale)
    }

    pub fn from_png_file(
        path: impl AsRef<Path>,
        size: f32,
        height_scale: f32,
    ) -> Result<Self> {
        let bytes = std::fs::read(path.as_ref())
            .with_context(|| format!("read PNG {}", path.as_ref().display()))?;
        Self::from_png_bytes(&bytes, size, height_scale)
    }

    fn from_luma16(
        img: image::ImageBuffer<image::Luma<u16>, Vec<u16>>,
        size: f32,
        height_scale: f32,
    ) -> Result<Self> {
        let (w, h) = img.dimensions();
        if w != h {
            bail!("heightmap PNG должно быть квадратным, получено {}×{}", w, h);
        }
        let res = w;
        let mut heights = Vec::with_capacity((res * res) as usize);
        // PNG хранит сверху-вниз, terrain — от -Z к +Z.
        for y in 0..res {
            for x in 0..res {
                let v = img.get_pixel(x, y)[0] as f32 / 65535.0;
                heights.push(v * height_scale);
            }
        }

        let mut hm = Self {
            resolution: res,
            size,
            heights,
            splat: vec![0u8; (res * res * 4) as usize],
            palette: DEFAULT_PALETTE,
            cached_min: 0.0,
            cached_max: 0.0,
        };
        hm.recompute_min_max();
        hm.auto_bake_splat();
        Ok(hm)
    }

    // ============================================================
    // Sampling
    // ============================================================

    #[inline]
    pub fn sample(&self, x: f32, z: f32) -> f32 {
        let res = self.resolution;
        let half = self.size * 0.5;
        let u = ((x + half) / self.size).clamp(0.0, 1.0) * (res - 1) as f32;
        let v = ((z + half) / self.size).clamp(0.0, 1.0) * (res - 1) as f32;

        let x0 = u.floor() as u32;
        let z0 = v.floor() as u32;
        let x1 = (x0 + 1).min(res - 1);
        let z1 = (z0 + 1).min(res - 1);
        let tx = u - x0 as f32;
        let tz = v - z0 as f32;

        let h00 = self.heights[(z0 * res + x0) as usize];
        let h10 = self.heights[(z0 * res + x1) as usize];
        let h01 = self.heights[(z1 * res + x0) as usize];
        let h11 = self.heights[(z1 * res + x1) as usize];

        let a = h00 + (h10 - h00) * tx;
        let b = h01 + (h11 - h01) * tx;
        a + (b - a) * tz
    }

    pub fn contains(&self, x: f32, z: f32) -> bool {
        let half = self.size * 0.5;
        x >= -half && x <= half && z >= -half && z <= half
    }

    pub fn normal_at(&self, x: f32, z: f32) -> Vec3 {
        let eps = self.size / self.resolution as f32;
        let h_l = self.sample(x - eps, z);
        let h_r = self.sample(x + eps, z);
        let h_d = self.sample(x, z - eps);
        let h_u = self.sample(x, z + eps);
        let dx = (h_r - h_l) / (2.0 * eps);
        let dz = (h_u - h_d) / (2.0 * eps);
        Vec3::new(-dx, 1.0, -dz).normalize_or(Vec3::Y)
    }

    pub fn min_max(&self) -> (f32, f32) {
        (self.cached_min, self.cached_max)
    }

    pub fn splat_at(&self, x: f32, z: f32) -> [u8; 4] {
        let res = self.resolution;
        let half = self.size * 0.5;
        let u = ((x + half) / self.size).clamp(0.0, 1.0) * (res - 1) as f32;
        let v = ((z + half) / self.size).clamp(0.0, 1.0) * (res - 1) as f32;
        let xi = u.round().min((res - 1) as f32) as u32;
        let zi = v.round().min((res - 1) as f32) as u32;
        let idx = ((zi * res + xi) * 4) as usize;
        [self.splat[idx], self.splat[idx + 1], self.splat[idx + 2], self.splat[idx + 3]]
    }

    // ============================================================
    // Mutation API
    // ============================================================

    pub fn recompute_min_max(&mut self) {
        let mut mn = f32::INFINITY;
        let mut mx = f32::NEG_INFINITY;
        for &h in &self.heights {
            if h < mn { mn = h; }
            if h > mx { mx = h; }
        }
        self.cached_min = if mn.is_finite() { mn } else { 0.0 };
        self.cached_max = if mx.is_finite() { mx } else { 0.0 };
    }

    /// Внутренняя конвертация world → grid.
    #[inline]
    fn to_grid(&self, w: f32) -> f32 {
        let half = self.size * 0.5;
        ((w + half) / self.size).clamp(0.0, 1.0) * (self.resolution - 1) as f32
    }

    /// Внутренняя конвертация grid → world.
    #[inline]
    fn to_world_xz(&self, gx: f32, gz: f32) -> (f32, f32) {
        let res = self.resolution.max(2);
        let u = gx / (res - 1) as f32;
        let v = gz / (res - 1) as f32;
        ((u - 0.5) * self.size, (v - 0.5) * self.size)
    }

    fn brush_region(&self, cx: f32, cz: f32, radius: f32) -> Option<BrushRegion> {
        let res = self.resolution;
        if res < 2 { return None; }
        let gx0 = self.to_grid(cx - radius).floor() as i32;
        let gz0 = self.to_grid(cz - radius).floor() as i32;
        let gx1 = self.to_grid(cx + radius).ceil() as i32;
        let gz1 = self.to_grid(cz + radius).ceil() as i32;
        let gx0 = gx0.max(0) as u32;
        let gz0 = gz0.max(0) as u32;
        let gx1 = gx1.min(res as i32 - 1).max(0) as u32;
        let gz1 = gz1.min(res as i32 - 1).max(0) as u32;
        if gx0 > gx1 || gz0 > gz1 { return None; }
        Some(BrushRegion { x0: gx0, z0: gz0, x1: gx1, z1: gz1 })
    }

    pub fn sculpt(
        &mut self,
        cx: f32, cz: f32, radius: f32,
        delta: f32,
        falloff: Falloff,
    ) -> Option<BrushRegion> {
        let r = self.brush_region(cx, cz, radius)?;
        for z in r.z0..=r.z1 {
            for x in r.x0..=r.x1 {
                let (wx, wz) = self.to_world_xz(x as f32, z as f32);
                let d = ((wx - cx).powi(2) + (wz - cz).powi(2)).sqrt();
                let w = falloff.weight(d, radius);
                if w < 1e-4 { continue; }
                let idx = (z * self.resolution + x) as usize;
                self.heights[idx] += delta * w;
            }
        }
        self.recompute_min_max();
        Some(r)
    }

    pub fn smooth(
        &mut self,
        cx: f32, cz: f32, radius: f32,
        strength: f32,
        falloff: Falloff,
    ) -> Option<BrushRegion> {
        let r = self.brush_region(cx, cz, radius)?;
        let res = self.resolution;
        let orig = self.heights.clone();
        for z in r.z0..=r.z1 {
            for x in r.x0..=r.x1 {
                let (wx, wz) = self.to_world_xz(x as f32, z as f32);
                let d = ((wx - cx).powi(2) + (wz - cz).powi(2)).sqrt();
                let w = falloff.weight(d, radius) * strength;
                if w < 1e-4 { continue; }
                let mut sum: f32 = 0.0;
                let mut cnt: f32 = 0.0;
                for dz in -1i32..=1 {
                    for dx in -1i32..=1 {
                        let nx = x as i32 + dx;
                        let nz = z as i32 + dz;
                        if nx < 0 || nz < 0 { continue; }
                        if nx >= res as i32 || nz >= res as i32 { continue; }
                        sum += orig[(nz as u32 * res + nx as u32) as usize];
                        cnt += 1.0;
                    }
                }
                let avg = sum / cnt.max(1.0);
                let idx = (z * res + x) as usize;
                self.heights[idx] += (avg - self.heights[idx]) * w;
            }
        }
        self.recompute_min_max();
        Some(r)
    }

    pub fn flatten(
        &mut self,
        cx: f32, cz: f32, radius: f32,
        target_h: f32,
        strength: f32,
        falloff: Falloff,
    ) -> Option<BrushRegion> {
        let r = self.brush_region(cx, cz, radius)?;
        for z in r.z0..=r.z1 {
            for x in r.x0..=r.x1 {
                let (wx, wz) = self.to_world_xz(x as f32, z as f32);
                let d = ((wx - cx).powi(2) + (wz - cz).powi(2)).sqrt();
                let w = falloff.weight(d, radius) * strength;
                if w < 1e-4 { continue; }
                let idx = (z * self.resolution + x) as usize;
                self.heights[idx] += (target_h - self.heights[idx]) * w;
            }
        }
        self.recompute_min_max();
        Some(r)
    }

    /// Покрасить слой `layer ∈ 0..=3`. Blend по falloff.
    pub fn paint(
        &mut self,
        cx: f32, cz: f32, radius: f32,
        layer: u8,
        strength: f32,
        falloff: Falloff,
    ) -> Option<BrushRegion> {
        let r = self.brush_region(cx, cz, radius)?;
        let layer = (layer as usize).min(3);
        for z in r.z0..=r.z1 {
            for x in r.x0..=r.x1 {
                let (wx, wz) = self.to_world_xz(x as f32, z as f32);
                let d = ((wx - cx).powi(2) + (wz - cz).powi(2)).sqrt();
                let w = falloff.weight(d, radius) * strength;
                if w < 1e-4 { continue; }
                let idx = (z * self.resolution + x) as usize * 4;
                for c in 0..4 {
                    let cur = self.splat[idx + c] as f32;
                    let target = if c == layer { 255.0 } else { 0.0 };
                    let new = cur + (target - cur) * w;
                    self.splat[idx + c] = new.clamp(0.0, 255.0) as u8;
                }
            }
        }
        Some(r)
    }

    /// Авто-покраска из высоты/склона.
    pub fn auto_bake_splat(&mut self) {
        let res = self.resolution;
        let range = (self.cached_max - self.cached_min).max(1.0);
        for z in 0..res {
            for x in 0..res {
                let (wx, wz) = self.to_world_xz(x as f32, z as f32);
                let h = self.sample(wx, wz);
                let n = self.normal_at(wx, wz);
                let slope = (1.0 - n.y).clamp(0.0, 1.0);
                let hn = ((h - self.cached_min) / range).clamp(0.0, 1.0);

                let grass = (1.0 - slope * 3.0).max(0.0)
                    * (1.0 - (hn - 0.7).max(0.0) * 3.0);
                let rock = (slope * 3.0).clamp(0.0, 1.0) * 0.9;
                let snow = ((hn - 0.70) / 0.30).clamp(0.0, 1.0)
                    * (1.0 - slope * 2.0).max(0.0);
                let dirt = ((hn - 0.25) / 0.45).clamp(0.0, 1.0)
                    * (1.0 - slope * 2.0).max(0.0)
                    * (1.0 - snow);

                let sum = (grass + rock + snow + dirt).max(1e-4);
                let idx = ((z * res + x) * 4) as usize;
                self.splat[idx]     = ((grass / sum) * 255.0) as u8;
                self.splat[idx + 1] = ((rock  / sum) * 255.0) as u8;
                self.splat[idx + 2] = ((snow  / sum) * 255.0) as u8;
                self.splat[idx + 3] = ((dirt  / sum) * 255.0) as u8;
            }
        }
    }
}

// ============================================================
// Mesh generation / update
// ============================================================

pub fn generate_terrain_mesh(device: &wgpu::Device, heightmap: &Heightmap) -> Mesh {
    let res = heightmap.resolution;
    let stride = res + 1;
    let size = heightmap.size;
    let eps = size / res as f32;

    let vertex_count = (stride * stride) as usize;
    let index_count = (res * res * 6) as usize;

    let mut vertices = Vec::with_capacity(vertex_count);
    let mut indices = Vec::with_capacity(index_count);

    for z in 0..=res {
        for x in 0..=res {
            let u = x as f32 / res as f32;
            let v = z as f32 / res as f32;
            let wx = (u - 0.5) * size;
            let wz = (v - 0.5) * size;
            let h = heightmap.sample(wx, wz);

            let h_l = heightmap.sample(wx - eps, wz);
            let h_r = heightmap.sample(wx + eps, wz);
            let h_d = heightmap.sample(wx, wz - eps);
            let h_u = heightmap.sample(wx, wz + eps);
            let dx = (h_r - h_l) / (2.0 * eps);
            let dz = (h_u - h_d) / (2.0 * eps);
            let n = Vec3::new(-dx, 1.0, -dz).normalize_or(Vec3::Y);

            vertices.push(Vertex3D::static_vertex(
                [wx, h, wz],
                n.to_array(),
                [u, v],
                [1.0, 1.0, 1.0, 1.0],
            ));
        }
    }

    for z in 0..res {
        for x in 0..res {
            let a = z * stride + x;
            let b = a + stride;
            indices.extend_from_slice(&[a, b, a + 1, a + 1, b, b + 1]);
        }
    }

    Mesh::from_raw_parts(device, &vertices, &indices, "terrain", false)
}

/// Инкрементально обновляет позиции и нормали вершин в заданном
/// прямоугольнике (в вершинном пространстве 0..=resolution).
///
/// UV, color, joints, weights сохраняются. Индексы не трогаются.
pub fn update_terrain_mesh_region(
    queue: &wgpu::Queue,
    mesh: &mut Mesh,
    heightmap: &Heightmap,
    x0: u32, z0: u32, x1: u32, z1: u32,
) {
    let res = heightmap.resolution;
    let stride = res + 1;
    let size = heightmap.size;
    let eps = size / res as f32;
    let vtx_size = std::mem::size_of::<Vertex3D>() as u64;

    let x0 = x0.min(res);
    let x1 = x1.min(res);
    let z0 = z0.min(res);
    let z1 = z1.min(res);

    if x0 > x1 || z0 > z1 { return; }
    if mesh.cpu_vertices.len() < (stride * stride) as usize { return; }

    for z in z0..=z1 {
        let row_len = (x1 - x0 + 1) as usize;
        let mut row: Vec<Vertex3D> = Vec::with_capacity(row_len);
        for x in x0..=x1 {
            let u = x as f32 / res as f32;
            let v = z as f32 / res as f32;
            let wx = (u - 0.5) * size;
            let wz = (v - 0.5) * size;
            let h = heightmap.sample(wx, wz);

            let h_l = heightmap.sample(wx - eps, wz);
            let h_r = heightmap.sample(wx + eps, wz);
            let h_d = heightmap.sample(wx, wz - eps);
            let h_u = heightmap.sample(wx, wz + eps);
            let dx = (h_r - h_l) / (2.0 * eps);
            let dz = (h_u - h_d) / (2.0 * eps);
            let n = Vec3::new(-dx, 1.0, -dz).normalize_or(Vec3::Y);

            let idx = (z * stride + x) as usize;
            let mut vtx = mesh.cpu_vertices[idx];
            vtx.position = [wx, h, wz];
            vtx.normal = n.to_array();
            mesh.cpu_vertices[idx] = vtx;
            row.push(vtx);
        }
        let first = (z * stride + x0) as u64;
        let off = first * vtx_size;
        queue.write_buffer(&mesh.vertex_buffer, off, bytemuck::cast_slice(&row));
    }

    mesh.invalidate_bvh();
    mesh.rebuild_triangles_from_cpu();
}

// ============================================================
// Texture bake / update
// ============================================================

pub fn generate_terrain_texture(heightmap: &Heightmap, size: u32) -> Vec<u8> {
    let size = size.max(16);
    let mut data = vec![0u8; (size * size * 4) as usize];
    for y in 0..size {
        for x in 0..size {
            let u = (x as f32 + 0.5) / size as f32;
            let v = (y as f32 + 0.5) / size as f32;
            let wx = (u - 0.5) * heightmap.size;
            let wz = (v - 0.5) * heightmap.size;
            let splat = heightmap.splat_at(wx, wz);
            let color = blend_palette(&heightmap.palette, splat);
            let idx = ((y * size + x) * 4) as usize;
            data[idx..idx + 4].copy_from_slice(&color);
        }
    }
    data
}

/// Обновить прямоугольник `terrain_tex` из splat.
pub fn update_terrain_texture_region(
    queue: &wgpu::Queue,
    texture: &wgpu::Texture,
    heightmap: &Heightmap,
    tex_size: u32,
    region: BrushRegion,
) {
    if tex_size == 0 { return; }
    let res = heightmap.resolution.max(2);
    let scale = tex_size as f32 / (res - 1) as f32;

    let tx0 = ((region.x0 as f32 - 1.0) * scale).floor().max(0.0) as u32;
    let tz0 = ((region.z0 as f32 - 1.0) * scale).floor().max(0.0) as u32;
    let tx1 = ((region.x1 as f32 + 1.0) * scale).ceil().min(tex_size as f32) as u32;
    let tz1 = ((region.z1 as f32 + 1.0) * scale).ceil().min(tex_size as f32) as u32;

    if tx0 >= tx1 || tz0 >= tz1 { return; }
    let tw = tx1 - tx0;
    let th = tz1 - tz0;

    let mut data = vec![0u8; (tw * th * 4) as usize];
    for ty in 0..th {
        for tx in 0..tw {
            let px = tx0 + tx;
            let py = tz0 + ty;
            let u = (px as f32 + 0.5) / tex_size as f32;
            let v = (py as f32 + 0.5) / tex_size as f32;
            let wx = (u - 0.5) * heightmap.size;
            let wz = (v - 0.5) * heightmap.size;
            let splat = heightmap.splat_at(wx, wz);
            let color = blend_palette(&heightmap.palette, splat);
            let idx = ((ty * tw + tx) * 4) as usize;
            data[idx..idx + 4].copy_from_slice(&color);
        }
    }

    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d { x: tx0, y: tz0, z: 0 },
            aspect: wgpu::TextureAspect::All,
        },
        &data,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(tw * 4),
            rows_per_image: Some(th),
        },
        wgpu::Extent3d { width: tw, height: th, depth_or_array_layers: 1 },
    );
}

// ============================================================
// Ray → heightmap (marching + binary refine)
// ============================================================

pub fn ray_heightmap(
    heightmap: &Heightmap,
    origin: Vec3,
    dir: Vec3,
    max_dist: f32,
) -> Option<Vec3> {
    let dir = dir.normalize_or_zero();
    if dir.length_squared() < 1e-8 { return None; }

    let step = (heightmap.size / heightmap.resolution as f32) * 0.5;
    let step = step.max(0.05);

    let mut t_prev = 0.0;
    let mut diff_prev: Option<f32> = None;
    let mut t = 0.0;

    while t < max_dist {
        let p = origin + dir * t;
        let diff = p.y - heightmap.sample(p.x, p.z);

        if let Some(dp) = diff_prev {
            if diff <= 0.0 && dp > 0.0 {
                // Бинарный поиск между t_prev и t.
                let mut a = t_prev;
                let mut b = t;
                for _ in 0..20 {
                    let m = (a + b) * 0.5;
                    let pm = origin + dir * m;
                    let dm = pm.y - heightmap.sample(pm.x, pm.z);
                    if dm > 0.0 { a = m; } else { b = m; }
                }
                let hit = origin + dir * ((a + b) * 0.5);
                if heightmap.contains(hit.x, hit.z) {
                    return Some(hit);
                }
            }
        }

        diff_prev = Some(diff);
        t_prev = t;
        t += step;
    }
    None
}

// ============================================================
// Noise
// ============================================================

#[inline]
fn hash_u32(mut x: u32) -> u32 {
    x = x.wrapping_mul(0x9E37_79B1);
    x ^= x >> 16;
    x = x.wrapping_mul(0x85EB_CA6B);
    x ^= x >> 13;
    x
}

fn value_noise_2d(x: f32, y: f32, seed: u32) -> f32 {
    let xi = x.floor() as i32;
    let yi = y.floor() as i32;
    let xf = x - xi as f32;
    let yf = y - yi as f32;

    let h = |px: i32, py: i32| -> f32 {
        let n = hash_u32(
            (px as u32).wrapping_mul(374_761_393)
                ^ (py as u32).wrapping_mul(668_265_263)
                ^ seed,
        );
        (n & 0x00FF_FFFF) as f32 / 16_777_215.0
    };

    let v00 = h(xi, yi);
    let v10 = h(xi + 1, yi);
    let v01 = h(xi, yi + 1);
    let v11 = h(xi + 1, yi + 1);

    let sx = xf * xf * (3.0 - 2.0 * xf);
    let sy = yf * yf * (3.0 - 2.0 * yf);

    let a = v00 + (v10 - v00) * sx;
    let b = v01 + (v11 - v01) * sx;
    a + (b - a) * sy
}