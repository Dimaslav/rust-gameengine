//! Weapon framework: data-driven оружие из RON.
//!
//! Каждый ствол — `.ron` файл в `assets/weapons/`. Движок загружает
//! их при старте в `WeaponRegistry`. Рантайм-состояние (патроны в
//! магазине, кулдаун, идёт ли перезарядка) хранится отдельно в
//! `WeaponRuntime` — по одному на каждый ствол.
//!
//! Расчёт стрельбы:
//!   * `spread_hip` или `spread_ads` — конус разброса в градусах.
//!   * `pellets` — сколько лучей вылетает за один выстрел (1 для
//!     всего кроме дробовика; 8–12 для shotgun).
//!   * `damage` — урон **на одну дробинку**. Для дробовика это
//!     маленький урон × 8 дробинок.
//!   * `fire_rate` — выстрелов в секунду. `cooldown = 1.0 / fire_rate`.
//!   * `recoil_up` / `recoil_side` — отдача в радианах. `recoil_side`
//!     симметричный (±), умножается на случайный знак.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

// ============================================================
// WeaponKind
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WeaponKind {
    Pistol,
    SMG,
    Shotgun,
    Sniper,
}

impl WeaponKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Pistol => "Pistol",
            Self::SMG => "SMG",
            Self::Shotgun => "Shotgun",
            Self::Sniper => "Sniper",
        }
    }
}

// ============================================================
// Weapon (data-driven, из RON)
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Weapon {
    pub id: String,
    pub name: String,
    pub kind: WeaponKind,

    /// Урон за одну дробинку (pellet).
    pub damage: f32,

    /// Выстрелов в секунду. `cooldown = 1.0 / fire_rate`.
    pub fire_rate: f32,

    /// Размер магазина.
    pub mag_size: u32,

    /// Сколько патронов в резерве при старте.
    #[serde(default = "default_reserve")]
    pub start_reserve: u32,

    /// Время перезарядки в секундах.
    pub reload_time: f32,

    /// Автоматический огонь (удержание ЛКМ).
    /// `false` — по одному клику.
    pub auto: bool,

    /// Разброс от бедра в градусах.
    pub spread_hip: f32,

    /// Разброс в прицеле (ПКМ) в градусах.
    pub spread_ads: f32,

    /// FOV множитель в ADS. 1.0 = без изменения, 0.5 = zoom ×2.
    #[serde(default = "default_ads_mult")]
    pub ads_fov_mult: f32,

    /// Отдача по вертикали в радианах (подброс вверх).
    #[serde(default)]
    pub recoil_up: f32,

    /// Отдача по горизонтали в радианах (симметричная).
    #[serde(default)]
    pub recoil_side: f32,

    /// Сколько дробинок вылетает за выстрел. 1 для всего кроме shotgun.
    #[serde(default = "default_pellets")]
    pub pellets: u32,

    /// Дальность стрельбы в метрах.
    pub range: f32,

    /// Сколько патронов даёт один ammo_pickup.
    #[serde(default = "default_ammo_pickup")]
    pub ammo_pickup: u32,
}

fn default_reserve() -> u32 { 60 }
fn default_ads_mult() -> f32 { 1.0 }
fn default_pellets() -> u32 { 1 }
fn default_ammo_pickup() -> u32 { 30 }

impl Weapon {
    pub fn cooldown(&self) -> f32 {
        if self.fire_rate <= 0.0 { 0.1 } else { 1.0 / self.fire_rate }
    }
}

// ============================================================
// WeaponRuntime (per-weapon state)
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeaponRuntime {
    pub mag: u32,
    pub reserve: u32,

    #[serde(skip)]
    pub fire_cooldown: f32,
    #[serde(skip)]
    pub reloading: bool,
    #[serde(skip)]
    pub reload_timer: f32,
}

impl WeaponRuntime {
    pub fn new(w: &Weapon) -> Self {
        Self {
            mag: w.mag_size,
            reserve: w.start_reserve,
            fire_cooldown: 0.0,
            reloading: false,
            reload_timer: 0.0,
        }
    }

    pub fn can_fire(&self) -> bool {
        !self.reloading && self.mag > 0 && self.fire_cooldown <= 0.0
    }

    pub fn can_reload(&self) -> bool {
        !self.reloading && self.mag < 100 && self.reserve > 0
    }

    pub fn start_reload(&mut self, w: &Weapon) {
        if !self.can_reload() { return; }
        self.reloading = true;
        self.reload_timer = w.reload_time;
    }

    pub fn tick(&mut self, dt: f32, w: &Weapon) {
        self.fire_cooldown = (self.fire_cooldown - dt).max(0.0);
        if self.reloading {
            self.reload_timer -= dt;
            if self.reload_timer <= 0.0 {
                self.reloading = false;
                self.reload_timer = 0.0;
                let need = w.mag_size.saturating_sub(self.mag);
                let take = need.min(self.reserve);
                self.mag += take;
                self.reserve -= take;
            }
        }
    }

    pub fn reload_progress(&self, w: &Weapon) -> f32 {
        if !self.reloading || w.reload_time <= 0.0 { return 0.0; }
        1.0 - (self.reload_timer / w.reload_time).clamp(0.0, 1.0)
    }
}

// ============================================================
// Registry
// ============================================================

pub struct WeaponRegistry {
    weapons: Vec<Weapon>,
}

impl WeaponRegistry {
    pub fn empty() -> Self {
        Self { weapons: Vec::new() }
    }

    /// Загрузка всех `*.ron` из папки. Файлы сортируются по имени.
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("read dir {}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
            .collect();
        paths.sort();

        let mut weapons = Vec::with_capacity(paths.len());
        for p in paths {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("read {}", p.display()))?;
            let w: Weapon = ron::from_str(&text)
                .with_context(|| format!("parse {}", p.display()))?;
            log::info!(
                "weapon: loaded '{}' ({}) from {}",
                w.name, w.kind.label(), p.display()
            );
            weapons.push(w);
        }
        Ok(Self { weapons })
    }

    /// Fallback: если папка `assets/weapons/` пуста — регистрируем
    /// встроенный набор из 4 стволов.
    pub fn default_set() -> Self {
        Self {
            weapons: vec![
                Weapon {
                    id: "pistol".into(), name: "Pistol".into(),
                    kind: WeaponKind::Pistol,
                    damage: 25.0, fire_rate: 6.0, mag_size: 12, start_reserve: 48,
                    reload_time: 1.2, auto: false,
                    spread_hip: 1.5, spread_ads: 0.3, ads_fov_mult: 0.85,
                    recoil_up: 0.010, recoil_side: 0.002,
                    pellets: 1, range: 100.0, ammo_pickup: 24,
                },
                Weapon {
                    id: "smg".into(), name: "SMG".into(),
                    kind: WeaponKind::SMG,
                    damage: 12.0, fire_rate: 14.0, mag_size: 30, start_reserve: 120,
                    reload_time: 1.6, auto: true,
                    spread_hip: 3.0, spread_ads: 1.2, ads_fov_mult: 0.9,
                    recoil_up: 0.006, recoil_side: 0.004,
                    pellets: 1, range: 60.0, ammo_pickup: 60,
                },
                Weapon {
                    id: "shotgun".into(), name: "Shotgun".into(),
                    kind: WeaponKind::Shotgun,
                    damage: 12.0, fire_rate: 1.2, mag_size: 6, start_reserve: 24,
                    reload_time: 2.2, auto: false,
                    spread_hip: 6.0, spread_ads: 3.5, ads_fov_mult: 1.0,
                    recoil_up: 0.030, recoil_side: 0.008,
                    pellets: 8, range: 25.0, ammo_pickup: 16,
                },
                Weapon {
                    id: "sniper".into(), name: "Sniper".into(),
                    kind: WeaponKind::Sniper,
                    damage: 120.0, fire_rate: 0.8, mag_size: 5, start_reserve: 20,
                    reload_time: 2.8, auto: false,
                    spread_hip: 5.0, spread_ads: 0.0, ads_fov_mult: 0.35,
                    recoil_up: 0.060, recoil_side: 0.003,
                    pellets: 1, range: 500.0, ammo_pickup: 10,
                },
            ],
        }
    }

    pub fn len(&self) -> usize { self.weapons.len() }
    pub fn is_empty(&self) -> bool { self.weapons.is_empty() }
    pub fn get(&self, idx: usize) -> Option<&Weapon> { self.weapons.get(idx) }
    pub fn iter(&self) -> impl Iterator<Item = &Weapon> { self.weapons.iter() }
}

// ============================================================
// Spread helper
// ============================================================

/// Простой thread-local LCG для случайного разброса.
fn rand01() -> f32 {
    use std::cell::Cell;
    thread_local! {
        static SEED: Cell<u32> = const { Cell::new(0xDEAD_BEEF) };
    }
    SEED.with(|s| {
        let mut v = s.get();
        v = v.wrapping_mul(1664525).wrapping_add(1013904223);
        s.set(v);
        ((v >> 8) & 0xFFFFFF) as f32 / 16777215.0
    })
}

/// Случайное направление в конусе `spread_deg` вокруг `dir`.
///
/// Возвращает нормализованный вектор. `spread_deg == 0` → возвращает
/// `dir` как есть.
pub fn apply_spread(dir: glam::Vec3, spread_deg: f32) -> glam::Vec3 {
    use glam::Vec3;
    if spread_deg <= 1e-6 { return dir; }

    let rad = spread_deg.to_radians();
    let cos_max = rad.cos();

    // Случайная точка в единичном конусе вокруг (0, 0, 1).
    let cos_t = 1.0 - rand01() * (1.0 - cos_max);
    let sin_t = (1.0 - cos_t * cos_t).sqrt();
    let phi = rand01() * std::f32::consts::TAU;

    // Локальный ортонормированный базис вокруг `dir`.
    let up = if dir.y.abs() > 0.99 { Vec3::X } else { Vec3::Y };
    let t1 = up.cross(dir).normalize_or(Vec3::X);
    let t2 = dir.cross(t1);

    let offset = t1 * (sin_t * phi.cos()) + t2 * (sin_t * phi.sin()) + dir * cos_t;
    offset.normalize_or(dir)
}