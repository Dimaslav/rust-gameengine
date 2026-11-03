//! Data-driven вендоры.
//!
//! # Формат
//!
//! `assets/vendors/<id>.ron`:
//!
//! ```ron
//! (
//!     id: "blacksmith",
//!     title: "Blacksmith",
//!     items: [
//!         (item_id: "health_potion", price: 25, stock: -1),
//!         (item_id: "steel_sword",  price: 150, stock: 1),
//!     ],
//! )
//! ```
//!
//! `stock = -1` — бесконечный запас. Иначе — сколько штук в наличии.
//!
//! # Компонент
//!
//! `VendorNpc` на entity NPC даёт ссылку на `VendorDef::id`.
//!
//! # Runtime
//!
//! `VendorSession` — состояние открытого вендора: сколько чего
//! осталось после покупок. Привязан к конкретному `npc_entity`.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::ecs::Entity;

// ============================================================
// VendorDef
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorItem {
    pub item_id: String,
    pub price: u32,
    /// `-1` = бесконечно. Иначе — сколько штук доступно.
    #[serde(default = "default_stock")]
    pub stock: i32,
}

fn default_stock() -> i32 { -1 }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorDef {
    pub id: String,
    pub title: String,
    pub items: Vec<VendorItem>,
}

impl VendorDef {
    pub fn total_items(&self) -> usize {
        self.items.len()
    }
}

// ============================================================
// Компонент
// ============================================================

/// NPC-вендор. Ссылается на `VendorDef::id` из `VendorRegistry`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VendorNpc {
    pub vendor_id: String,
}

// ============================================================
// Registry
// ============================================================

pub struct VendorRegistry {
    vendors: HashMap<String, VendorDef>,
}

impl VendorRegistry {
    pub fn empty() -> Self { Self { vendors: HashMap::new() } }

    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut vendors = HashMap::new();
        if !dir.exists() {
            log::warn!("vendors: dir '{}' not found, vendors disabled", dir.display());
            return Ok(Self { vendors });
        }

        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("read dir {}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
            .collect();
        paths.sort();

        for p in paths {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("read {}", p.display()))?;
            let v: VendorDef = ron::from_str(&text)
                .with_context(|| format!("parse {}", p.display()))?;
            log::info!(
                "vendor: '{}' ({} items) from {}",
                v.id, v.items.len(), p.display()
            );
            vendors.insert(v.id.clone(), v);
        }
        Ok(Self { vendors })
    }

    pub fn get(&self, id: &str) -> Option<&VendorDef> {
        self.vendors.get(id)
    }

    pub fn len(&self) -> usize { self.vendors.len() }
    pub fn is_empty(&self) -> bool { self.vendors.is_empty() }
}

impl Default for VendorRegistry {
    fn default() -> Self { Self::empty() }
}

// ============================================================
// Session (runtime state открытого вендора)
// ============================================================

pub struct VendorSession {
    pub vendor_id: String,
    /// Параллельно `VendorDef::items`. `-1` — бесконечно.
    pub stock: Vec<i32>,
    /// NPC, с которым торгуем (для закрытия/контекста).
    pub npc_entity: Entity,
    /// Сколько золота потрачено/получено в этой сессии (для UI).
    pub spent: u32,
    pub earned: u32,
}

impl VendorSession {
    pub fn new(def: &VendorDef, npc_entity: Entity) -> Self {
        Self {
            vendor_id: def.id.clone(),
            stock: def.items.iter().map(|i| i.stock).collect(),
            npc_entity,
            spent: 0,
            earned: 0,
        }
    }

    /// Есть ли ещё в наличии `idx` (с учётом -1 = ∞).
    pub fn has_stock(&self, idx: usize) -> bool {
        match self.stock.get(idx).copied() {
            Some(s) => s != 0,
            None => false,
        }
    }

    /// Списать 1 единицу.
    pub fn take_one(&mut self, idx: usize) {
        if let Some(s) = self.stock.get_mut(idx)
            && *s > 0 {
                *s -= 1;
            }
    }
}