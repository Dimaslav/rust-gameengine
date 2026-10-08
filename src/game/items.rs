//! Item framework: data-driven предметы из RON.
//!
//! # Формат
//!
//! Каждый предмет — `.ron` файл в `assets/items/`. Загружается в
//! `ItemRegistry` при старте.
//!
//! # Категории
//!
//! * `Weapon` — оружие, ссылается на `weapon_id` из WeaponRegistry.
//! * `Armor` — броня, даёт бонусы к атрибутам.
//! * `Trinket` — аксессуар, даёт бонусы.
//! * `Consumable` — расходник (heal potion, ammo box).
//! * `Key` — квестовый ключ.
//! * `Misc` — прочее (золото, ресурсы).
//!
//! # Инвентарь
//!
//! `Inventory` — плоский `Vec<Option<ItemStack>>` длины `capacity`.
//! `Equipment` — три слота: weapon, armor, trinket. Их бонусы
//! складываются с бонусами от атрибутов игрока.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::Path;

use super::stats::Attributes;

// ============================================================
// ItemKind
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ItemKind {
    Weapon,
    Armor,
    Trinket,
    Consumable,
    Key,
    Misc,
}

impl ItemKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Weapon => "Weapon",
            Self::Armor => "Armor",
            Self::Trinket => "Trinket",
            Self::Consumable => "Consumable",
            Self::Key => "Key",
            Self::Misc => "Misc",
        }
    }
    pub fn icon(self) -> &'static str {
        match self {
            Self::Weapon => "⚔",
            Self::Armor => "🛡",
            Self::Trinket => "💍",
            Self::Consumable => "🧪",
            Self::Key => "🔑",
            Self::Misc => "📦",
        }
    }
}

// ============================================================
// Rarity
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Rarity {
    Common,
    Uncommon,
    Rare,
    Epic,
    Legendary,
}

impl Rarity {
    pub fn label(self) -> &'static str {
        match self {
            Self::Common => "Common",
            Self::Uncommon => "Uncommon",
            Self::Rare => "Rare",
            Self::Epic => "Epic",
            Self::Legendary => "Legendary",
        }
    }
    /// RGB цвет для UI.
    pub fn color(self) -> [f32; 4] {
        match self {
            Self::Common =>    [0.75, 0.75, 0.75, 1.0],
            Self::Uncommon =>  [0.35, 0.85, 0.35, 1.0],
            Self::Rare =>      [0.35, 0.55, 1.00, 1.0],
            Self::Epic =>      [0.75, 0.35, 1.00, 1.0],
            Self::Legendary => [1.00, 0.70, 0.20, 1.0],
        }
    }
}

impl Default for Rarity {
    fn default() -> Self { Self::Common }
}

// ============================================================
// Item — data-driven
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub id: String,
    pub name: String,
    pub kind: ItemKind,
    #[serde(default)]
    pub rarity: Rarity,
    #[serde(default)]
    pub description: String,

    /// Для Weapon: id из WeaponRegistry.
    #[serde(default)]
    pub weapon_id: Option<String>,

    /// Бонусы к атрибутам при экипировке.
    #[serde(default)]
    pub bonus_strength: i32,
    #[serde(default)]
    pub bonus_dexterity: i32,
    #[serde(default)]
    pub bonus_intelligence: i32,
    #[serde(default)]
    pub bonus_vitality: i32,
    #[serde(default)]
    pub bonus_luck: i32,

    /// Для Consumable: сколько HP восстанавливает при использовании.
    #[serde(default)]
    pub heal_amount: f32,

    /// Для Misc: сколько золота даёт (если это золото).
    #[serde(default)]
    pub gold_value: u32,

    /// Цена продажи (для будущего vendor).
    #[serde(default)]
    pub sell_price: u32,

    /// Максимальный стек. 1 = не стакуется.
    #[serde(default = "default_stack")]
    pub max_stack: u32,

    /// Может быть экипирован (только Weapon/Armor/Trinket).
    #[serde(default)]
    pub equippable: bool,
}

fn default_stack() -> u32 { 1 }

impl Item {
    pub fn is_stackable(&self) -> bool {
        self.max_stack > 1
    }
}

// ============================================================
// ItemStack
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemStack {
    pub item_id: String,
    pub count: u32,
}

impl ItemStack {
    pub fn new(id: impl Into<String>, count: u32) -> Self {
        Self { item_id: id.into(), count }
    }
}

// ============================================================
// Registry
// ============================================================

pub struct ItemRegistry {
    items: Vec<Item>,
}

impl ItemRegistry {
    pub fn empty() -> Self { Self { items: Vec::new() } }

    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("read dir {}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
            .collect();
        paths.sort();

        let mut items = Vec::with_capacity(paths.len());
        for p in paths {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("read {}", p.display()))?;
            let item: Item = ron::from_str(&text)
                .with_context(|| format!("parse {}", p.display()))?;
            log::info!(
                "item: loaded '{}' ({}) from {}",
                item.name, item.kind.label(), p.display()
            );
            items.push(item);
        }
        Ok(Self { items })
    }

    /// Fallback: встроенный набор.
    pub fn default_set() -> Self {
        Self {
            items: vec![
                Item {
                    id: "gold_coin".into(),
                    name: "Gold Coin".into(),
                    kind: ItemKind::Misc,
                    rarity: Rarity::Common,
                    description: "Universal currency".into(),
                    weapon_id: None,
                    bonus_strength: 0, bonus_dexterity: 0,
                    bonus_intelligence: 0, bonus_vitality: 0, bonus_luck: 0,
                    heal_amount: 0.0, gold_value: 1, sell_price: 0,
                    max_stack: 9999, equippable: false,
                },
                Item {
                    id: "health_potion".into(),
                    name: "Health Potion".into(),
                    kind: ItemKind::Consumable,
                    rarity: Rarity::Common,
                    description: "Restores 40 HP".into(),
                    weapon_id: None,
                    bonus_strength: 0, bonus_dexterity: 0,
                    bonus_intelligence: 0, bonus_vitality: 0, bonus_luck: 0,
                    heal_amount: 40.0, gold_value: 0, sell_price: 15,
                    max_stack: 5, equippable: false,
                },
                Item {
                    id: "leather_armor".into(),
                    name: "Leather Armor".into(),
                    kind: ItemKind::Armor,
                    rarity: Rarity::Uncommon,
                    description: "+3 Vitality, +1 Dexterity".into(),
                    weapon_id: None,
                    bonus_strength: 0, bonus_dexterity: 1,
                    bonus_intelligence: 0, bonus_vitality: 3, bonus_luck: 0,
                    heal_amount: 0.0, gold_value: 0, sell_price: 60,
                    max_stack: 1, equippable: true,
                },
                Item {
                    id: "lucky_charm".into(),
                    name: "Lucky Charm".into(),
                    kind: ItemKind::Trinket,
                    rarity: Rarity::Rare,
                    description: "+5 Luck — higher crit chance".into(),
                    weapon_id: None,
                    bonus_strength: 0, bonus_dexterity: 0,
                    bonus_intelligence: 0, bonus_vitality: 0, bonus_luck: 5,
                    heal_amount: 0.0, gold_value: 0, sell_price: 150,
                    max_stack: 1, equippable: true,
                },
            ],
        }
    }

    pub fn len(&self) -> usize { self.items.len() }
    pub fn is_empty(&self) -> bool { self.items.is_empty() }

    pub fn get_by_id(&self, id: &str) -> Option<&Item> {
        self.items.iter().find(|i| i.id == id)
    }
    pub fn get_by_index(&self, idx: usize) -> Option<&Item> {
        self.items.get(idx)
    }
    pub fn iter(&self) -> impl Iterator<Item = &Item> {
        self.items.iter()
    }
}

// ============================================================
// Loot tables
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LootEntry {
    pub item_id: String,
    /// 0.0..=1.0. Шанс, что предмет выпадет.
    pub chance: f32,
    pub min_count: u32,
    pub max_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct LootTable {
    pub id: String,
    pub entries: Vec<LootEntry>,
    /// Гарантированный дроп золота (min..max).
    #[serde(default)]
    pub gold_min: u32,
    #[serde(default)]
    pub gold_max: u32,
}

pub struct LootRegistry {
    tables: Vec<LootTable>,
}

impl LootRegistry {
    pub fn empty() -> Self { Self { tables: Vec::new() } }

    pub fn load_from_dir(dir: impl AsRef<Path>) -> Result<Self> {
        let dir = dir.as_ref();
        let mut paths: Vec<_> = std::fs::read_dir(dir)
            .with_context(|| format!("read dir {}", dir.display()))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("ron"))
            .collect();
        paths.sort();

        let mut tables = Vec::with_capacity(paths.len());
        for p in paths {
            let text = std::fs::read_to_string(&p)
                .with_context(|| format!("read {}", p.display()))?;
            let table: LootTable = ron::from_str(&text)
                .with_context(|| format!("parse {}", p.display()))?;
            log::info!(
                "loot: table '{}' ({} entries) from {}",
                table.id, table.entries.len(), p.display()
            );
            tables.push(table);
        }
        Ok(Self { tables })
    }

    pub fn default_set() -> Self {
        Self {
            tables: vec![
                LootTable {
                    id: "enemy_patrol".into(),
                    entries: vec![
                        LootEntry { item_id: "gold_coin".into(), chance: 1.0, min_count: 2, max_count: 8 },
                        LootEntry { item_id: "health_potion".into(), chance: 0.25, min_count: 1, max_count: 1 },
                    ],
                    gold_min: 0, gold_max: 0,
                },
                LootTable {
                    id: "enemy_elite".into(),
                    entries: vec![
                        LootEntry { item_id: "gold_coin".into(), chance: 1.0, min_count: 10, max_count: 30 },
                        LootEntry { item_id: "health_potion".into(), chance: 0.5, min_count: 1, max_count: 2 },
                        LootEntry { item_id: "leather_armor".into(), chance: 0.15, min_count: 1, max_count: 1 },
                    ],
                    gold_min: 0, gold_max: 0,
                },
                LootTable {
                    id: "enemy_boss".into(),
                    entries: vec![
                        LootEntry { item_id: "gold_coin".into(), chance: 1.0, min_count: 100, max_count: 250 },
                        LootEntry { item_id: "lucky_charm".into(), chance: 1.0, min_count: 1, max_count: 1 },
                        LootEntry { item_id: "leather_armor".into(), chance: 0.8, min_count: 1, max_count: 1 },
                    ],
                    gold_min: 0, gold_max: 0,
                },
            ],
        }
    }

    pub fn get(&self, id: &str) -> Option<&LootTable> {
        self.tables.iter().find(|t| t.id == id)
    }

    pub fn len(&self) -> usize { self.tables.len() }
    pub fn is_empty(&self) -> bool { self.tables.is_empty() }
}

/// Ролл лут-таблицы. Возвращает Vec<ItemStack> выпавшего.
pub fn roll_loot(table: &LootTable) -> Vec<ItemStack> {
    let mut out = Vec::new();
    for entry in &table.entries {
        if rand01() < entry.chance {
            let count = if entry.max_count > entry.min_count {
                entry.min_count + (rand01() * (entry.max_count - entry.min_count + 1) as f32) as u32
            } else {
                entry.min_count
            };
            let count = count.max(1);
            out.push(ItemStack::new(entry.item_id.clone(), count));
        }
    }
    out
}

/// Простой LCG.
fn rand01() -> f32 {
    use std::cell::Cell;
    thread_local! {
        static S: Cell<u32> = const { Cell::new(0x1DEA_BEEF) };
    }
    S.with(|s| {
        let mut v = s.get();
        v = v.wrapping_mul(1664525).wrapping_add(1013904223);
        s.set(v);
        ((v >> 8) & 0xFFFFFF) as f32 / 16777215.0
    })
}

// ============================================================
// Inventory
// ============================================================

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Inventory {
    pub slots: Vec<Option<ItemStack>>,
    pub capacity: usize,
}

impl Inventory {
    pub fn new(capacity: usize) -> Self {
        Self {
            slots: vec![None; capacity],
            capacity,
        }
    }

    /// Добавить предмет. Если стакуется и уже есть в слоте — увеличит
    /// count. Возвращает остаток (сколько не поместилось).
    pub fn add(&mut self, registry: &ItemRegistry, stack: ItemStack) -> u32 {
        let Some(item) = registry.get_by_id(&stack.item_id) else {
            return stack.count;
        };
        let max_stack = item.max_stack.max(1);
        let mut remaining = stack.count;

        // 1. Сначала пытаемся дополнить существующие стеки.
        if item.is_stackable() {
            for slot in self.slots.iter_mut() {
                if remaining == 0 { break; }
                if let Some(s) = slot {
                    if s.item_id == stack.item_id && s.count < max_stack {
                        let can_add = max_stack - s.count;
                        let add = can_add.min(remaining);
                        s.count += add;
                        remaining -= add;
                    }
                }
            }
        }

        // 2. Затем кладём в пустые слоты.
        while remaining > 0 {
            let Some(empty_idx) = self.slots.iter().position(|s| s.is_none()) else {
                break;
            };
            let put = remaining.min(max_stack);
            self.slots[empty_idx] = Some(ItemStack::new(stack.item_id.clone(), put));
            remaining -= put;
        }

        remaining
    }

    /// Убрать 1 единицу из слота idx. Возвращает убранный stack.
    pub fn take_one(&mut self, idx: usize) -> Option<ItemStack> {
        let slot = self.slots.get_mut(idx)?;
        let s = slot.as_mut()?;
        s.count -= 1;
        let out = ItemStack::new(s.item_id.clone(), 1);
        if s.count == 0 {
            *slot = None;
        }
        Some(out)
    }

    /// Полностью очистить слот idx.
    pub fn take_slot(&mut self, idx: usize) -> Option<ItemStack> {
        self.slots.get_mut(idx)?.take()
    }

    pub fn is_full(&self) -> bool {
        self.slots.iter().all(|s| s.is_some())
    }

    /// Общее количество предметов с данным id во всём инвентаре.
    pub fn count_of(&self, item_id: &str) -> u32 {
        self.slots.iter().filter_map(|s| s.as_ref())
            .filter(|s| s.item_id == item_id)
            .map(|s| s.count)
            .sum()
    }

    /// Убрать N единиц предмета (для будущего крафта / траты).
    pub fn remove_by_id(&mut self, item_id: &str, mut count: u32) -> u32 {
        for slot in self.slots.iter_mut() {
            if count == 0 { break; }
            let Some(s) = slot.as_mut() else { continue; };
            if s.item_id != item_id { continue; }
            let take = s.count.min(count);
            s.count -= take;
            count -= take;
            if s.count == 0 {
                *slot = None;
            }
        }
        count
    }
}

// ============================================================
// Equipment
// ============================================================

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Equipment {
    pub weapon_id: Option<String>,
    pub armor_id: Option<String>,
    pub trinket_id: Option<String>,
}

impl Equipment {
    /// Суммарные бонусы от экипировки. Складываются с базовыми
    /// атрибутами игрока.
    pub fn bonus_attributes(&self, registry: &ItemRegistry) -> Attributes {
        let mut attrs = Attributes {
            strength: 0, dexterity: 0, intelligence: 0, vitality: 0, luck: 0,
        };
        for id_opt in [&self.weapon_id, &self.armor_id, &self.trinket_id] {
            let Some(id) = id_opt else { continue };
            let Some(item) = registry.get_by_id(id) else { continue };
            attrs.strength = attrs.strength.saturating_add_signed(item.bonus_strength);
            attrs.dexterity = attrs.dexterity.saturating_add_signed(item.bonus_dexterity);
            attrs.intelligence = attrs.intelligence.saturating_add_signed(item.bonus_intelligence);
            attrs.vitality = attrs.vitality.saturating_add_signed(item.bonus_vitality);
            attrs.luck = attrs.luck.saturating_add_signed(item.bonus_luck);
        }
        attrs
    }
}