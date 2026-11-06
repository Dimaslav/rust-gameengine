//! Data-driven crafting.
//!
//! # Формат
//!
//! `assets/recipes/<id>.ron`:
//!
//! ```ron
//! (
//!     id: "forge_steel_sword",
//!     title: "Steel Sword",
//!     category: "Weapons",
//!     inputs: [
//!         (item_id: "ruby_gem", count: 2),
//!     ],
//!     outputs: [
//!         (item_id: "steel_sword", count: 1),
//!     ],
//!     gold_cost: 50,
//! )
//! ```
//!
//! # Session
//!
//! `CraftingSession` — состояние открытого окна кузницы. Хранит `bench_id`
//! и id выбранного рецепта. `selected_recipe_id` = `None` — рецепт не
//! выбран, но окно открыто (показывается только список).

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::ecs::Entity;

// ============================================================
// RecipeDef
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeInput {
    pub item_id: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeOutput {
    pub item_id: String,
    pub count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecipeDef {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub category: String,
    pub inputs: Vec<RecipeInput>,
    pub outputs: Vec<RecipeOutput>,
    #[serde(default)]
    pub gold_cost: u32,
    #[serde(default = "default_level")]
    pub required_level: u32,
}

fn default_level() -> u32 { 1 }

impl RecipeDef {
    pub fn is_valid(&self) -> bool {
        !self.id.is_empty() && !self.outputs.is_empty()
    }
}

// ============================================================
// Registry
// ============================================================

pub struct CraftingRegistry {
    recipes: HashMap<String, RecipeDef>,
}

impl CraftingRegistry {
    pub fn empty() -> Self { Self { recipes: HashMap::new() } }

    /// FIX (P1.3): дубликаты id логируются и пропускаются (раньше
    /// `HashMap::insert` молча перезаписывал).
    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut recipes = HashMap::new();
        if !dir.exists() {
            log::warn!("recipes: dir '{}' not found, crafting disabled", dir.display());
            return Ok(Self { recipes });
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
            let r: RecipeDef = ron::from_str(&text)
                .with_context(|| format!("parse {}", p.display()))?;
            if !r.is_valid() {
                log::warn!("recipe '{}': invalid, skipped", p.display());
                continue;
            }
            if recipes.contains_key(&r.id) {
                log::error!(
                    "recipe: duplicate id '{}' in {} — skipped (already loaded)",
                    r.id, p.display()
                );
                continue;
            }
            log::info!(
                "recipe: '{}' [{}] ({} in → {} out) from {}",
                r.id, r.category, r.inputs.len(), r.outputs.len(), p.display()
            );
            recipes.insert(r.id.clone(), r);
        }
        Ok(Self { recipes })
    }

    pub fn get(&self, id: &str) -> Option<&RecipeDef> {
        self.recipes.get(id)
    }

    pub fn iter(&self) -> impl Iterator<Item = &RecipeDef> {
        self.recipes.values()
    }

    pub fn len(&self) -> usize { self.recipes.len() }
    pub fn is_empty(&self) -> bool { self.recipes.is_empty() }

    /// Рецепты, отсортированные по категории, затем по title.
    pub fn sorted(&self) -> Vec<&RecipeDef> {
        let mut v: Vec<&RecipeDef> = self.recipes.values().collect();
        v.sort_by(|a, b| {
            a.category.cmp(&b.category).then_with(|| a.title.cmp(&b.title))
        });
        v
    }
}

impl Default for CraftingRegistry {
    fn default() -> Self { Self::empty() }
}

// ============================================================
// Session
// ============================================================

pub struct CraftingSession {
    pub bench_id: String,
    /// Entity, с которым ассоциирован bench (NPC или anvil).
    pub bench_entity: Entity,
    /// Id выбранного рецепта (для details-панели).
    pub selected_recipe_id: Option<String>,
}

impl CraftingSession {
    pub fn new(
        bench_id: impl Into<String>,
        bench_entity: Entity,
        registry: &CraftingRegistry,
    ) -> Self {
        let first = registry.sorted().first().map(|r| r.id.clone());
        Self {
            bench_id: bench_id.into(),
            bench_entity,
            selected_recipe_id: first,
        }
    }
}