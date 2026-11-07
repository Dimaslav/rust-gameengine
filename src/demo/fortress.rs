//! "The Fallen Citadel" — 5-актовая сюжетная кампания.

use std::any::Any;
use std::collections::{HashMap, HashSet};

use glam::{Mat4, Quat, Vec3};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::ecs::{Entity, System, World};
use crate::engine::{Game, Input, InputMap, Key, ShotFired, WorldLoadRequest};
use crate::game::ai::{AiAgent, AiState, AiTarget, DebugPath};
use crate::game::animation::{AnimationEventTriggered, AnimationEvents, AnimationRuntime};
use crate::game::audio::{AudioBus, AudioSource};
use crate::game::components::*;
use crate::game::decals::Decal;
use crate::game::dialogs::{DialogEffect, DialogGraph, DialogNpc, DialogSession};
use crate::game::items::{
    roll_loot, Equipment, Inventory, ItemKind, ItemRegistry, ItemStack, LootRegistry,
};
use crate::game::vendors::{VendorDef, VendorNpc, VendorRegistry, VendorSession};
use crate::game::crafting::{CraftingRegistry, CraftingSession, RecipeDef};
use crate::ui::vendor_ui::{self, VendorAction};
use crate::ui::crafting_ui::{self, CraftingAction};
use crate::game::rpg::GoldValue;
use crate::game::lights::{DirectionalLight, PointLight};
use crate::game::stats::{PlayerStats, STAT_DESCRIPTIONS, STAT_NAMES};
use crate::game::timers::{Timer, TimerFinished, TimerSystem};
use crate::game::weapons::{apply_spread, Weapon, WeaponRegistry, WeaponRuntime};
use crate::game::quests::{QuestManager, Reward};
use crate::physics::{Collider, RigidBody};
use crate::render::{
    skinning::AnimationClip, AlphaMode, Camera3D, DebugView, GpuLight, GpuPointLight,
    InstanceData, LineBatch, LineVertex, Material, Mesh, MeshDraw, PostFx, Renderer, Skeleton,
};
use crate::ui::Hud;
use crate::ui::inventory_ui::{
    self, InvDragState, InvLocation, InventoryAction,
};

use super::primitives::*;
use super::primitives::{stop_gate_if_reached, GateMotion};

// ============================================================
// Layout — координаты актов
// ============================================================

const ACT1_CENTER: Vec3 = Vec3::new(-60.0, 0.0, 0.0);
const ACT1_HALF: f32 = 10.0;

const ACT2_CENTER: Vec3 = Vec3::new(0.0, 0.0, 0.0);
const ACT2_HALF: f32 = 30.0;

const ACT3_CENTER: Vec3 = Vec3::new(55.0, 0.0, 0.0);
const ACT3_HALF_X: f32 = 15.0;
const ACT3_HALF_Z: f32 = 10.0;

const ACT4_CENTER: Vec3 = Vec3::new(55.0, -12.0, -40.0);
const ACT4_HALF: f32 = 20.0;

const ACT5_CENTER: Vec3 = Vec3::new(55.0, -12.0, -75.0);
const ACT5_HALF_X: f32 = 12.0;
const ACT5_HALF_Z: f32 = 8.0;

const WALL_H: f32 = 5.0;
const WALL_T: f32 = 0.6;

// ============================================================
// Layout-константы инвентаря (должны совпадать с inventory_ui)
// ============================================================

const INV_PANEL_W: f32 = 720.0;
const INV_PANEL_H: f32 = 520.0;
const INV_PAD: f32 = 20.0;
const INV_CELL: f32 = 64.0;
const INV_GAP: f32 = 6.0;
const INV_COLS: usize = 6;
const INV_EQ_SLOT_W: f32 = 220.0;
const INV_EQ_SLOT_H: f32 = 44.0;

// ============================================================
// Systems
// ============================================================

struct RotationSystem;
impl System for RotationSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        let entities: Vec<Entity> = world.query::<Spinner>().map(|(e, _)| e).collect();
        for e in entities {
            let spinner = world.get::<Spinner>(e).copied();
            let Some(s) = spinner else { continue };
            let axis = s.axis.normalize_or_zero();
            if axis.length_squared() < 1e-6 {
                continue;
            }
            if let Some(t) = world.get_mut::<Transform>(e) {
                let dq = Quat::from_axis_angle(axis, s.speed * dt);
                t.rotation = dq * t.rotation;
            }
        }
    }
}

struct MovementSystem;
impl System for MovementSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        world.for_each_pair::<Transform, Velocity, _>(|_e, t, v| {
            t.position += v.value * dt;
        });
    }
}

// ============================================================
// Campaign state
// ============================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
enum Stage {
    Act1,
    Act2,
    Act3,
    Act4,
    Act5,
    Victory,
}

impl Stage {
    fn title(self) -> &'static str {
        match self {
            Stage::Act1 => "ACT I — Awakening",
            Stage::Act2 => "ACT II — Courtyard",
            Stage::Act3 => "ACT III — Lobby",
            Stage::Act4 => "ACT IV — Dungeon",
            Stage::Act5 => "ACT V — Escape",
            Stage::Victory => "VICTORY",
        }
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct CampaignState {
    stage: Stage,
    kills_total: u32,
    kills_in_stage: u32,
    kills_required: u32,
    has_key_red: bool,
    gold: u32,
    objective: String,
    processed: HashSet<Entity>,
    dialog_shown: HashSet<String>,
    checkpoints: HashSet<String>,
    dialog: Option<(String, f32)>,
    victory_time: Option<f32>,
    playtime: f32,
}

impl CampaignState {
    fn new() -> Self {
        Self {
            stage: Stage::Act1,
            kills_total: 0,
            kills_in_stage: 0,
            kills_required: 2,
            has_key_red: false,
            gold: 0,
            objective: "Kill 2 guards".into(),
            processed: HashSet::new(),
            dialog_shown: HashSet::new(),
            checkpoints: HashSet::new(),
            dialog: Some(("Welcome to the Fallen Citadel.".into(), 4.0)),
            victory_time: None,
            playtime: 0.0,
        }
    }

    fn show_dialog(&mut self, text: impl Into<String>, ttl: f32) {
        self.dialog = Some((text.into(), ttl));
    }

    fn tick_dialog(&mut self, dt: f32) {
        if let Some((_, t)) = &mut self.dialog {
            *t -= dt;
            if *t <= 0.0 {
                self.dialog = None;
            }
        }
    }
}

// ============================================================
// Helper: спавн NPC с диалогом
// ============================================================

fn spawn_dialog_npc(
    world: &mut World,
    name: impl Into<String>,
    pos: Vec3,
    dialog_id: impl Into<String>,
) -> Entity {
    let e = world.spawn();
    world.insert(e, Name(name.into()));
    world.insert(e, Transform::at(pos).with_scale(1.0));
    world.insert(e, MeshHandle("capsule".into()));
    world.insert(e, MaterialHandle("gold".into()));
    world.insert(e, DialogNpc { dialog_id: dialog_id.into() });
    world.insert(e, Collider::capsule(0.4, 1.8));
    world.insert(e, RigidBody::static_body());
    e
}

// ============================================================
// FortressDemo
// ============================================================

pub struct FortressDemo {
    camera: Camera3D,
    systems: Vec<Box<dyn System>>,
    postfx: PostFx,
    built: bool,
    dragging: bool,
    show_grid: bool,
    show_culling: bool,

    skeletons: HashMap<String, Skeleton>,
    animations: HashMap<String, AnimationClip>,
    animation_runtime: AnimationRuntime,
    animation_events: AnimationEvents,

    bell_entity: Option<Entity>,
    ai_target_entity: Option<Entity>,
    boss_entity: Option<Entity>,
    gate_1: Option<Entity>,
    gate_2: Option<Entity>,
    gate_4: Option<Entity>,
    gate_boss: Option<Entity>,
    navmesh_bake_requested: bool,

    demo_health: f32,
    demo_health_max: f32,
    demo_paused: bool,
    demo_show_debug: bool,
    last_target_hp: f32,

    weapons: WeaponRegistry,
    current_weapon: usize,
    weapon_runtimes: Vec<WeaponRuntime>,
    weapon_raise_timer: f32,
    demo_reloading: bool,
    demo_reload_timer: f32,
    demo_fire_cooldown: f32,
    ads_active: bool,
    ads_blend: f32,
    base_fov: f32,

    player_stats: PlayerStats,
    show_character_sheet: bool,

    inventory: Inventory,
    equipment: Equipment,
    item_registry: ItemRegistry,
    loot_registry: LootRegistry,
    show_inventory: bool,
    inv_drag: InvDragState,

    // === Vendor ===
    vendor_registry: VendorRegistry,
    vendor_session: Option<VendorSession>,
    vendor_esc_pressed: bool,

    // === Crafting ===
    crafting_registry: CraftingRegistry,
    crafting_session: Option<CraftingSession>,
    crafting_esc_pressed: bool,
    rmb_pressed_this_frame: bool,

    last_play_mode: bool,
    hud: Hud,
    campaign: CampaignState,
    quest_manager: QuestManager,

    // === Dialog ===
    dialogs: HashMap<String, DialogGraph>,
    current_dialog_lang: String,
    dialog_session: Option<DialogSession>,

    // === Save / Load ===
    save_manager: crate::scene::SaveManager,
    pending_world_request: Option<WorldLoadRequest>,
    save_slots_cache: Vec<(u32, Option<crate::scene::SaveHeader>)>,
    save_slots_cache_dirty: bool,
    pending_autosave: bool,
    pending_save_slot: Option<u32>,
    pending_quit_to_menu: bool,

    prev_world_matrices: HashMap<Entity, glam::Mat4>,
    was_in_play_mode: bool,
    last_dt: f32,
    weapon_recoil: f32,
    pending_hit_stop: Option<(f32, f32)>,
    auto_fire_accumulator: f32,

    probe_baked_this_session: bool,
}

impl FortressDemo {
    pub fn new() -> Self {
        let mut quest_manager = QuestManager::new();
        if let Ok(qm) = QuestManager::load_from_dir("assets/quests") {
            quest_manager = qm;
        }

        let initial_lang = crate::game::locale::current_id().unwrap_or_else(|| "ru".to_string());
        let dialogs = DialogGraph::load_from_dir("assets/dialogs", &initial_lang).unwrap_or_default();
        log::info!("dialogs: {} graphs loaded [{}]", dialogs.len(), initial_lang);

        Self {
            camera: Camera3D::new(16.0 / 9.0),
            systems: vec![
                Box::new(TimerSystem),
                Box::new(RotationSystem),
                Box::new(MovementSystem),
            ],
            postfx: default_postfx(),
            built: false,
            dragging: false,
            show_grid: false,
            show_culling: true,

            skeletons: HashMap::new(),
            animations: HashMap::new(),
            animation_runtime: AnimationRuntime::new(),
            animation_events: AnimationEvents::new(),

            bell_entity: None,
            ai_target_entity: None,
            boss_entity: None,
            gate_1: None,
            gate_2: None,
            gate_4: None,
            gate_boss: None,
            navmesh_bake_requested: false,

            demo_health: 100.0,
            demo_health_max: 100.0,
            demo_paused: false,
            demo_show_debug: true,
            last_target_hp: 100.0,

            weapons: WeaponRegistry::empty(),
            current_weapon: 0,
            weapon_runtimes: Vec::new(),
            weapon_raise_timer: 0.0,
            demo_reloading: false,
            demo_reload_timer: 0.0,
            demo_fire_cooldown: 0.0,
            ads_active: false,
            ads_blend: 0.0,
            base_fov: 75.0_f32.to_radians(),

            player_stats: PlayerStats::default(),
            show_character_sheet: false,

            inventory: Inventory::new(20),
            equipment: Equipment::default(),
            item_registry: ItemRegistry::empty(),
            loot_registry: LootRegistry::empty(),
            show_inventory: false,
            inv_drag: InvDragState::default(),
            rmb_pressed_this_frame: false,

            vendor_registry: VendorRegistry::empty(),
            vendor_session: None,
            vendor_esc_pressed: false,

            crafting_registry: CraftingRegistry::empty(),
            crafting_session: None,
            crafting_esc_pressed: false,

            last_play_mode: false,
            hud: Hud::new(),
            campaign: CampaignState::new(),
            quest_manager,

            dialogs,
            current_dialog_lang: initial_lang,
            dialog_session: None,

            save_manager: crate::scene::SaveManager::new("saves"),
            pending_world_request: None,
            save_slots_cache: Vec::new(),
            save_slots_cache_dirty: true,
            pending_autosave: false,
            pending_save_slot: None,
            pending_quit_to_menu: false,

            prev_world_matrices: HashMap::new(),
            was_in_play_mode: false,
            last_dt: 1.0 / 60.0,
            weapon_recoil: 0.0,
            pending_hit_stop: None,
            auto_fire_accumulator: 0.0,
            probe_baked_this_session: false,
        }
    }

    // ============================================================
    // Save / Load
    // ============================================================

    fn save_slot(&mut self, slot: u32, world: &World, renderer: &Renderer) {
        match crate::scene::save_scene_with_assets_to_string(world, renderer, None) {
            Ok(scene_ron) => {
                let gs = self.save_game_state();
                let header = crate::scene::SaveHeader {
                    format_version: crate::scene::CURRENT_FORMAT,
                    game_version: env!("CARGO_PKG_VERSION").to_string(),
                    timestamp_unix: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .map(|d| d.as_secs())
                        .unwrap_or(0),
                    playtime_secs: self.campaign.playtime,
                    stage_title: self.campaign.stage.title().to_string(),
                    kills: self.campaign.kills_total,
                };
                match self.save_manager.save(slot, scene_ron, gs, header) {
                    Ok(()) => {
                        self.hud.push(format!("💾 Saved to slot {}", slot + 1));
                        self.save_slots_cache_dirty = true;
                    }
                    Err(e) => self.hud.push(format!("Save failed: {}", e)),
                }
            }
            Err(e) => self.hud.push(format!("Snapshot failed: {}", e)),
        }
    }

    fn load_slot(&mut self, slot: u32) {
        match self.save_manager.load(slot) {
            Ok(file) => {
                self.pending_world_request = Some(WorldLoadRequest {
                    scene_ron: file.scene_ron,
                    game_state_ron: file.game_state_ron,
                });
                self.hud.push(format!("📂 Loading slot {}…", slot + 1));
            }
            Err(e) => self.hud.push(format!("Load failed: {}", e)),
        }
    }

    fn delete_slot(&mut self, slot: u32) {
        if let Err(e) = self.save_manager.delete(slot) {
            self.hud.push(format!("Delete failed: {}", e));
        } else {
            self.hud.push(format!("🗑 Slot {} deleted", slot + 1));
            self.save_slots_cache_dirty = true;
        }
    }

    fn refresh_save_slots(&mut self) {
        self.save_slots_cache = self.save_manager.list();
        self.save_slots_cache_dirty = false;
    }

    // ============================================================
    // Gate helpers
    // ============================================================

    fn find_by_name(world: &World, name: &str) -> Option<Entity> {
        world.query::<Name>()
            .find_map(|(e, n)| if n.0 == name { Some(e) } else { None })
    }

    fn open_gate_by_name(
        &self,
        world: &mut World,
        entity: Option<Entity>,
        fallback_name: &str,
    ) {
        let g = match entity {
            Some(e) if world.entities().contains(&e) => e,
            _ => match Self::find_by_name(world, fallback_name) {
                Some(e) => {
                    log::warn!("Gate '{}': stale entity, resolved by name → #{}", fallback_name, e);
                    e
                }
                None => {
                    log::error!("Gate '{}' not found", fallback_name);
                    return;
                }
            },
        };
        self.open_gate(world, Some(g));
    }

    // ============================================================
    // Weapon helpers
    // ============================================================

    fn current_weapon(&self) -> Option<&Weapon> {
        self.weapons.get(self.current_weapon)
    }

    fn current_runtime(&self) -> Option<&WeaponRuntime> {
        self.weapon_runtimes.get(self.current_weapon)
    }

    fn current_runtime_mut(&mut self) -> Option<&mut WeaponRuntime> {
        self.weapon_runtimes.get_mut(self.current_weapon)
    }

    fn switch_weapon(&mut self, idx: usize) {
        if idx == self.current_weapon || idx >= self.weapons.len() {
            return;
        }
        if let Some(rt) = self.current_runtime_mut() {
            rt.reloading = false;
            rt.reload_timer = 0.0;
        }
        self.current_weapon = idx;
        self.weapon_raise_timer = 0.30;
        self.ads_active = false;
    }

    fn give_ammo_all(&mut self, mult: f32) -> u32 {
        let mut total = 0;
        for i in 0..self.weapons.len() {
            let Some(w) = self.weapons.get(i).cloned() else { continue };
            let Some(rt) = self.weapon_runtimes.get_mut(i) else { continue };
            let add = ((w.ammo_pickup as f32) * mult).round() as u32;
            rt.reserve = rt.reserve.saturating_add(add);
            total += add;
        }
        total
    }

    fn heal(&mut self, amount: f32) {
        self.demo_health = (self.demo_health + amount).min(self.demo_health_max);
    }

    // ============================================================
    // RPG helpers
    // ============================================================

    fn total_attributes(&self) -> crate::game::stats::Attributes {
        let mut a = self.player_stats.attributes;
        let bonus = self.equipment.bonus_attributes(&self.item_registry);
        a.strength = a.strength.saturating_add(bonus.strength);
        a.dexterity = a.dexterity.saturating_add(bonus.dexterity);
        a.intelligence = a.intelligence.saturating_add(bonus.intelligence);
        a.vitality = a.vitality.saturating_add(bonus.vitality);
        a.luck = a.luck.saturating_add(bonus.luck);
        a
    }

    fn try_pickup(&mut self, item_id: &str, count: u32) {
        if let Some(item) = self.item_registry.get_by_id(item_id).cloned()
            && item.kind == ItemKind::Misc
            && item.gold_value > 0
        {
            let total = item.gold_value.saturating_mul(count);
            self.campaign.gold = self.campaign.gold.saturating_add(total);
            self.hud.push(format!("+{} gold", total));
            return;
        }

        let stack = ItemStack::new(item_id, count);
        let leftover = self.inventory.add(&self.item_registry, stack);
        let picked = count.saturating_sub(leftover);

        if picked > 0 {
            let name = self
                .item_registry
                .get_by_id(item_id)
                .map(|i| i.name.clone())
                .unwrap_or_else(|| item_id.to_string());
            self.hud.push(format!("+{} × {}", picked, name));
        }
        if leftover > 0 {
            self.hud.push("Inventory full!");
        }
    }

    fn equip_from_slot(&mut self, idx: usize) {
        let Some(stack) = self.inventory.slots.get(idx).and_then(|s| s.clone()) else { return };
        let Some(item) = self.item_registry.get_by_id(&stack.item_id).cloned() else { return };
        if !item.equippable {
            return;
        }

        let current = match item.kind {
            ItemKind::Weapon => self.equipment.weapon_id.take(),
            ItemKind::Armor => self.equipment.armor_id.take(),
            ItemKind::Trinket => self.equipment.trinket_id.take(),
            _ => return,
        };

        let _ = self.inventory.take_one(idx);

        if let Some(old_id) = current {
            let _ = self.inventory.add(&self.item_registry, ItemStack::new(old_id, 1));
        }

        match item.kind {
            ItemKind::Weapon => self.equipment.weapon_id = Some(item.id.clone()),
            ItemKind::Armor => self.equipment.armor_id = Some(item.id.clone()),
            ItemKind::Trinket => self.equipment.trinket_id = Some(item.id.clone()),
            _ => {}
        }
        self.hud.push(format!("Equipped: {}", item.name));
    }

    fn use_from_slot(&mut self, idx: usize) {
        let Some(stack) = self.inventory.slots.get(idx).and_then(|s| s.clone()) else { return };
        let Some(item) = self.item_registry.get_by_id(&stack.item_id).cloned() else { return };
        if item.kind != ItemKind::Consumable {
            return;
        }
        if item.heal_amount > 0.0 {
            self.heal(item.heal_amount);
            self.hud.push(format!("Used {} · +{:.0} HP", item.name, item.heal_amount));
        }
        let _ = self.inventory.take_one(idx);
    }

    fn hit_test_inventory(&self, sw: f32, sh: f32, mx: f32, my: f32) -> Option<InvLocation> {
        let px = (sw - INV_PANEL_W) * 0.5;
        let py = (sh - INV_PANEL_H) * 0.5;
        let content_y = py + 100.0;

        let eq_x = px + INV_PAD;
        let eq_y = content_y + 8.0 + 18.0;
        let slots = [
            (crate::game::items::EquipSlot::Weapon, 0usize),
            (crate::game::items::EquipSlot::Armor, 1),
            (crate::game::items::EquipSlot::Trinket, 2),
        ];
        for (slot, idx) in slots {
            let y = eq_y + idx as f32 * (INV_EQ_SLOT_H + INV_GAP);
            if mx >= eq_x && mx < eq_x + INV_EQ_SLOT_W
                && my >= y && my < y + INV_EQ_SLOT_H {
                return Some(InvLocation::Equipment(slot));
            }
        }

        let grid_x = eq_x + INV_EQ_SLOT_W + INV_PAD;
        let grid_y = content_y + 8.0 + 18.0;
        for i in 0..self.inventory.capacity {
            let col = (i % INV_COLS) as f32;
            let row = (i / INV_COLS) as f32;
            let cx = grid_x + col * (INV_CELL + INV_GAP);
            let cy = grid_y + row * (INV_CELL + INV_GAP);
            if mx >= cx && mx < cx + INV_CELL
                && my >= cy && my < cy + INV_CELL {
                return Some(InvLocation::Inventory(i));
            }
        }
        None
    }

    fn handle_craft(&mut self, recipe: &RecipeDef) {
        if self.player_stats.experience.level < recipe.required_level {
            self.hud.push(format!("Requires level {}", recipe.required_level));
            return;
        }
        if self.campaign.gold < recipe.gold_cost {
            self.hud.push("Not enough gold");
            return;
        }
        for input in &recipe.inputs {
            if self.inventory.count_of(&input.item_id) < input.count {
                let name = self.item_registry
                    .get_by_id(&input.item_id)
                    .map(|i| i.name.clone())
                    .unwrap_or_else(|| input.item_id.clone());
                self.hud.push(format!("Need {} ×{}", name, input.count));
                return;
            }
        }

        let mut inv_check = self.inventory.clone();
        for input in &recipe.inputs {
            inv_check.remove_by_id(&input.item_id, input.count);
        }
        for output in &recipe.outputs {
            let leftover = inv_check.add(
                &self.item_registry,
                ItemStack::new(output.item_id.clone(), output.count),
            );
            if leftover > 0 {
                self.hud.push("Inventory full!");
                return;
            }
        }

        for input in &recipe.inputs {
            self.inventory.remove_by_id(&input.item_id, input.count);
        }
        for output in &recipe.outputs {
            self.inventory.add(
                &self.item_registry,
                ItemStack::new(output.item_id.clone(), output.count),
            );
        }
        self.campaign.gold -= recipe.gold_cost;
        self.hud.push(format!("🔨 Crafted: {}", recipe.title));
    }

    fn handle_vendor_buy(&mut self, def: &VendorDef, idx: usize) {
        let Some(session) = self.vendor_session.as_mut() else { return };
        let Some(vitem) = def.items.get(idx).cloned() else { return };
        if !session.has_stock(idx) {
            self.hud.push("Out of stock");
            return;
        }
        if self.campaign.gold < vitem.price {
            self.hud.push("Not enough gold");
            return;
        }
        let stack = ItemStack::new(vitem.item_id.clone(), 1);
        let leftover = self.inventory.add(&self.item_registry, stack);
        if leftover > 0 {
            self.hud.push("Inventory full!");
            return;
        }
        self.campaign.gold -= vitem.price;
        session.take_one(idx);
        session.spent += vitem.price;
        let name = self.item_registry
            .get_by_id(&vitem.item_id)
            .map(|i| i.name.clone())
            .unwrap_or(vitem.item_id.clone());
        self.hud.push(format!("Bought {} for {}g", name, vitem.price));
    }

    fn handle_vendor_sell(&mut self, idx: usize) {
        let Some(stack) = self.inventory.slots.get(idx).and_then(|s| s.clone()) else { return };
        let Some(item) = self.item_registry.get_by_id(&stack.item_id).cloned() else { return };
        if item.sell_price == 0 {
            self.hud.push("This item cannot be sold");
            return;
        }
        let _ = self.inventory.take_one(idx);
        self.campaign.gold += item.sell_price;
        if let Some(session) = self.vendor_session.as_mut() {
            session.earned += item.sell_price;
        }
        self.hud.push(format!("Sold {} for {}g", item.name, item.sell_price));
    }

    fn handle_inv_swap(&mut self, a: InvLocation, b: InvLocation) {
        match (a, b) {
            (InvLocation::Inventory(i), InvLocation::Inventory(j)) => {
                self.inventory.swap(i, j, &self.item_registry);
            }
            (InvLocation::Inventory(i), InvLocation::Equipment(slot)) => {
                let Some(stack) = self.inventory.slots.get(i).and_then(|s| s.clone()) else {
                    return;
                };
                let Some(item) = self.item_registry.get_by_id(&stack.item_id).cloned() else {
                    return;
                };
                if !item.equippable {
                    return;
                }
                let matches_slot = matches!(
                    (item.kind, slot),
                    (ItemKind::Weapon, crate::game::items::EquipSlot::Weapon)
                        | (ItemKind::Armor, crate::game::items::EquipSlot::Armor)
                        | (ItemKind::Trinket, crate::game::items::EquipSlot::Trinket)
                );
                if !matches_slot {
                    return;
                }

                let _ = self.inventory.take_slot(i);
                let old = self.equipment.put_slot(slot, item.id.clone());
                if let Some(old_id) = old {
                    let _ = self.inventory.add(
                        &self.item_registry,
                        ItemStack::new(old_id, 1),
                    );
                }
                self.hud.push(format!("Equipped: {}", item.name));
            }
            (InvLocation::Equipment(slot), InvLocation::Inventory(_)) => {
                let Some(old_id) = self.equipment.take_slot(slot) else {
                    return;
                };
                let leftover = self.inventory.add(
                    &self.item_registry,
                    ItemStack::new(old_id, 1),
                );
                if leftover > 0 {
                    self.hud.push("Inventory full!");
                } else {
                    self.hud.push("Unequipped");
                }
            }
            (InvLocation::Equipment(_), InvLocation::Equipment(_)) => {}
        }
    }

    // ============================================================
    // Building
    // ============================================================

    fn build(&mut self, world: &mut World) {
        let terrain_e = world.spawn();
        world.insert(terrain_e, Name("Terrain".into()));
        world.insert(terrain_e, Transform::at(Vec3::ZERO));
        world.insert(terrain_e, MeshHandle("terrain".into()));
        world.insert(terrain_e, MaterialHandle("terrain_mat".into()));

        self.build_act1(world);
        self.build_act2(world);
        self.build_act3(world);
        self.build_act4(world);
        self.build_act5(world);
        self.build_bridges(world);
        self.build_global_lighting(world);
        self.build_ai_target(world);
        self.build_decorations(world);
        self.bell_entity = Some(self.build_bell(world));

        log::info!("Campaign built: 5 acts, {} entities", world.len());
        self.navmesh_bake_requested = true;
    }

    fn build_bridges(&mut self, world: &mut World) {
        static_box(world, "Bridge_Act1_Act2", "arena_floor",
            Vec3::new(-40.0, 0.0, 0.0), Vec3::new(20.0, 0.5, 8.0),
            Vec3::splat(0.5), 2.0);
        wall(world, "Bridge_A1A2_Rail_N",
            Vec3::new(-50.0, 0.0, -4.0), Vec3::new(-30.0, 0.0, -4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        wall(world, "Bridge_A1A2_Rail_S",
            Vec3::new(-50.0, 0.0, 4.0), Vec3::new(-30.0, 0.0, 4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        torch(world, "Bridge_A1A2_Torch1", Vec3::new(-45.0, 3.0, 0.0));
        torch(world, "Bridge_A1A2_Torch2", Vec3::new(-35.0, 3.0, 0.0));

        static_box(world, "Bridge_Act2_Act3", "arena_floor",
            Vec3::new(35.0, 0.0, 0.0), Vec3::new(10.0, 0.5, 8.0),
            Vec3::splat(0.5), 2.0);
        wall(world, "Bridge_A2A3_Rail_N",
            Vec3::new(30.0, 0.0, -4.0), Vec3::new(40.0, 0.0, -4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        wall(world, "Bridge_A2A3_Rail_S",
            Vec3::new(30.0, 0.0, 4.0), Vec3::new(40.0, 0.0, 4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        torch(world, "Bridge_A2A3_Torch", Vec3::new(35.0, 3.0, 0.0));
    }

    fn build_act1(&mut self, world: &mut World) {
        let c = ACT1_CENTER;
        let h = ACT1_HALF;

        static_box(world, "Act1_Floor", "arena_floor",
            c, Vec3::new(h * 2.0, 0.5, h * 2.0), Vec3::splat(0.5), 2.5);

        wall(world, "Act1_Wall_W", c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_N", c + Vec3::new(-h, 0.0, -h), c + Vec3::new(h, 0.0, -h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_S", c + Vec3::new(-h, 0.0, h), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_E1", c + Vec3::new(h, 0.0, -h), c + Vec3::new(h, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_E2", c + Vec3::new(h, 0.0, 3.0), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        let gate_pos = c + Vec3::new(h, 2.0, 0.0);
        let gate = locked_gate(world, "gate_east_1", gate_pos,
            Vec3::new(0.5, 4.0, 6.0), "arena_door");
        self.gate_1 = Some(gate);

        torch(world, "Act1_Torch_1", c + Vec3::new(-h + 1.0, 3.0, -h + 1.0));
        torch(world, "Act1_Torch_2", c + Vec3::new(-h + 1.0, 3.0,  h - 1.0));

        let patrol = vec![
            c + Vec3::new(-6.0, 0.0, -6.0),
            c + Vec3::new( 6.0, 0.0, -6.0),
            c + Vec3::new( 6.0, 0.0,  6.0),
            c + Vec3::new(-6.0, 0.0,  6.0),
        ];
        enemy(world, "Act1_Enemy_1", c + Vec3::new(-5.0, 0.0, 0.0), EnemyKind::Patrol, Some(patrol.clone()));
        enemy(world, "Act1_Enemy_2", c + Vec3::new( 5.0, 0.0, 0.0), EnemyKind::Patrol, Some(patrol));

        checkpoint(world, "checkpoint_1", c + Vec3::new(-h + 3.0, 0.02, 0.0));
        health_pickup(world, "health_a1", c + Vec3::new(-7.0, 0.5, 7.0), 30.0);
        ammo_pickup(world, "ammo_a1", c + Vec3::new(-7.0, 0.5, -7.0), 20);
        objective_marker(world, "objective_gate1", gate_pos + Vec3::new(0.0, 3.0, 0.0));
        dialog_zone(world, "dialog_intro", c + Vec3::new(-h + 4.0, 1.0, 0.0), 3.0);

        blood_decal(world, c + Vec3::new(-3.0, 0.02, 2.0), 1.5, 0.7);
        blood_decal(world, c + Vec3::new( 4.0, 0.02, -4.0), 1.8, 0.8);

        spawn_dialog_npc(world, "npc_elder", c + Vec3::new(-7.0, 0.9, -7.0), "elder");
        let smith_e = spawn_dialog_npc(world, "npc_blacksmith", c + Vec3::new(7.0, 0.9, -7.0), "blacksmith");
        world.insert(smith_e, VendorNpc { vendor_id: "blacksmith".into() });

        enemy(world, "Act1_Archer", c + Vec3::new(-9.0, 0.0, 8.0), EnemyKind::Archer, None);
    }

    fn build_act2(&mut self, world: &mut World) {
        let c = ACT2_CENTER;
        let h = ACT2_HALF;

        static_box(world, "Act2_Floor", "arena_floor",
            c, Vec3::new(h * 2.0, 0.5, h * 2.0), Vec3::splat(0.5), 2.0);

        wall(world, "Act2_Wall_W1", c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-h, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_W2", c + Vec3::new(-h, 0.0, 3.0), c + Vec3::new(-h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_E1", c + Vec3::new(h, 0.0, -h), c + Vec3::new(h, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_E2", c + Vec3::new(h, 0.0, 3.0), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_N", c + Vec3::new(-h, 0.0, -h), c + Vec3::new(h, 0.0, -h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_S", c + Vec3::new(-h, 0.0, h), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        let tower_off = h - 3.0;
        let towers = [
            ("NW", c + Vec3::new(-tower_off, 0.0, -tower_off)),
            ("NE", c + Vec3::new( tower_off, 0.0, -tower_off)),
            ("SW", c + Vec3::new(-tower_off, 0.0,  tower_off)),
            ("SE", c + Vec3::new( tower_off, 0.0,  tower_off)),
        ];
        for (side, center) in &towers {
            static_box(world, format!("Act2_Tower_{}", side), "arena_column",
                *center + Vec3::Y * 3.0, Vec3::new(6.0, 6.0, 6.0),
                Vec3::splat(0.5), 2.0);
            static_box(world, format!("Act2_Tower_{}_top", side), "arena_wall",
                *center + Vec3::Y * 6.4, Vec3::new(6.6, 0.4, 6.6),
                Vec3::splat(0.5), 1.0);
            torch(world, format!("Act2_Torch_{}", side), *center + Vec3::Y * 7.2);
        }

        let col_r = 12.0;
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let (sn, cs) = a.sin_cos();
            static_mesh(world, format!("Act2_Col_{}", i), "column_fluted", "arena_column",
                c + Vec3::new(cs * col_r, 2.5, sn * col_r), Quat::IDENTITY,
                Vec3::new(1.0, 5.0, 1.0),
                Some(Collider::aabb(Vec3::splat(0.4))));
        }

        for i in 0..3u32 {
            let r = 5.0 - i as f32 * 1.3;
            let y = i as f32 * 0.5 + 0.25;
            static_mesh(world, format!("Act2_Altar_Step_{}", i), "cylinder", "arena_platform",
                c + Vec3::Y * y, Quat::IDENTITY, Vec3::new(r * 2.0, 0.5, r * 2.0),
                Some(Collider::aabb(Vec3::splat(0.5))));
        }

        self.build_treasure_room(world, c + Vec3::new(tower_off, 0.0, tower_off), 6.0);

        let gate2_pos = c + Vec3::new(h, 2.0, 0.0);
        let gate2 = locked_gate(world, "gate_east_2", gate2_pos,
            Vec3::new(0.5, 4.0, 6.0), "arena_door");
        self.gate_2 = Some(gate2);

        torch(world, "Act2_Torch_C1", c + Vec3::new(-14.0, 3.0, -14.0));
        torch(world, "Act2_Torch_C2", c + Vec3::new( 14.0, 3.0, -14.0));
        torch(world, "Act2_Torch_C3", c + Vec3::new(-14.0, 3.0,  14.0));

        let perimeter = vec![
            c + Vec3::new(-24.0, 0.0, -24.0),
            c + Vec3::new( 24.0, 0.0, -24.0),
            c + Vec3::new( 24.0, 0.0,  24.0),
            c + Vec3::new(-24.0, 0.0,  24.0),
        ];
        enemy(world, "Act2_Patrol_1", perimeter[0], EnemyKind::Patrol, Some(perimeter.clone()));
        enemy(world, "Act2_Patrol_2", perimeter[2], EnemyKind::Patrol, Some(perimeter.clone()));

        let inner = vec![
            c + Vec3::new(-9.0, 0.0, 0.0),
            c + Vec3::new( 0.0, 0.0, -9.0),
            c + Vec3::new( 9.0, 0.0, 0.0),
            c + Vec3::new( 0.0, 0.0,  9.0),
        ];
        enemy(world, "Act2_Patrol_3", inner[0], EnemyKind::Patrol, Some(inner.clone()));
        enemy(world, "Act2_Patrol_4", inner[2], EnemyKind::Patrol, Some(inner));

        enemy(world, "Act2_Sniper_NW",
            c + Vec3::new(-tower_off, 6.6, -tower_off), EnemyKind::Sniper, None);
        enemy(world, "Act2_Sniper_NE",
            c + Vec3::new( tower_off, 6.6, -tower_off), EnemyKind::Sniper, None);

        health_pickup(world, "health_a2", c + Vec3::new(-20.0, 0.5, 20.0), 40.0);
        ammo_pickup(world, "ammo_a2_1", c + Vec3::new( 20.0, 0.5, -20.0), 30);
        ammo_pickup(world, "ammo_a2_2", c + Vec3::new(-24.0, 0.5, 0.0), 30);

        dialog_zone(world, "dialog_act2", c + Vec3::new(-h + 4.0, 1.0, 0.0), 3.0);

        for (x, z, s, a) in [
            (-12.0, -10.0, 1.8, 0.85),
            ( 10.0, -13.0, 2.0, 0.80),
            ( 15.0,  10.0, 1.6, 0.75),
            ( -8.0,  15.0, 1.9, 0.85),
            (  5.0,   5.0, 1.3, 0.70),
        ] {
            blood_decal(world, c + Vec3::new(x, 0.02, z), s, a);
        }

        enemy(world, "Act2_Mage_1", c + Vec3::new(-20.0, 0.0, 0.0), EnemyKind::Mage, None);
        enemy(world, "Act2_Mage_2", c + Vec3::new(20.0, 0.0, 0.0), EnemyKind::Mage, None);
        enemy(world, "Act2_Assassin_1",
            c + Vec3::new(0.0, 0.0, 20.0), EnemyKind::Assassin, None);
        enemy(world, "Act2_Tank_1",
            c + Vec3::new(0.0, 0.0, -20.0), EnemyKind::Tank, None);
    }

    fn build_treasure_room(&mut self, world: &mut World, center: Vec3, half: f32) {
        wall(world, "Act2_Treasure_W1",
            center + Vec3::new(-half, 0.0, -half),
            center + Vec3::new(-half, 0.0, -1.5),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Treasure_W2",
            center + Vec3::new(-half, 0.0, 1.5),
            center + Vec3::new(-half, 0.0, half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Treasure_N",
            center + Vec3::new(-half, 0.0, -half),
            center + Vec3::new( half, 0.0, -half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Treasure_S",
            center + Vec3::new(-half, 0.0, half),
            center + Vec3::new( half, 0.0, half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Treasure_E",
            center + Vec3::new(half, 0.0, -half),
            center + Vec3::new(half, 0.0, half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        enemy(world, "Act2_Treasure_Guard1",
            center + Vec3::new(2.0, 0.0, -2.0), EnemyKind::Elite, None);
        enemy(world, "Act2_Treasure_Guard2",
            center + Vec3::new(2.0, 0.0,  2.0), EnemyKind::Elite, None);

        chest(world, "Act2_Chest_A", center + Vec3::new(4.0, 0.25, -3.0), 200);
        chest(world, "Act2_Chest_B", center + Vec3::new(4.0, 0.25,  0.0), 350);

        let key_pos = center + Vec3::new(0.0, 0.6, 0.0);
        key_pickup(world, "key_red", key_pos, [1.0, 0.2, 0.2]);
        objective_marker(world, "objective_key", key_pos + Vec3::Y * 2.0);
        torch(world, "Act2_Treasure_Torch", center + Vec3::new(0.0, 3.5, 0.0));
    }

    fn build_act3(&mut self, world: &mut World) {
        let c = ACT3_CENTER;
        let hx = ACT3_HALF_X;
        let hz = ACT3_HALF_Z;

        static_box(world, "Act3_Floor", "arena_platform",
            c, Vec3::new(hx * 2.0, 0.5, hz * 2.0), Vec3::splat(0.5), 1.5);

        wall(world, "Act3_Wall_W1", c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(-hx, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_W2", c + Vec3::new(-hx, 0.0, 3.0), c + Vec3::new(-hx, 0.0, hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_E", c + Vec3::new(hx, 0.0, -hz), c + Vec3::new(hx, 0.0, hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_N", c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(hx, 0.0, -hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_S", c + Vec3::new(-hx, 0.0, hz), c + Vec3::new(hx, 0.0, hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        let portal_pos = c + Vec3::new(hx - 5.0, 1.5, 0.0);
        let act4_dest = ACT4_CENTER + Vec3::new(0.0, 1.0, 10.0);
        portal(world, "portal_to_dungeon", portal_pos, act4_dest, [0.55, 0.25, 0.9, 0.75]);
        objective_marker(world, "objective_portal", portal_pos + Vec3::Y * 2.0);

        enemy(world, "Act3_Guard", c + Vec3::new(-3.0, 0.0, 0.0), EnemyKind::Elite, None);

        health_pickup(world, "health_a3", c + Vec3::new(-8.0, 0.5, 5.0), 50.0);
        ammo_pickup(world, "ammo_a3_1", c + Vec3::new(-8.0, 0.5, -5.0), 40);
        ammo_pickup(world, "ammo_a3_2", c + Vec3::new( 0.0, 0.5,  6.0), 40);

        checkpoint(world, "checkpoint_3", c + Vec3::new(-hx + 3.0, 0.02, 0.0));
        dialog_zone(world, "dialog_act3", c + Vec3::new(-hx + 4.0, 1.0, 0.0), 3.0);

        torch(world, "Act3_Torch_1", c + Vec3::new(-hx + 2.0, 3.5, -hz + 2.0));
        torch(world, "Act3_Torch_2", c + Vec3::new(-hx + 2.0, 3.5,  hz - 2.0));
    }

    fn build_act4(&mut self, world: &mut World) {
        let c = ACT4_CENTER;
        let h = ACT4_HALF;

        static_box(world, "Act4_Floor", "rpg_dungeon",
            c, Vec3::new(h * 2.0, 0.5, h * 2.0), Vec3::splat(0.5), 2.0);
        static_box(world, "Act4_Ceiling", "arena_wall",
            c + Vec3::Y * 6.0, Vec3::new(h * 2.0, 0.5, h * 2.0),
            Vec3::splat(0.5), 2.0);

        wall(world, "Act4_Wall_W", c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-h, 0.0, h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_E", c + Vec3::new(h, 0.0, -h), c + Vec3::new(h, 0.0, h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_S", c + Vec3::new(-h, 0.0, h), c + Vec3::new(h, 0.0, h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_N1", c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-3.0, 0.0, -h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_N2", c + Vec3::new(3.0, 0.0, -h), c + Vec3::new(h, 0.0, -h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);

        let gate_boss_pos = c + Vec3::new(0.0, 2.5, -h);
        let gate_boss = locked_gate(world, "gate_boss", gate_boss_pos,
            Vec3::new(6.0, 5.0, 0.5), "rpg_dungeon");
        self.gate_boss = Some(gate_boss);

        crystal(world, "Act4_Crystal_A", c + Vec3::new(-10.0, 1.5, -5.0), [0.35, 0.6, 1.0]);
        crystal(world, "Act4_Crystal_B", c + Vec3::new( 10.0, 1.5, -5.0), [0.6, 0.35, 1.0]);
        crystal(world, "Act4_Crystal_C", c + Vec3::new(  0.0, 1.5, 10.0), [0.35, 1.0, 0.85]);

        enemy(world, "Act4_Wolf_1", c + Vec3::new(-10.0, 0.0, 5.0), EnemyKind::Patrol, None);
        enemy(world, "Act4_Wolf_2", c + Vec3::new( 10.0, 0.0, 5.0), EnemyKind::Patrol, None);
        enemy(world, "Act4_Wolf_3", c + Vec3::new(  0.0, 0.0, -10.0), EnemyKind::Patrol, None);
        enemy(world, "Act4_Mage",
            c + Vec3::new(-15.0, 0.0, -15.0), EnemyKind::Mage, None);
        enemy(world, "Act4_Archer",
            c + Vec3::new(15.0, 0.0, -15.0), EnemyKind::Archer, None);
        enemy(world, "Act4_Assassin",
            c + Vec3::new(0.0, 0.0, 15.0), EnemyKind::Assassin, None);
        enemy(world, "Act4_Tank",
            c + Vec3::new(-15.0, 0.0, 15.0), EnemyKind::Tank, None);

        chest(world, "Act4_Chest_A", c + Vec3::new(-14.0, 0.25, 14.0), 750);
        chest(world, "Act4_Chest_B", c + Vec3::new( 14.0, 0.25, 14.0), 1000);

        let back_dest = ACT3_CENTER + Vec3::new(0.0, 1.0, 0.0);
        portal(world, "portal_back_3",
            c + Vec3::new(-15.0, 1.5, 15.0), back_dest, [0.9, 0.5, 0.2, 0.75]);

        checkpoint(world, "checkpoint_4", c + Vec3::new(0.0, 0.02, 10.0));
        dialog_zone(world, "dialog_act4", c + Vec3::new(0.0, 1.0, 8.0), 4.0);

        for i in 0..6 {
            let a = i as f32 / 6.0 * std::f32::consts::TAU;
            rune_decal(world,
                c + Vec3::new(a.cos() * 8.0, 0.02, a.sin() * 8.0),
                1.2, [0.4, 0.3, 0.9, 0.6]);
        }

        torch(world, "Act4_Torch_1", c + Vec3::new(-15.0, 4.0, -15.0));
        torch(world, "Act4_Torch_2", c + Vec3::new( 15.0, 4.0, -15.0));

        self.build_boss_room(world);

        let gate4_pos = c + Vec3::new(0.0, 2.5, -h * 2.0);
        let gate4 = locked_gate(world, "gate_north_4", gate4_pos,
            Vec3::new(6.0, 5.0, 0.5), "rpg_dungeon");
        self.gate_4 = Some(gate4);
    }

    fn build_boss_room(&mut self, world: &mut World) {
        let c = ACT4_CENTER;
        let h = ACT4_HALF;
        let room_cx = c.x;
        let room_cz = c.z - h - 7.0;
        let half_x = 8.0;
        let half_z = 7.0;

        static_box(world, "Boss_Floor", "rpg_dungeon",
            Vec3::new(room_cx, c.y, room_cz),
            Vec3::new(half_x * 2.0, 0.5, half_z * 2.0),
            Vec3::splat(0.5), 2.0);
        static_box(world, "Boss_Ceiling", "arena_wall",
            Vec3::new(room_cx, c.y + 8.0, room_cz),
            Vec3::new(half_x * 2.0, 0.5, half_z * 2.0),
            Vec3::splat(0.5), 2.0);

        wall(world, "Boss_Wall_W",
            Vec3::new(room_cx - half_x, c.y, room_cz - half_z),
            Vec3::new(room_cx - half_x, c.y, room_cz + half_z),
            c.y, 8.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Boss_Wall_E",
            Vec3::new(room_cx + half_x, c.y, room_cz - half_z),
            Vec3::new(room_cx + half_x, c.y, room_cz + half_z),
            c.y, 8.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Boss_Wall_N1",
            Vec3::new(room_cx - half_x, c.y, room_cz - half_z),
            Vec3::new(-3.0, c.y, room_cz - half_z),
            c.y, 8.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Boss_Wall_N2",
            Vec3::new(3.0, c.y, room_cz - half_z),
            Vec3::new(room_cx + half_x, c.y, room_cz - half_z),
            c.y, 8.0, WALL_T, "rpg_dungeon", 2.0);

        static_mesh(world, "Boss_Pedestal", "cylinder", "arena_platform",
            Vec3::new(room_cx, c.y + 0.4, room_cz), Quat::IDENTITY,
            Vec3::new(4.0, 0.8, 4.0),
            Some(Collider::aabb(Vec3::splat(0.5))));

        let boss = enemy(world, "Boss_Dungeon",
            Vec3::new(room_cx, c.y, room_cz), EnemyKind::Boss, None);
        self.boss_entity = Some(boss);

        crystal(world, "Boss_Crystal_1",
            Vec3::new(room_cx - 5.0, c.y + 2.0, room_cz - 4.0), [1.0, 0.3, 0.3]);
        crystal(world, "Boss_Crystal_2",
            Vec3::new(room_cx + 5.0, c.y + 2.0, room_cz - 4.0), [1.0, 0.3, 0.3]);

        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            rune_decal(world,
                Vec3::new(room_cx + a.cos() * 6.0, c.y + 0.02, room_cz + a.sin() * 6.0),
                1.5, [1.0, 0.2, 0.2, 0.7]);
        }
    }

    fn build_act5(&mut self, world: &mut World) {
        let c = ACT5_CENTER;
        let hx = ACT5_HALF_X;
        let hz = ACT5_HALF_Z;

        static_box(world, "Act5_Floor", "arena_platform",
            c, Vec3::new(hx * 2.0, 0.5, hz * 2.0), Vec3::splat(0.5), 1.0);
        static_box(world, "Act5_Ceiling", "arena_wall",
            c + Vec3::Y * 6.0, Vec3::new(hx * 2.0, 0.5, hz * 2.0),
            Vec3::splat(0.5), 1.5);

        wall(world, "Act5_Wall_W", c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(-hx, 0.0, hz),
            0.0, 6.0, WALL_T, "arena_column", 2.0);
        wall(world, "Act5_Wall_E", c + Vec3::new(hx, 0.0, -hz), c + Vec3::new(hx, 0.0, hz),
            0.0, 6.0, WALL_T, "arena_column", 2.0);
        wall(world, "Act5_Wall_N", c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(hx, 0.0, -hz),
            0.0, 6.0, WALL_T, "arena_column", 2.0);

        let exit_pos = c + Vec3::new(0.0, 1.5, -hz + 3.0);
        exit_zone(world, "exit_zone", exit_pos, 2.5);

        for i in 0..4 {
            let x = if i % 2 == 0 { -hx + 2.0 } else { hx - 2.0 };
            let z = if i < 2 { -hz + 2.0 } else { hz - 2.0 };
            crystal(world, format!("Act5_Crystal_{}", i),
                c + Vec3::new(x, 2.0, z), [1.0, 0.8, 0.4]);
        }

        checkpoint(world, "checkpoint_5", c + Vec3::new(0.0, 0.02, hz - 3.0));
    }

    fn build_global_lighting(&mut self, world: &mut World) {
        let sun = world.spawn();
        world.insert(sun, Name("Sun".into()));
        world.insert(sun, Transform::at(Vec3::new(0.0, 40.0, 0.0)));
        let mut l = DirectionalLight::sun();
        l.direction = Vec3::new(0.45, 1.0, 0.35);
        l.intensity = 1.1;
        l.color = [1.0, 0.95, 0.85];
        world.insert(sun, l);

        let fill = world.spawn();
        world.insert(fill, Name("Fill".into()));
        world.insert(fill, Transform::at(Vec3::new(0.0, 10.0, 0.0)));
        let mut f = DirectionalLight::fill();
        f.intensity = 0.15;
        world.insert(fill, f);
    }

    fn build_ai_target(&mut self, world: &mut World) {
        let e = world.spawn();
        world.insert(e, Name("Player".into()));
        world.insert(e, Transform::at(ACT1_CENTER + Vec3::Y * 0.9).with_scale(0.5));
        world.insert(e, AiTarget);
        world.insert(e, Health::new(self.demo_health_max));
        world.insert(e, MeshHandle("sphere".into()));
        world.insert(e, MaterialHandle("arena_marker".into()));
        self.ai_target_entity = Some(e);
        self.last_target_hp = self.demo_health_max;
    }

    fn build_bell(&mut self, world: &mut World) -> Entity {
        let bell = world.spawn();
        world.insert(bell, Name("Bell".into()));
        world.insert(bell, Transform::at(ACT2_CENTER + Vec3::Y * 6.0).with_scale(0.4));
        world.insert(bell, MeshHandle("sphere".into()));
        world.insert(bell, MaterialHandle("arena_marker".into()));
        world.insert(bell, Spinner::new(Vec3::Y, 0.5));
        world.insert(bell, Timer::new(8.0));
        world.insert(bell, AudioSource::new("ding")
            .with_bus(AudioBus::Music)
            .with_range(2.0, 40.0)
            .with_volume(0.5));
        bell
    }

    fn build_decorations(&mut self, world: &mut World) {
        for i in 0..3 {
            crate_box(world, format!("Act1_Crate_{}", i),
                Vec3::new(-66.0 + i as f32 * 0.8, 0.5, -6.0), 1.0);
        }
        for i in 0..3 {
            barrel(world, format!("Act1_Barrel_{}", i),
                Vec3::new(-65.0 + i as f32 * 0.5, 0.6, 6.0));
        }
        static_mesh(world, "Act1_Rock_1", "rock_cluster_a", "arena_column",
            Vec3::new(-68.0, 0.0, 8.0), Quat::IDENTITY, Vec3::ONE, None);
        for i in 0..4 {
            static_mesh(world, format!("Act1_Grave_{}", i), "gravestone", "arena_column",
                Vec3::new(-68.0 + i as f32 * 1.2, 0.0, 8.5),
                Quat::IDENTITY, Vec3::ONE, None);
        }

        let trees = [
            (Vec3::new(-40.0, 0.0, -40.0), "tree_pine_a"),
            (Vec3::new( 40.0, 0.0, -40.0), "tree_pine_b"),
            (Vec3::new(-40.0, 0.0,  40.0), "tree_oak_a"),
            (Vec3::new( 40.0, 0.0,  40.0), "tree_oak_b"),
        ];
        for (i, (pos, mesh)) in trees.iter().enumerate() {
            static_mesh(world, format!("Act2_Tree_{}", i), mesh, "foliage",
                *pos, Quat::from_axis_angle(Vec3::Y, i as f32 * 0.9), Vec3::ONE, None);
        }
        for i in 0..6 {
            crate_box(world, format!("Act2_Crate_{}", i),
                Vec3::new(-26.0 + i as f32 * 1.2, 0.5, -26.0), 1.0);
        }
        for i in 0..5 {
            barrel(world, format!("Act2_Barrel_{}", i),
                Vec3::new(24.0, 0.6 + (i / 2) as f32 * 0.9, 26.0));
        }

        for i in 0..5 {
            static_mesh(world, format!("Act3_Col_{}", i), "column_fluted", "arena_column",
                Vec3::new(-12.0 + i as f32 * 6.0, 2.5, -9.0),
                Quat::IDENTITY, Vec3::new(0.6, 5.0, 0.6), None);
        }

        let pillars = [
            (Vec3::new(-8.0, 0.0, 0.0), "pillar_ruined_a"),
            (Vec3::new( 8.0, 0.0, 0.0), "pillar_ruined_b"),
            (Vec3::new( 0.0, 0.0, -8.0), "pillar_ruined_c"),
        ];
        for (i, (pos, mesh)) in pillars.iter().enumerate() {
            static_mesh(world, format!("Act4_Pillar_{}", i), mesh, "rpg_dungeon",
                *pos + Vec3::new(ACT4_CENTER.x, ACT4_CENTER.y, ACT4_CENTER.z),
                Quat::from_axis_angle(Vec3::Y, i as f32 * 0.7),
                Vec3::new(1.0, 6.0, 1.0), None);
        }
    }

    fn open_gate(&self, world: &mut World, gate: Option<Entity>) {
        let Some(g) = gate else { return };
        let speed = world.get::<GateMotion>(g).map(|m| m.speed).unwrap_or(4.0);
        if let Some(v) = world.get_mut::<Velocity>(g) {
            v.value = Vec3::new(0.0, speed, 0.0);
        }
        if let Some(src) = world.get_mut::<AudioSource>(g) {
            src.playing = true;
        }
    }

    fn tick_gates(&mut self, world: &mut World) {
        for gate in [self.gate_1, self.gate_2, self.gate_4, self.gate_boss] {
            if let Some(g) = gate
                && world.entities().contains(&g) && stop_gate_if_reached(world, g) {
                    log::info!("Gate #{} stopped at open_y", g);
                }
        }
    }

    // ============================================================
    // FIX #4: process_triggers — once через processed, repeatable без
    // ============================================================

    fn process_triggers(&mut self, world: &mut World) {
        // FIX #4: once — через `processed`, repeatable — без `processed`.
        let fired: Vec<(Entity, String, bool)> = world
            .query::<Trigger>()
            .filter(|(e, t)| {
                if !t.fired {
                    return false;
                }
                if t.once && self.campaign.processed.contains(e) {
                    return false;
                }
                true
            })
            .filter_map(|(e, t)| {
                let name = world.get::<Name>(e)?.0.clone();
                Some((e, name, t.once))
            })
            .collect();

        for (e, name, once) in fired {
            if once {
                self.campaign.processed.insert(e);
            }
            self.on_trigger(world, e, &name);
        }
    }

    fn on_trigger(&mut self, world: &mut World, e: Entity, name: &str) {
        if let Some(rest) = name.strip_prefix("Pickup_")
            && let Some((item_id, _idx)) = rest.rsplit_once('_') {
                let count = world.get::<GoldValue>(e).map(|g| g.0).unwrap_or(1);
                self.try_pickup(item_id, count);
                world.despawn(e);
                return;
            }

        if name == "key_red" {
            self.campaign.has_key_red = true;
            self.hud.push("🔑 Red key acquired");
            self.campaign.show_dialog("The dungeon key is in your hands!", 3.5);
            if let Some(t) = world.get_mut::<Transform>(e) {
                t.position.y = -500.0;
            }
            self.hide_objective(world, "objective_key");
            return;
        }

        if name.starts_with("health_") {
            let amount = world
                .get::<Tint>(e)
                .map(|t| (t.0[0] * 100.0).max(10.0))
                .unwrap_or(30.0);
            self.heal(amount);
            self.hud.push(format!("+{:.0} HP", amount));
            if let Some(t) = world.get_mut::<Transform>(e) {
                t.position.y = -500.0;
            }
            return;
        }

        if name.starts_with("ammo_") {
            let mult = world
                .get::<Tint>(e)
                .map(|t| (t.0[0] * 4.0).max(1.0))
                .unwrap_or(2.0);
            let added = self.give_ammo_all(mult);
            self.hud.push(format!("+{} ammo (all weapons)", added));
            if let Some(t) = world.get_mut::<Transform>(e) {
                t.position.y = -500.0;
            }
            return;
        }

        if name.starts_with("checkpoint_") {
            if self.campaign.checkpoints.insert(name.to_string()) {
                self.hud.push("💾 Checkpoint (autosave)");
                self.pending_autosave = true;
            }
            return;
        }

        if name.starts_with("dialog_") {
            if self.campaign.dialog_shown.insert(name.to_string()) {
                let line = match name {
                    "dialog_intro" => "You awaken in ruins. Fight through the guards!",
                    "dialog_act2"  => "The courtyard swarms with enemies. Find the key in the treasure room.",
                    "dialog_act3"  => "A portal lies ahead. Grab health and ammo.",
                    "dialog_act4"  => "This is the Lord's dungeon. Prepare for battle.",
                    _ => "…",
                };
                self.campaign.show_dialog(line, 4.0);
            }
            return;
        }

        if name == "exit_zone" && self.campaign.stage != Stage::Victory {
            self.trigger_victory();
        }
    }

    fn hide_objective(&self, world: &mut World, name: &str) {
        let targets: Vec<Entity> = world
            .query::<Name>()
            .filter_map(|(e, n)| if n.0 == name { Some(e) } else { None })
            .collect();
        for e in targets {
            world.insert(e, Visible(false));
        }
    }

    fn trigger_victory(&mut self) {
        self.campaign.stage = Stage::Victory;
        self.campaign.victory_time = Some(self.campaign.playtime);
        self.campaign.objective = "Victory!".into();
        self.hud.push("🏆 VICTORY!");
    }

    fn check_stage_transitions(&mut self, world: &mut World) {
        match self.campaign.stage {
            Stage::Act1 => {
                if self.campaign.kills_required > 0
                    && self.campaign.kills_in_stage >= self.campaign.kills_required
                {
                    self.open_gate_by_name(world, self.gate_1, "gate_east_1");
                    self.hide_objective(world, "objective_gate1");
                    self.campaign.stage = Stage::Act2;
                    self.campaign.kills_in_stage = 0;
                    self.campaign.kills_required = 0;
                    self.campaign.objective = "Find the red key in the treasure room".into();
                    self.campaign.show_dialog("The gates are open. The courtyard lies ahead.", 4.0);
                }
            }
            Stage::Act2 => {
                if self.campaign.has_key_red {
                    self.open_gate_by_name(world, self.gate_2, "gate_east_2");
                    self.campaign.stage = Stage::Act3;
                    self.campaign.kills_in_stage = 0;
                    self.campaign.kills_required = 0;
                    self.campaign.objective = "Enter the lobby portal".into();
                    self.campaign.show_dialog("The key opened the path to the lobby.", 4.0);
                }
            }
            Stage::Act3 => {
                if self.camera.position().y < -5.0 {
                    self.campaign.stage = Stage::Act4;
                    self.campaign.objective = "Kill the Lord of the Citadel".into();
                    self.hide_objective(world, "objective_portal");
                }
            }
            Stage::Act4 => {
                let stored = self.boss_entity
                    .map(|b| world.entities().contains(&b))
                    .unwrap_or(false);
                let name_alive = Self::find_by_name(world, "Boss_Dungeon").is_some();
                let boss_dead = !stored && !name_alive;

                if boss_dead {
                    self.open_gate_by_name(world, self.gate_4, "gate_north_4");
                    self.campaign.stage = Stage::Act5;
                    self.campaign.objective = "Leave the Citadel".into();
                    self.campaign.show_dialog("The Lord has fallen. Freedom awaits!", 5.0);
                }
            }
            Stage::Act5 | Stage::Victory => {}
        }

        if self.gate_1.is_none_or(|e| !world.entities().contains(&e)) {
            self.gate_1 = Self::find_by_name(world, "gate_east_1");
        }
        if self.gate_2.is_none_or(|e| !world.entities().contains(&e)) {
            self.gate_2 = Self::find_by_name(world, "gate_east_2");
        }
        if self.gate_4.is_none_or(|e| !world.entities().contains(&e)) {
            self.gate_4 = Self::find_by_name(world, "gate_north_4");
        }
        if self.gate_boss.is_none_or(|e| !world.entities().contains(&e)) {
            self.gate_boss = Self::find_by_name(world, "gate_boss");
        }
        if self.boss_entity.is_none_or(|e| !world.entities().contains(&e)) {
            self.boss_entity = Self::find_by_name(world, "Boss_Dungeon");
        }
    }

    fn on_animation_event(&mut self, world: &World, ev: AnimationEventTriggered) {
        let pos = crate::game::world_position(world, ev.entity).unwrap_or(Vec3::ZERO);
        match ev.name.as_str() {
            "footstep" => {
                log::debug!("[ANIM] footstep #{} at ({:.1},{:.1},{:.1})",
                    ev.entity, pos.x, pos.y, pos.z);
            }
            "hit" => {
                self.hud.push("⚔ Hit!");
            }
            "open_door" => {
                self.hud.push("🚪 Door opens");
            }
            _ => {
                log::debug!("[ANIM] event '{}' on #{} (clip '{}')",
                    ev.name, ev.entity, ev.clip);
            }
        }
    }
}

// ============================================================
// Default PostFx
// ============================================================
//
// FIX: туман 0.006 → 0.0006 (сцена не вымывается), shadow_bias
// увеличен вдвое (убирает черные зубцы на стенах Act1_*).

fn default_postfx() -> PostFx {
    PostFx {
        bloom_threshold: 0.9,
        bloom_strength: 0.55,
        bloom_knee: 0.5,
        bloom_radius: 1.1,
        exposure: 0.6,
        ssao_strength: 0.9,
        ssao_radius: 0.65,
        ibl_strength: 0.2,
        debug_view: DebugView::Final,
        fxaa_strength: 1.0,
        fog_color: [0.35, 0.40, 0.55],
        fog_density: 0.0008,
        fog_height_base: 0.0,
        fog_height_falloff: 0.06,
        vignette_strength: 0.15,
        film_grain: 0.03,
        chromatic_aberration: 0.0,
        shadow_bias: 0.0050,
        shadow_normal_bias: 5.0,
        shadow_fade_start: 80.0,
        shadow_fade_end: 120.0,
        lod_bias: 1.0,
        lod_distances: [30.0, 80.0, 200.0, 500.0],
        taa_strength: 1.0,
        taa_sharpening: 0.1,
        volumetric_density: 0.0006,
        volumetric_scattering: 0.35,
        volumetric_phase_g: 0.5,
        ..Default::default()
    }
}

// ============================================================
// Game impl
// ============================================================

impl Game for FortressDemo {
    fn init_with_assets(
        &mut self,
        _world: &mut World,
        renderer: &mut Renderer,
        assets: &crate::assets::AssetDatabase,
    ) {
        log::info!("FortressDemo: AssetDatabase {} assets", assets.len());

        renderer.add_mesh("cube", Mesh::cube(&renderer.device, 1.0));
        renderer.add_mesh("sphere", Mesh::sphere(&renderer.device, 0.5, 16, 24));
        renderer.add_mesh("ground", Mesh::plane(&renderer.device, 200.0, 1));
        renderer.add_mesh("quad", Mesh::plane(&renderer.device, 2.0, 1));
        renderer.add_mesh("quad_xy", Mesh::plane_xy(&renderer.device, 2.0, 1));
        renderer.add_mesh("cylinder", Mesh::cylinder(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("cone", Mesh::cone(&renderer.device, 0.5, 1.0, 24));
        renderer.add_mesh("capsule", Mesh::capsule(&renderer.device, 0.4, 0.8, 6, 20));

        use crate::render::props;
        renderer.add_mesh("crate_detail",    props::crate_detail(&renderer.device));
        renderer.add_mesh("barrel_detail",   props::barrel_detail(&renderer.device));
        renderer.add_mesh("column_fluted",   props::column_fluted(&renderer.device));
        renderer.add_mesh("pillar_ruined_a", props::pillar_ruined(&renderer.device, 1));
        renderer.add_mesh("pillar_ruined_b", props::pillar_ruined(&renderer.device, 2));
        renderer.add_mesh("pillar_ruined_c", props::pillar_ruined(&renderer.device, 3));
        renderer.add_mesh("chest_detail",    props::chest_detail(&renderer.device));
        renderer.add_mesh("torch_stand",     props::torch_stand(&renderer.device));
        renderer.add_mesh("brazier_detail",  props::brazier_detail(&renderer.device));
        renderer.add_mesh("tree_pine_a",     props::tree_pine(&renderer.device, 8.0));
        renderer.add_mesh("tree_pine_b",     props::tree_pine(&renderer.device, 6.0));
        renderer.add_mesh("tree_pine_c",     props::tree_pine(&renderer.device, 10.0));
        renderer.add_mesh("tree_oak_a",      props::tree_oak(&renderer.device, 7.0, 42));
        renderer.add_mesh("tree_oak_b",      props::tree_oak(&renderer.device, 6.0, 99));
        renderer.add_mesh("rock_cluster_a",  props::rock_cluster(&renderer.device, 0.8, 11));
        renderer.add_mesh("rock_cluster_b",  props::rock_cluster(&renderer.device, 0.5, 22));
        renderer.add_mesh("rock_cluster_c",  props::rock_cluster(&renderer.device, 1.2, 33));
        renderer.add_mesh("bench",           props::bench(&renderer.device));
        renderer.add_mesh("table",           props::table(&renderer.device));
        renderer.add_mesh("fence_post",      props::fence_post(&renderer.device));
        renderer.add_mesh("gravestone",      props::gravestone(&renderer.device));
        renderer.add_mesh("weapon_rack",     props::weapon_rack(&renderer.device));
        renderer.add_mesh("skull",           props::skull(&renderer.device));

        renderer.add_mesh("weapon_pistol",  props::weapon_pistol(&renderer.device));
        renderer.add_mesh("weapon_smg",     props::weapon_smg(&renderer.device));
        renderer.add_mesh("weapon_shotgun", props::weapon_shotgun(&renderer.device));
        renderer.add_mesh("weapon_sniper",  props::weapon_sniper(&renderer.device));
        renderer.add_mesh("weapon_default", props::weapon_default(&renderer.device));
        if !renderer.has_material("weapon_gunmetal") {
            renderer.add_material("weapon_gunmetal",
                Material::new([0.25, 0.25, 0.28, 1.0])
                    .with_metallic_roughness(0.85, 0.35));
        }

        let mut data = vec![0u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let idx = (y * 64 + x) * 4;
                let c = if ((x / 8) + (y / 8)) % 2 == 0 { 220 } else { 60 };
                data[idx] = c; data[idx + 1] = c; data[idx + 2] = c; data[idx + 3] = 255;
            }
        }
        renderer.load_texture_rgba("checker", &data, 64, 64).expect("checker");

        let mut blood = vec![0u8; 64 * 64 * 4];
        for y in 0..64 {
            for x in 0..64 {
                let idx = (y * 64 + x) * 4;
                let dx = x as f32 - 31.5;
                let dy = y as f32 - 31.5;
                let d = (dx * dx + dy * dy).sqrt() / 32.0;
                let a = (1.0 - d * 1.6).clamp(0.0, 1.0);
                let noise = ((x * 17 + y * 31) % 16) as f32 / 16.0 * 0.3;
                let alpha = (a * (1.0 - noise)).clamp(0.0, 1.0);
                blood[idx] = (140.0 * (1.0 - d * 0.5)) as u8;
                blood[idx + 1] = 20;
                blood[idx + 2] = 15;
                blood[idx + 3] = (alpha * 255.0) as u8;
            }
        }
        renderer.load_texture_rgba("arena_blood", &blood, 64, 64).expect("blood");

        let registry = WeaponRegistry::load_from_dir("assets/weapons")
            .ok()
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| {
                log::warn!("assets/weapons/ empty — using builtin set");
                WeaponRegistry::default_set()
            });
        let runtimes: Vec<WeaponRuntime> = registry.iter().map(WeaponRuntime::new).collect();
        self.current_weapon = 0;
        self.weapon_runtimes = runtimes;
        log::info!("weapons: {} loaded", registry.len());
        self.weapons = registry;

        self.item_registry = ItemRegistry::load_from_dir("assets/items")
            .ok()
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| {
                log::warn!("assets/items/ empty — using builtin set");
                ItemRegistry::default_set()
            });
        log::info!("items: {} loaded", self.item_registry.len());

        self.loot_registry = LootRegistry::load_from_dir("assets/loot")
            .ok()
            .filter(|r| !r.is_empty())
            .unwrap_or_else(|| {
                log::warn!("assets/loot/ empty — using builtin set");
                LootRegistry::default_set()
            });
        log::info!("loot: {} tables loaded", self.loot_registry.len());

        self.vendor_registry = VendorRegistry::load_from_dir("assets/vendors")
            .unwrap_or_else(|e| {
                log::warn!("vendors: failed to load: {:#}", e);
                VendorRegistry::empty()
            });
        log::info!("vendors: {} loaded", self.vendor_registry.len());

        self.crafting_registry = CraftingRegistry::load_from_dir("assets/recipes")
            .unwrap_or_else(|e| {
                log::warn!("recipes: failed to load: {:#}", e);
                CraftingRegistry::empty()
            });
        log::info!("recipes: {} loaded", self.crafting_registry.len());

        add_materials(renderer);

        let heightmap = crate::render::terrain::Heightmap::new_procedural(256, 2000.0, 42);
        let terrain_mesh = crate::render::terrain::generate_terrain_mesh(&renderer.device, &heightmap);
        renderer.add_mesh("terrain", terrain_mesh);
        let tex_data = crate::render::terrain::generate_terrain_texture(&heightmap, 2048);
        renderer.load_texture_rgba("terrain_tex", &tex_data, 2048, 2048).expect("terrain texture");
        renderer.add_material("terrain_mat",
            Material::new([1.0, 1.0, 1.0, 1.0])
                .with_metallic_roughness(0.0, 0.92)
                .with_texture("terrain_tex"));
        renderer.terrain = Some(heightmap);

        if let Ok(loaded) = crate::render::load_gltf_into(renderer, "assets/animated.glb", "anim") {
            self.skeletons = loaded.skeletons;
            self.animations = loaded.animations;
        }
        if let Ok(ev) = AnimationEvents::from_file("assets/animated.anim_events.ron") {
            self.animation_events = ev;
        }

        log::info!("FortressDemo: {} reflection probe slots ready", renderer.probes.len());
    }

    fn configure_input(&mut self, map: &mut InputMap) {
        map.bind("toggle_grid", Key::KeyG)
            .bind("toggle_culling", Key::KeyC)
            .bind("reload_shaders", Key::F12);
        for act in ["toggle_grid", "toggle_culling"] {
            map.action_in_context(act, "editor");
        }
    }

    fn wants_navmesh_bake(&mut self) -> bool {
        std::mem::take(&mut self.navmesh_bake_requested)
    }

    fn update(
        &mut self,
        world: &mut World,
        input: &Input,
        renderer: &mut Renderer,
        dt: f32,
    ) -> bool {
        if let Some(lang) = crate::game::locale::current_id() {
            if lang != self.current_dialog_lang {
                log::info!("Language changed to '{}', reloading dialogs...", lang);
                self.dialogs = DialogGraph::load_from_dir("assets/dialogs", &lang).unwrap_or_default();
                self.current_dialog_lang = lang;
                self.dialog_session = None;
            }
        }

        let in_play = input.play_mode;
        self.last_play_mode = in_play;

        self.rmb_pressed_this_frame = input.mouse_pressed(MouseButton::Right);
        self.vendor_esc_pressed = input.key_pressed(KeyCode::Escape)
            && self.vendor_session.is_some();
        self.crafting_esc_pressed = input.key_pressed(KeyCode::Escape)
            && self.crafting_session.is_some();

        let total_attrs = self.total_attributes();
        let max_hp = 100.0 + total_attrs.vitality as f32 * 10.0;
        self.demo_health_max = max_hp;
        if self.demo_health > self.demo_health_max {
            self.demo_health = self.demo_health_max;
        }

        if in_play && !self.demo_paused {
            if input.key_pressed(KeyCode::Tab) {
                self.show_character_sheet = !self.show_character_sheet;
                if self.show_character_sheet { self.show_inventory = false; }
            }
            if input.key_pressed(KeyCode::KeyI) {
                self.show_inventory = !self.show_inventory;
                if self.show_inventory { self.show_character_sheet = false; }
            }
        }
        if !in_play {
            self.show_character_sheet = false;
            self.show_inventory = false;
            self.inv_drag.cancel();
        }

        self.last_dt = dt;
        if self.weapon_recoil > 0.0 {
            let alpha = 1.0 - (-dt / 0.12_f32).exp();
            self.weapon_recoil = (self.weapon_recoil * (1.0 - alpha)).max(0.0);
        }
        self.auto_fire_accumulator = (self.auto_fire_accumulator - dt * 2.0).max(0.0);

        if !self.built {
            self.built = true;
            self.build(world);
            self.camera.target = ACT1_CENTER;
            self.camera.distance = 24.0;
            self.camera.yaw = -0.6;
            self.camera.pitch = 0.75;
        }

        if input.play_mode && !self.probe_baked_this_session {
            self.probe_baked_this_session = true;
            if !renderer.probes.is_empty() {
                let draws = self.collect_draws(world, renderer);
                let center = ACT2_CENTER + Vec3::Y * 3.0;
                let box_half = Vec3::splat(35.0);
                crate::render::renderer::probe::bake_probe(
                    renderer,
                    0,
                    &draws,
                    center,
                    center - box_half,
                    center + box_half,
                );
                log::info!("FortressDemo: probe 0 baked at {:?}", center);
            }
        }

        if input.play_mode && !self.was_in_play_mode {
            let p = self.camera.first_person_pos;
            let looks_default = p.x.abs() < 1.0 && (p.z - 45.0).abs() < 1.0;
            if looks_default {
                self.camera.first_person_pos = ACT1_CENTER + Vec3::new(0.0, 1.7, 0.0);
            }
            self.camera.yaw = std::f32::consts::PI;
            self.camera.pitch = 0.0;
            self.base_fov = self.camera.fov_y;
            self.ads_blend = 0.0;
            self.ads_active = false;
            self.auto_fire_accumulator = 0.0;
            self.weapon_raise_timer = 0.30;
            log::info!("Play entered at ({:.1}, {:.1}, {:.1})", p.x, p.y, p.z);
        }
        self.was_in_play_mode = input.play_mode;

        if self.pending_autosave {
            self.pending_autosave = false;
            self.save_slot(0, world, renderer);
        }
        if let Some(slot) = self.pending_save_slot.take() {
            self.save_slot(slot, world, renderer);
        }

        if input.pressed("reload_shaders")
            && let Err(e) = renderer.reload_shaders() {
                log::error!("Shader reload: {}", e);
            }
        let ctrl = input.key_down(KeyCode::ControlLeft) || input.key_down(KeyCode::ControlRight);
        let alt = input.key_down(KeyCode::AltLeft) || input.key_down(KeyCode::AltRight);
        if !ctrl && !alt {
            if input.pressed("toggle_grid") { self.show_grid = !self.show_grid; }
            if input.pressed("toggle_culling") { self.show_culling = !self.show_culling; }
        }
        if input.key_pressed(KeyCode::F1) {
            self.demo_show_debug = !self.demo_show_debug;
        }

        self.weapon_raise_timer = (self.weapon_raise_timer - dt).max(0.0);
        {
            let n = self.weapons.len().min(self.weapon_runtimes.len());
            for i in 0..n {
                let w = self.weapons.get(i).cloned();
                if let (Some(w), Some(rt)) = (w, self.weapon_runtimes.get_mut(i)) {
                    rt.tick(dt, &w);
                }
            }
        }
        if in_play && !self.demo_paused {
            if input.key_pressed(KeyCode::Digit1) { self.switch_weapon(0); }
            if input.key_pressed(KeyCode::Digit2) { self.switch_weapon(1); }
            if input.key_pressed(KeyCode::Digit3) { self.switch_weapon(2); }
            if input.key_pressed(KeyCode::Digit4) { self.switch_weapon(3); }
            if input.key_pressed(KeyCode::Digit5) { self.switch_weapon(4); }
            if input.key_pressed(KeyCode::Digit6) { self.switch_weapon(5); }
            if input.key_pressed(KeyCode::Digit7) { self.switch_weapon(6); }
            if input.key_pressed(KeyCode::Digit8) { self.switch_weapon(7); }
        }

        let rmb = input.mouse_down(MouseButton::Right);
        let want_ads = in_play && !self.demo_paused && rmb
            && self.weapon_raise_timer <= 0.0
            && !self.demo_reloading
            && !self.show_inventory;
        if want_ads != self.ads_active {
            self.ads_active = want_ads;
        }
        let ads_speed = if self.ads_active { 6.0 } else { 8.0 };
        let target = if self.ads_active { 1.0 } else { 0.0 };
        self.ads_blend += (target - self.ads_blend) * (dt * ads_speed).min(1.0);
        self.ads_blend = self.ads_blend.clamp(0.0, 1.0);
        if let Some(w) = self.current_weapon() {
            let fov_mult = 1.0 + (w.ads_fov_mult - 1.0) * self.ads_blend;
            self.camera.fov_y = self.base_fov * fov_mult;
        }

        let (rt_reloading, rt_reload_timer, rt_fire_cd) = self
            .current_runtime()
            .map(|rt| (rt.reloading, rt.reload_timer, rt.fire_cooldown))
            .unwrap_or((false, 0.0, 0.0));
        self.demo_reloading = rt_reloading;
        self.demo_reload_timer = rt_reload_timer;
        self.demo_fire_cooldown = rt_fire_cd;

        if in_play && !self.demo_paused && self.campaign.stage != Stage::Victory {
            let w_opt = self.current_weapon().cloned();
            let rt_opt = self.current_runtime().cloned();
            if let (Some(w), Some(rt)) = (w_opt, rt_opt) {
                if !rt.reloading && rt.mag == 0 && rt.reserve > 0
                    && let Some(rtm) = self.current_runtime_mut() {
                        rtm.start_reload(&w);
                        self.hud.push(format!("Reloading {}…", w.name));
                    }
                if input.key_pressed(KeyCode::KeyR) && !rt.reloading {
                    if rt.mag >= w.mag_size {
                        self.hud.push("Magazine full");
                    } else if rt.reserve == 0 {
                        self.hud.push("No reserve ammo");
                    } else if let Some(rtm) = self.current_runtime_mut() {
                        rtm.start_reload(&w);
                        self.hud.push(format!("Reloading {}…", w.name));
                    }
                }
            }
        }

        let lmb_down = input.mouse_down(MouseButton::Left);
        let lmb_pressed = input.mouse_pressed(MouseButton::Left);
        let w_opt = self.current_weapon().cloned();
        if let Some(w) = w_opt {
            let trigger = if w.auto { lmb_down } else { lmb_pressed };
            let can_shoot = in_play
                && !self.demo_paused
                && self.campaign.stage != Stage::Victory
                && self.weapon_raise_timer <= 0.0
                && !self.show_character_sheet
                && !self.show_inventory
                && self.dialog_session.is_none();

            if can_shoot && trigger {
                let rt_opt = self.current_runtime().cloned();
                if let Some(rt) = rt_opt
                    && rt.can_fire() {
                        if let Some(rtm) = self.current_runtime_mut() {
                            rtm.mag = rtm.mag.saturating_sub(1);
                            rtm.fire_cooldown = w.cooldown();
                        }

                        let spread_deg = if self.ads_active {
                            w.spread_ads
                        } else {
                            w.spread_hip
                        };
                        let origin = self.camera.position();
                        let base_dir = self.camera.forward();

                        let total = self.total_attributes();
                        let dmg_mult = total.ranged_damage_mult();
                        let crit_chance = total.crit_chance();
                        let crit_mult = total.crit_mult();

                        for _ in 0..w.pellets.max(1) {
                            let dir = apply_spread(base_dir, spread_deg);
                            let is_crit = rand01_crit() < crit_chance;
                            let mut damage = w.damage * dmg_mult;
                            if is_crit { damage *= crit_mult; }
                            world.send(ShotFired { origin, direction: dir, damage, is_crit });
                        }

                        let ads_mult = if self.ads_active { 0.5 } else { 1.0 };
                        let kick_up = w.recoil_up * ads_mult
                            + self.auto_fire_accumulator * 0.002;
                        self.camera.add_recoil(kick_up);
                        if w.recoil_side > 1e-6 {
                            let sign = if rand01_side() > 0.5 { 1.0 } else { -1.0 };
                            self.camera.yaw += sign * w.recoil_side * ads_mult;
                        }

                        self.auto_fire_accumulator =
                            (self.auto_fire_accumulator + 0.6).min(1.5);
                        self.weapon_recoil = (self.weapon_recoil + 0.35).min(1.0);
                    }
            }
        }

        if !input.play_mode && !input.editor_flying && !self.demo_paused {
            let lmb = input.mouse_down(MouseButton::Left) && !input.editor_captured;
            if lmb {
                let (dx, dy) = input.mouse_delta;
                if self.dragging {
                    self.camera.orbit(dx * 0.005, dy * 0.005);
                }
                self.dragging = true;
            } else {
                self.dragging = false;
            }
            if input.scroll_delta.abs() > 0.01 {
                self.camera.zoom(input.scroll_delta * 0.05);
            }
            let speed = 15.0 * dt;
            let mut pan = (0.0, 0.0);
            if input.key_down(KeyCode::KeyW) { pan.1 -= speed; }
            if input.key_down(KeyCode::KeyS) { pan.1 += speed; }
            if input.key_down(KeyCode::KeyA) { pan.0 -= speed; }
            if input.key_down(KeyCode::KeyD) { pan.0 += speed; }
            if pan != (0.0, 0.0) {
                self.camera.pan(pan.0, pan.1);
            }
        }

        if in_play && !self.demo_paused && self.campaign.stage != Stage::Victory {
            self.campaign.playtime += dt;
        }
        self.campaign.tick_dialog(dt);

        if input.play_mode {
            if let Some(t) = self.ai_target_entity {
                let p = self.camera.position();
                if let Some(tr) = world.get_mut::<Transform>(t) {
                    tr.position = Vec3::new(p.x, p.y - 0.8, p.z);
                }
                world.insert(t, Visible(false));
            }
        } else if let Some(t) = self.ai_target_entity {
            world.insert(t, Visible(true));
        }

        if let Some(t) = self.ai_target_entity
            && let Some(h) = world.get_mut::<Health>(t) {
                let damage = (self.last_target_hp - h.current).max(0.0);
                if damage > 0.001 {
                    self.demo_health = (self.demo_health - damage).max(0.0);
                    self.hud.push(format!("-{:.0} HP", damage));
                }
                h.current = self.demo_health_max;
                self.last_target_hp = self.demo_health_max;
            }

        if in_play {
            self.process_triggers(world);
            self.check_stage_transitions(world);
        }

        let finished: Vec<Entity> = world.read_events::<TimerFinished>()
            .map(|ev| ev.entity)
            .collect();
        for e in finished {
            if Some(e) == self.bell_entity {
                if let Some(src) = world.get_mut::<AudioSource>(e) {
                    src.playing = true;
                }
                if let Some(t) = world.get_mut::<Timer>(e) {
                    t.restart();
                }
            }
        }

        if in_play && !self.demo_paused && self.dialog_session.is_none() {
            if input.key_pressed(KeyCode::KeyE) {
                let cam_pos = self.camera.position();
                let cam_dir = self.camera.forward();
                if let Some(npc_e) = crate::game::dialogs::find_npc_in_front(
                    world, cam_pos, cam_dir, 3.0, 0.85,
                ) {
                    if let Some(npc) = world.get::<DialogNpc>(npc_e).cloned() {
                        if let Some(graph) = self.dialogs.get(&npc.dialog_id).cloned() {
                            if let Some(session) = DialogSession::new(graph, npc_e) {
                                log::info!("Dialog started: {} with NPC #{}", npc.dialog_id, npc_e);
                                self.dialog_session = Some(session);
                            }
                        }
                    }
                }
            }
        }

        for sys in self.systems.iter_mut() {
            sys.update(world, dt);
        }

        self.tick_gates(world);

        let anim_events = self.animation_runtime.advance_all(
            world, renderer,
            &self.animations, &self.skeletons, &self.animation_events,
            dt,
        );
        for ev in anim_events {
            self.on_animation_event(world, ev);
        }

        true
    }

    fn apply_postfx(&mut self, postfx: PostFx) {
        self.postfx = postfx;
    }

    fn take_hit_stop(&mut self) -> Option<(f32, f32)> {
        self.pending_hit_stop.take()
    }

    fn on_play_enter(&mut self, _world: &World) -> Option<Box<dyn Any>> {
        Some(Box::new(()))
    }

    fn on_play_exit(&mut self, _state: Box<dyn Any>) {}

    fn on_pause_changed(&mut self, paused: bool) {
        if self.demo_paused != paused {
            self.demo_paused = paused;
            log::info!("FortressDemo: pause = {}", paused);
            if paused {
                self.inv_drag.cancel();
            }
        }
    }

    fn save_game_state(&self) -> Option<String> {
        #[derive(serde::Serialize)]
        struct StateSnapshot<'a> {
            campaign: &'a CampaignState,
            camera_pos: [f32; 3],
            camera_target: [f32; 3],
            camera_distance: f32,
            camera_yaw: f32,
            camera_pitch: f32,
            hp: f32,
            current_weapon: usize,
            weapon_runtimes: &'a [WeaponRuntime],
            player_stats: &'a PlayerStats,
            inventory: &'a Inventory,
            equipment: &'a Equipment,
        }

        let snap = StateSnapshot {
            campaign: &self.campaign,
            camera_pos: self.camera.first_person_pos.to_array(),
            camera_target: self.camera.target.to_array(),
            camera_distance: self.camera.distance,
            camera_yaw: self.camera.yaw,
            camera_pitch: self.camera.pitch,
            hp: self.demo_health,
            current_weapon: self.current_weapon,
            weapon_runtimes: &self.weapon_runtimes,
            player_stats: &self.player_stats,
            inventory: &self.inventory,
            equipment: &self.equipment,
        };
        ron::ser::to_string(&snap).ok()
    }

    fn load_game_state(&mut self, ron: &str) {
        #[derive(serde::Deserialize)]
        struct StateSnapshot {
            campaign: CampaignState,
            camera_pos: [f32; 3],
            camera_target: [f32; 3],
            camera_distance: f32,
            camera_yaw: f32,
            camera_pitch: f32,
            hp: f32,
            #[serde(default)] current_weapon: usize,
            #[serde(default)] weapon_runtimes: Vec<WeaponRuntime>,
            #[serde(default)] player_stats: PlayerStats,
            #[serde(default)] inventory: Option<Inventory>,
            #[serde(default)] equipment: Equipment,
        }

        match ron::from_str::<StateSnapshot>(ron) {
            Ok(snap) => {
                self.campaign = snap.campaign;
                self.campaign.processed.clear();
                self.campaign.dialog = None;

                self.camera.first_person_pos = Vec3::from_array(snap.camera_pos);
                self.camera.target = Vec3::from_array(snap.camera_target);
                self.camera.distance = snap.camera_distance;
                self.camera.yaw = snap.camera_yaw;
                self.camera.pitch = snap.camera_pitch;
                self.demo_health = snap.hp;

                if !snap.weapon_runtimes.is_empty()
                    && snap.weapon_runtimes.len() == self.weapons.len()
                {
                    self.weapon_runtimes = snap.weapon_runtimes;
                    for rt in self.weapon_runtimes.iter_mut() {
                        rt.fire_cooldown = 0.0;
                        rt.reloading = false;
                        rt.reload_timer = 0.0;
                    }
                }
                self.current_weapon = snap.current_weapon.min(
                    self.weapons.len().saturating_sub(1),
                );
                self.weapon_raise_timer = 0.30;

                self.player_stats = snap.player_stats;
                if let Some(inv) = snap.inventory {
                    self.inventory = inv;
                }
                self.equipment = snap.equipment;
                self.demo_health_max = self.total_attributes().vitality as f32 * 10.0 + 100.0;
                self.demo_health = self.demo_health.min(self.demo_health_max);

                self.inv_drag.cancel();
                self.hud.push("📂 Game loaded");
            }
            Err(e) => {
                log::error!("load_game_state parse failed: {}", e);
                self.hud.push("Load state failed");
            }
        }
    }

    fn load_world_request(&mut self) -> Option<WorldLoadRequest> {
        self.pending_world_request.take()
    }

    fn take_quit_to_menu(&mut self) -> bool {
        std::mem::take(&mut self.pending_quit_to_menu)
    }

    fn on_kill(&mut self, world: &mut World, target: Entity) {
        self.campaign.kills_total += 1;
        self.campaign.kills_in_stage += 1;

        let target_name = world.get::<Name>(target).map(|n| n.0.clone()).unwrap_or_default();

        let rewards = self.quest_manager.on_enemy_killed(&target_name);
        if !rewards.is_empty() {
            self.hud.push("⭐ Quest Completed!");
            for reward in rewards {
                match reward {
                    Reward::Xp(amt) => {
                        let levels = self.player_stats.gain_xp(amt);
                        self.hud.push(format!("+{} XP", amt));
                        if levels > 0 {
                            self.hud.push(format!("⭐ LEVEL UP! → Lv {}", self.player_stats.experience.level));
                        }
                    }
                    Reward::Gold(amt) => {
                        self.campaign.gold += amt;
                        self.hud.push(format!("+{} Gold", amt));
                    }
                    Reward::Item { id, count } => {
                        self.try_pickup(&id, count);
                    }
                }
            }
        }

        self.hud.push(format!(
            "Kill #{} (stage: {}/{})",
            self.campaign.kills_total,
            self.campaign.kills_in_stage,
            self.campaign.kills_required,
        ));

        let strength = (0.04 + self.auto_fire_accumulator * 0.02).min(0.08);
        self.pending_hit_stop = Some((strength, 0.12));
        self.auto_fire_accumulator = 0.0;

        let xp_gain = 25u32;
        let levels = self.player_stats.gain_xp(xp_gain);
        if levels > 0 {
            let new_level = self.player_stats.experience.level;
            let hp_before = self.demo_health;
            self.demo_health_max = self.total_attributes().vitality as f32 * 10.0 + 100.0;
            self.demo_health = self.demo_health_max;
            self.hud.push(format!(
                "⭐ LEVEL UP! → Lv {} · +{} HP · +{} stat points",
                new_level,
                (self.demo_health - hp_before).max(0.0) as i32,
                levels * 2,
            ));
        }

        let pos = world.get::<Transform>(target).map(|t| t.position).unwrap_or(Vec3::ZERO);
        let table_id = if target_name.contains("Boss") {
            "enemy_boss"
        } else if target_name.contains("Elite")
            || target_name.contains("Guard")
            || target_name.contains("Tank")
            || target_name.contains("Mage")
            || target_name.contains("Archer")
            || target_name.contains("Assassin")
        {
            "enemy_elite"
        } else {
            "enemy_patrol"
        };

        if let Some(table) = self.loot_registry.get(table_id).cloned() {
            let drops = roll_loot(&table);
            for (i, stack) in drops.iter().enumerate() {
                let item = self.item_registry.get_by_id(&stack.item_id).cloned();
                let (mesh, mat, tint) = match item.as_ref().map(|i| i.kind) {
                    Some(ItemKind::Consumable) => ("cube", "emissive_cold", [0.5, 1.0, 0.5, 1.0]),
                    Some(ItemKind::Armor) => ("cube", "arena_crate", [0.8, 0.5, 0.2, 1.0]),
                    Some(ItemKind::Trinket) => ("sphere", "gold", [1.0, 0.9, 0.3, 1.0]),
                    _ => ("cylinder", "gold", [1.0, 0.85, 0.3, 1.0]),
                };

                let e = world.spawn();
                let offset = Vec3::new(
                    (i as f32 - drops.len() as f32 * 0.5) * 0.7,
                    0.5,
                    ((i as f32 * 1.3) % 1.0 - 0.5) * 0.7,
                );
                world.insert(e, Name(format!("Pickup_{}_{}", stack.item_id, i)));
                world.insert(e, Transform::at(pos + offset).with_scale(0.4));
                world.insert(e, MeshHandle(mesh.into()));
                world.insert(e, MaterialHandle(mat.into()));
                world.insert(e, Tint(tint));
                world.insert(e, Spinner::new(Vec3::Y, 2.0));
                world.insert(e, Trigger::repeatable(1.5,
                    TriggerAction::PlaySound("pickup".into())));
                world.insert(e, GoldValue(stack.count));
            }
            log::info!("Loot: {} stacks dropped at {:?}", drops.len(), pos);
        }
    }

    fn collect_draws(&mut self, world: &mut World, renderer: &Renderer) -> Vec<MeshDraw> {
        use std::collections::HashMap;
        type BucketKey = (String, String, [u32; 4], bool, bool, [u32; 3]);
        let mut buckets: HashMap<BucketKey, Vec<InstanceData>> = HashMap::new();
        let planes = self.camera.frustum_planes();
        let cam_pos = self.camera.position();

        let lod_bias = self.postfx.lod_bias.max(0.01);
        let lod_dists = self.postfx.lod_distances;

        let mut new_prev: HashMap<Entity, glam::Mat4> = HashMap::new();
        let entities: Vec<_> = world.entities().to_vec();

        for e in entities {
            if let Some(v) = world.get::<Visible>(e)
                && !v.0 { continue; }
            if world.has::<Decal>(e) { continue; }

            let (Some(_t), Some(m), Some(mat)) = (
                world.get::<Transform>(e),
                world.get::<MeshHandle>(e),
                world.get::<MaterialHandle>(e),
            ) else { continue };

            let model = crate::game::world_matrix(world, e);
            let prev_model = self.prev_world_matrices.get(&e).copied().unwrap_or(model);
            new_prev.insert(e, model);

            let Some(mesh) = renderer.meshes.get(&m.0) else { continue; };
            let (world_center, world_radius) = mesh.world_bounds(&model);

            if self.show_culling {
                let mut visible = true;
                for p in &planes {
                    let d = p.x * world_center.x
                          + p.y * world_center.y
                          + p.z * world_center.z
                          + p.w;
                    if d < -world_radius { visible = false; break; }
                }
                if !visible { continue; }
            }

            let dist = (world_center - cam_pos).length();
            let dist_eff = (dist - world_radius).max(0.0) / lod_bias;
            let lod_level = if mesh.lods.is_empty() || dist_eff < lod_dists[0] {
                0
            } else if dist_eff < lod_dists[1] {
                1.min(mesh.lods.len())
            } else if dist_eff < lod_dists[2] {
                2.min(mesh.lods.len())
            } else {
                3.min(mesh.lods.len())
            };

            let mesh_name = if lod_level == 0 {
                m.0.clone()
            } else {
                format!("{}__lod{}", m.0, lod_level - 1)
            };

            let material = renderer.materials.get(&mat.0)
                .unwrap_or_else(|| renderer.materials_default());
            let blend = material.alpha_mode == AlphaMode::Blend;
            let double_sided = material.double_sided;

            let color = world.get::<Tint>(e).map(|t| t.0).unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let tiling_size = world.get::<TextureTiling>(e).map(|t| t.size).unwrap_or(1.0);

            let (scale, _, _) = model.to_scale_rotation_translation();
            let scale_abs = scale.abs();
            let local_extent = (mesh.aabb_max - mesh.aabb_min).abs();
            let we = local_extent * scale_abs;
            let tile = tiling_size.max(0.001);

            let sx = if we.x < 0.001 { 1.0 } else { we.x / tile };
            let sy = if we.y < 0.001 { 1.0 } else { we.y / tile };
            let sz = if we.z < 0.001 { 1.0 } else { we.z / tile };

            let key = (
                mesh_name,
                mat.0.clone(),
                color.map(f32::to_bits),
                blend,
                double_sided,
                [(sx * 1000.0) as i32 as u32, (sy * 1000.0) as i32 as u32, (sz * 1000.0) as i32 as u32],
            );
            let inst = InstanceData::new_full(model, prev_model, color, [sx, sy, sz]);
            buckets.entry(key).or_default().push(inst);
        }

        self.prev_world_matrices = new_prev;

        let mut result: Vec<MeshDraw> = buckets
            .into_iter()
            .map(
                |((mesh, material_name, _, blend, double_sided, _), instances)| MeshDraw {
                    mesh,
                    instances,
                    texture: Some(material_name),
                    blend,
                    double_sided,
                },
            )
            .collect();

        // FIX: ViewModel — оружие в руках, привязано к камере.
        if self.last_play_mode
            && !self.demo_paused
            && !self.show_inventory
            && !self.show_character_sheet
            && self.dialog_session.is_none()
            && self.campaign.stage != Stage::Victory
        {
            if let Some(w) = self.current_weapon() {
                let mesh_name = w
                    .mesh
                    .clone()
                    .unwrap_or_else(|| "weapon_default".to_string());
                let mat_name = w
                    .material
                    .clone()
                    .unwrap_or_else(|| "weapon_gunmetal".to_string());

                let mut pos = Vec3::from_array(w.hip_pos);
                let mut rot = Quat::from_euler(
                    glam::EulerRot::XYZ,
                    w.hip_rot[0],
                    w.hip_rot[1],
                    w.hip_rot[2],
                );

                if self.ads_blend > 0.0 {
                    let ads_p = Vec3::from_array(w.ads_pos);
                    let ads_r = Quat::from_euler(
                        glam::EulerRot::XYZ,
                        w.ads_rot[0],
                        w.ads_rot[1],
                        w.ads_rot[2],
                    );
                    pos = pos.lerp(ads_p, self.ads_blend);
                    rot = rot.slerp(ads_r, self.ads_blend);
                }

                pos.z += self.weapon_recoil * w.kick_back;
                pos.y += self.weapon_recoil * w.kick_back * 0.5;

                if self.demo_reloading {
                    let p = self
                        .current_runtime()
                        .map(|r| r.reload_progress(w))
                        .unwrap_or(0.0);
                    let dip = (p * std::f32::consts::PI).sin();
                    pos.y -= dip * 0.15;
                    pos.x -= dip * 0.05;
                }

                if self.weapon_raise_timer > 0.0 {
                    let t = (self.weapon_raise_timer / 0.30).clamp(0.0, 1.0);
                    pos.y -= t * 0.3;
                    pos.z += t * 0.2;
                }

                let local_mat =
                    Mat4::from_scale_rotation_translation(Vec3::splat(w.scale), rot, pos);
                let inv_view = self.camera.view_matrix().inverse();
                let final_mat = inv_view * local_mat;

                result.push(MeshDraw {
                    mesh: mesh_name,
                    instances: vec![InstanceData::new_full(
                        final_mat,
                        final_mat,
                        [1.0, 1.0, 1.0, 1.0],
                        [1.0, 1.0, 1.0],
                    )],
                    texture: Some(mat_name),
                    blend: false,
                    double_sided: false,
                });
            }
        }

        result
    }

    fn collect_lines(
        &mut self,
        world: &mut World,
        renderer: &Renderer,
        selected: &[Entity],
    ) -> Vec<LineVertex> {
        let mut batch = LineBatch::new();

        if self.show_grid {
            batch.grid(120.0, 2.0,
                [0.15, 0.18, 0.22, 0.5], [0.35, 0.40, 0.48, 0.7], 5);
            batch.axes(4.0);
        }

        for (e, agent) in world.query::<AiAgent>() {
            if !world.has::<DebugPath>(e) { continue; }
            if agent.path.len() < 2 { continue; }
            for w in agent.path.windows(2) {
                batch.line(
                    w[0] + Vec3::Y * 0.15,
                    w[1] + Vec3::Y * 0.15,
                    [1.0, 0.9, 0.2, 0.85],
                );
            }
        }

        for (e, agent) in world.query::<AiAgent>() {
            let Some(t) = world.get::<Transform>(e) else { continue };
            let eye = t.position + Vec3::Y * 1.0;
            let fwd = Vec3::new(-agent.yaw.sin(), 0.0, -agent.yaw.cos());
            let color = match agent.state {
                AiState::Idle | AiState::Patrol => [0.4, 0.9, 0.4, 0.6],
                AiState::Investigate => [0.9, 0.7, 0.3, 0.7],
                AiState::Chase => [1.0, 0.8, 0.2, 0.8],
                AiState::Attack => [1.0, 0.3, 0.3, 0.85],
                AiState::Dead => [0.4, 0.4, 0.4, 0.3],
            };
            let range = agent.vision_range.min(10.0);
            let half = agent.vision_angle_cos.acos();
            batch.line(eye, eye + fwd * range, color);
            let l = Quat::from_axis_angle(Vec3::Y, half) * fwd;
            let r = Quat::from_axis_angle(Vec3::Y, -half) * fwd;
            batch.line(eye, eye + l * range, color);
            batch.line(eye, eye + r * range, color);
        }

        for &e in selected {
            if let (Some(_), Some(mh)) = (world.get::<Transform>(e), world.get::<MeshHandle>(e))
                && let Some(mesh) = renderer.meshes.get(&mh.0) {
                    let model = crate::game::world_matrix(world, e);
                    let (center, radius) = mesh.world_bounds(&model);
                    batch.sphere_wireframe(center, radius * 1.05, [1.0, 0.85, 0.2, 1.0], 24);
                }
        }

        batch.vertices().to_vec()
    }

    fn dir_lights(&self, world: &World) -> Vec<GpuLight> {
        world.query::<DirectionalLight>().map(|(_, l)| GpuLight {
            direction: [l.direction.x, l.direction.y, l.direction.z, l.intensity],
            color: [l.color[0], l.color[1], l.color[2], 0.0],
        }).take(4).collect()
    }

    fn point_lights(&self, world: &World) -> Vec<GpuPointLight> {
        world.query::<PointLight>().filter_map(|(e, l)| {
            let t = world.get::<Transform>(e)?;
            Some(GpuPointLight {
                position: [t.position.x, t.position.y, t.position.z, l.range],
                color: [l.color[0], l.color[1], l.color[2], l.intensity],
            })
        }).take(16).collect()
    }

    fn ambient(&self) -> [f32; 3] { [0.05, 0.06, 0.09] }
    fn postfx(&self) -> PostFx { self.postfx }
    fn camera(&self) -> &Camera3D { &self.camera }
    fn camera_mut(&mut self) -> &mut Camera3D { &mut self.camera }

    fn collect_ui(
        &mut self,
        world: &mut World,
        _renderer: &Renderer,
        ui: &mut crate::ui::UiLayer,
    ) {
        self.hud.tick(self.last_dt);
        if !self.last_play_mode { return; }
        if self.vendor_session.is_some() && self.vendor_esc_pressed {
            self.vendor_session = None;
            self.vendor_esc_pressed = false;
        }
        if self.crafting_session.is_some() && self.crafting_esc_pressed {
            self.crafting_session = None;
            self.crafting_esc_pressed = false;
        }

        let is_victory = self.campaign.stage == Stage::Victory;

        if self.demo_show_debug {
            let real_fps = if self.last_dt > 1e-4 { 1.0 / self.last_dt } else { 0.0 };
            self.hud.debug_overlay(ui, real_fps, world.len(), self.camera.position());
        }

        if !is_victory && self.ads_blend < 0.5 && self.dialog_session.is_none() {
            let has_ammo = self.current_runtime().map(|r| r.mag > 0).unwrap_or(false);
            self.hud.crosshair(ui, has_ammo);
        }

        self.hud.health_bar(ui, self.demo_health, self.demo_health_max);

        if !is_victory {
            let sh = ui.screen_h();
            let bar_w = 220.0;
            let bar_h = 12.0;
            let x = 24.0;
            let y = sh - 24.0 - 22.0 - 6.0 - bar_h;
            let frac = self.player_stats.experience.progress();

            ui.rect(x - 6.0, y - 4.0, bar_w + 12.0, bar_h + 8.0, [0.0, 0.0, 0.0, 0.55]);
            ui.bar(x, y, bar_w, bar_h, frac, [0.5, 0.7, 1.0, 1.0], [0.1, 0.1, 0.15, 0.9]);

            let label = format!(
                "Lv {} · {}/{} XP",
                self.player_stats.experience.level,
                self.player_stats.experience.xp,
                self.player_stats.experience.xp_to_next,
            );
            ui.text_centered(x + bar_w * 0.5, y + bar_h * 0.5, &label, 10.0,
                [0.9, 0.95, 1.0, 1.0]);
        }

        if self.player_stats.unspent_points > 0 && !is_victory && !self.show_character_sheet {
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            let text = format!(
                "⭐ {} unspent stat points — press Tab",
                self.player_stats.unspent_points
            );
            let tw = ui.text_width(&text, 14.0);
            let y = sh - 24.0 - 22.0 - 6.0 - 12.0 - 22.0;
            ui.rect(sw * 0.5 - tw * 0.5 - 12.0, y - 4.0, tw + 24.0, 22.0,
                [0.15, 0.1, 0.0, 0.75]);
            ui.text_centered(sw * 0.5, y + 11.0, &text, 14.0, [1.0, 0.9, 0.5, 1.0]);
        }

        if let (Some(w), Some(rt)) = (self.current_weapon(), self.current_runtime()) {
            let weapon_name = w.name.clone();
            let mag = rt.mag;
            let reserve = rt.reserve;
            let mag_size = w.mag_size;
            let reloading = rt.reloading;
            let reload_progress = rt.reload_progress(w);
            let out_of_ammo = mag == 0 && reserve == 0;

            let ammo_text = format!("{}/{} · reserve {}", mag, mag_size, reserve);
            let size = 22.0;
            let name_size = 14.0;
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            let pad = 14.0;

            let ammo_w = ui.text_width(&ammo_text, size);
            let name_w = ui.text_width(&weapon_name, name_size);
            let box_w = ammo_w.max(name_w) + pad * 2.0;
            let box_h = size + name_size + 12.0 + pad * 1.2;
            let x = sw - 24.0 - box_w;
            let y = sh - 24.0 - box_h;

            let color = if reloading { [1.0, 0.85, 0.4, 1.0] }
                else if out_of_ammo { [1.0, 0.3, 0.3, 1.0] }
                else if mag == 0 { [1.0, 0.5, 0.4, 1.0] }
                else { [1.0, 1.0, 0.85, 1.0] };

            ui.rect(x, y, box_w, box_h, [0.0, 0.0, 0.0, 0.6]);
            ui.rect_outline(x, y, box_w, box_h, 1.0, [0.5, 0.55, 0.65, 0.7]);
            ui.text(x + pad, y + 4.0, &weapon_name, name_size, [0.75, 0.85, 1.0, 1.0]);
            ui.text(x + pad, y + name_size + 6.0, &ammo_text, size, color);

            if reloading {
                let bar_h = 6.0;
                let bar_y = y - bar_h - 6.0;
                ui.rect(x, bar_y, box_w, bar_h, [0.15, 0.15, 0.18, 0.9]);
                ui.rect(x, bar_y, box_w * reload_progress, bar_h, [0.6, 0.9, 1.0, 1.0]);
                ui.text_centered(x + box_w * 0.5, bar_y - 10.0, "RELOADING",
                    11.0, [0.8, 0.9, 1.0, 1.0]);
            }
        }

        if let Some(rt) = self.current_runtime()
            && rt.mag == 0 && rt.reserve == 0 && !is_victory {
                ui.text_centered(ui.screen_w() * 0.5, ui.screen_h() * 0.5 + 60.0,
                    "OUT OF AMMO", 28.0, [1.0, 0.3, 0.3, 0.9]);
            }

        if self.ads_blend > 0.5 && !is_victory {
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            let cx = sw * 0.5;
            let cy = sh * 0.5;
            let r = 220.0;
            let alpha = (self.ads_blend - 0.5) * 2.0 * 0.75;
            ui.rect(0.0, 0.0, cx - r, sh, [0.0, 0.0, 0.0, alpha]);
            ui.rect(cx + r, 0.0, cx - r, sh, [0.0, 0.0, 0.0, alpha]);
            ui.rect(cx - r, 0.0, r * 2.0, cy - r, [0.0, 0.0, 0.0, alpha]);
            ui.rect(cx - r, cy + r, r * 2.0, cy - r, [0.0, 0.0, 0.0, alpha]);
        }

        self.hud.notifications(ui);

        if !is_victory {
            let sw = ui.screen_w();
            let size = 18.0;
            let title = self.campaign.stage.title();
            let obj = &self.campaign.objective;
            let title_w = ui.text_width(title, 14.0);
            let obj_w = ui.text_width(obj, size);
            let box_w = title_w.max(obj_w) + 32.0;
            let box_h = 56.0;
            let x = sw * 0.5 - box_w * 0.5;
            let y = 16.0;

            ui.rect(x, y, box_w, box_h, [0.05, 0.07, 0.12, 0.75]);
            ui.rect_outline(x, y, box_w, box_h, 1.5, [0.5, 0.6, 0.85, 0.9]);
            ui.text_centered(sw * 0.5, y + 14.0, title, 14.0, [0.6, 0.75, 1.0, 1.0]);
            ui.text_centered(sw * 0.5, y + 38.0, obj, size, [1.0, 0.95, 0.75, 1.0]);
        }

        {
            let sw = ui.screen_w();
            let size = 14.0;
            let mut y = 90.0;

            if self.campaign.kills_required > 0 {
                let kills_txt = format!(
                    "⚔ Kills {}/{} (total {})",
                    self.campaign.kills_in_stage,
                    self.campaign.kills_required,
                    self.campaign.kills_total
                );
                let kw = ui.text_width(&kills_txt, size);
                ui.rect(sw - kw - 40.0, y - 6.0, kw + 24.0, size + 12.0,
                    [0.0, 0.0, 0.0, 0.55]);
                ui.text(sw - kw - 28.0, y, &kills_txt, size, [1.0, 0.85, 0.85, 1.0]);
                y += size + 10.0;
            }
            if self.campaign.has_key_red {
                let k = "🔑 Red Key";
                let kww = ui.text_width(k, size);
                ui.rect(sw - kww - 40.0, y - 6.0, kww + 24.0, size + 12.0,
                    [0.0, 0.0, 0.0, 0.55]);
                ui.text(sw - kww - 28.0, y, k, size, [1.0, 0.4, 0.4, 1.0]);
            }
        }

        if let Some((text, ttl)) = &self.campaign.dialog {
            let alpha = ttl.min(1.0);
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            let size = 16.0;
            let tw = ui.text_width(text, size);
            let box_w = tw + 40.0;
            let box_h = size + 24.0;
            let x = sw * 0.5 - box_w * 0.5;
            let y = sh - 140.0;

            ui.rect(x, y, box_w, box_h, [0.0, 0.0, 0.0, 0.75 * alpha]);
            ui.rect_outline(x, y, box_w, box_h, 1.0, [0.6, 0.7, 0.9, 0.9 * alpha]);
            ui.text_centered(sw * 0.5, y + box_h * 0.5, text, size, [1.0, 1.0, 1.0, alpha]);
        }

        if is_victory {
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.7]);
            ui.text_centered(sw * 0.5, sh * 0.3, "VICTORY", 64.0, [1.0, 0.85, 0.4, 1.0]);
            ui.text_centered(sw * 0.5, sh * 0.42, "The Fallen Citadel", 24.0,
                [0.8, 0.85, 0.95, 1.0]);
            let stats = format!(
                "Kills: {}  ·  Time: {:.1}s  ·  Level: {}",
                self.campaign.kills_total,
                self.campaign.victory_time.unwrap_or(0.0),
                self.player_stats.experience.level,
            );
            ui.text_centered(sw * 0.5, sh * 0.55, &stats, 20.0, [1.0, 1.0, 1.0, 1.0]);
        }

        if self.demo_paused {
            if self.save_slots_cache_dirty {
                self.refresh_save_slots();
            }

            let sw = ui.screen_w();
            let sh = ui.screen_h();
            ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.65]);

            let pw = 500.0;
            let ph = 490.0;
            let px = (sw - pw) * 0.5;
            let py = (sh - ph) * 0.5;
            ui.rect(px - 2.0, py - 2.0, pw + 4.0, ph + 4.0, [0.55, 0.6, 0.75, 1.0]);
            ui.rect(px, py, pw, ph, [0.13, 0.15, 0.20, 0.98]);

            ui.text_centered(sw * 0.5, py + 30.0, "PAUSED", 28.0, [1.0, 0.9, 0.6, 1.0]);
            ui.text_centered(sw * 0.5, py + 58.0, self.campaign.stage.title(),
                14.0, [0.7, 0.75, 0.85, 1.0]);

            let bw = 220.0;
            let bh = 36.0;
            let bx = sw * 0.5 - bw * 0.5;

            if ui.button(bx, py + 82.0, bw, bh, "Resume") {
                self.demo_paused = false;
            }
            if ui.button(bx, py + 122.0, bw, bh, "Heal +25") {
                self.heal(25.0);
                self.hud.push("Healed +25");
            }
            if ui.button(bx, py + 162.0, bw, bh, "Refill Ammo") {
                for i in 0..self.weapons.len() {
                    if let (Some(w), Some(rt)) = (
                        self.weapons.get(i).cloned(),
                        self.weapon_runtimes.get_mut(i),
                    ) {
                        rt.mag = w.mag_size;
                        rt.reserve = w.start_reserve.max(rt.reserve);
                        rt.reloading = false;
                        rt.reload_timer = 0.0;
                    }
                }
                self.hud.push("Ammo refilled");
            }
            if ui.button(bx, py + 202.0, bw, bh, "🏠 Quit to Main Menu") {
                self.pending_quit_to_menu = true;
                self.demo_paused = false;
            }

            ui.text_centered(sw * 0.5, py + 258.0, "SAVE / LOAD", 16.0,
                [0.8, 0.85, 1.0, 1.0]);

            let slot_w = 420.0;
            let slot_h = 32.0;
            let slot_x = sw * 0.5 - slot_w * 0.5;
            let mut y = py + 285.0;

            let slots = self.save_slots_cache.clone();
            let mut action: Option<(u32, &'static str)> = None;

            for (slot, header) in &slots {
                let label = match header {
                    Some(h) => format!(
                        "Slot {}  ·  {}  ·  {} kills  ·  {:.0}s",
                        slot + 1, h.stage_title, h.kills, h.playtime_secs,
                    ),
                    None => format!("Slot {}  ·  — empty —", slot + 1),
                };

                let btn_w = 44.0;
                let gap = 4.0;

                if ui.button(slot_x, y, btn_w, slot_h, "S") {
                    action = Some((*slot, "save"));
                }
                if header.is_some() && ui.button(slot_x + btn_w + gap, y, btn_w, slot_h, "L") {
                    action = Some((*slot, "load"));
                }
                if header.is_some() && ui.button(slot_x + (btn_w + gap) * 2.0, y, btn_w, slot_h, "X") {
                    action = Some((*slot, "delete"));
                }

                let label_x = slot_x + (btn_w + gap) * 3.0;
                ui.text(label_x, y + 8.0, &label, 12.0, [0.9, 0.9, 0.85, 1.0]);
                y += slot_h + 4.0;
            }

            if let Some((slot, kind)) = action {
                match kind {
                    "save" => self.pending_save_slot = Some(slot),
                    "load" => self.load_slot(slot),
                    "delete" => self.delete_slot(slot),
                    _ => {}
                }
            }

            ui.text_centered(sw * 0.5, py + ph - 18.0,
                "S = Save · L = Load · X = Delete",
                11.0, [0.6, 0.65, 0.75, 0.8]);
        } else if !is_victory {
            let text = "Esc — pause · LMB — fire · RMB — aim · E — talk · 1/2/3/4 — weapon · R — reload · Tab — stats · I — inventory";
            ui.text_centered(ui.screen_w() * 0.5, ui.screen_h() - 24.0,
                text, 12.0, [1.0, 1.0, 1.0, 0.5]);
        }

        if self.show_character_sheet && !self.demo_paused && !is_victory {
            draw_character_sheet(ui, &mut self.player_stats);
        }

        if self.show_inventory && !self.demo_paused && !is_victory {
            let input = ui.input();
            if self.rmb_pressed_this_frame {
                let (mx, my) = input.mouse_pos;
                let sw = ui.screen_w();
                let sh = ui.screen_h();
                if let Some(loc) = self.hit_test_inventory(sw, sh, mx, my) {
                    match loc {
                        InvLocation::Inventory(idx) => {
                            if let Some(stack) = self.inventory.slots.get(idx).and_then(|s| s.clone()) {
                                if let Some(item) = self.item_registry.get_by_id(&stack.item_id) {
                                    match item.kind {
                                        ItemKind::Consumable => self.use_from_slot(idx),
                                        _ if item.equippable => self.equip_from_slot(idx),
                                        _ => {}
                                    }
                                }
                            }
                        }
                        InvLocation::Equipment(slot) => {
                            if let Some(old_id) = self.equipment.take_slot(slot) {
                                let leftover = self.inventory.add(
                                    &self.item_registry,
                                    ItemStack::new(old_id, 1),
                                );
                                if leftover > 0 {
                                    self.hud.push("Inventory full!");
                                } else {
                                    self.hud.push("Unequipped");
                                }
                            }
                        }
                    }
                }
            }

            let action = inventory_ui::draw(
                ui,
                &self.inventory,
                &self.equipment,
                &self.item_registry,
                &mut self.inv_drag,
            );
            match action {
                InventoryAction::None => {}
                InventoryAction::StartDrag(_) => {}
                InventoryAction::Swap(a, b) => self.handle_inv_swap(a, b),
                InventoryAction::Split(idx) => {
                    if self.inventory.split_stack(idx).is_some() {
                        self.hud.push("Stack split");
                    }
                }
                InventoryAction::Use(idx) => self.use_from_slot(idx),
                InventoryAction::EquipFromInv(idx) => self.equip_from_slot(idx),
                InventoryAction::Unequip(slot) => {
                    if let Some(old_id) = self.equipment.take_slot(slot) {
                        let _ = self.inventory.add(
                            &self.item_registry,
                            ItemStack::new(old_id, 1),
                        );
                        self.hud.push("Unequipped");
                    }
                }
                InventoryAction::Drop(idx) => {
                    if self.inventory.take_slot(idx).is_some() {
                        self.hud.push("Item dropped");
                    }
                }
                InventoryAction::SortBy(key) => {
                    self.inventory.sort(&self.item_registry, key);
                    self.hud.push(format!("Sorted: {:?}", key));
                }
            }
        }

        if let Some(session) = self.vendor_session.as_ref() {
            if let Some(def) = self.vendor_registry.get(&session.vendor_id).cloned() {
                let action = vendor_ui::draw(
                    ui,
                    session,
                    &def,
                    &self.item_registry,
                    &self.inventory,
                    &self.equipment,
                    self.campaign.gold,
                );

                match action {
                    VendorAction::None => {}
                    VendorAction::Close => {
                        self.vendor_session = None;
                    }
                    VendorAction::Buy(idx) => {
                        self.handle_vendor_buy(&def, idx);
                    }
                    VendorAction::Sell(idx) => {
                        self.handle_vendor_sell(idx);
                    }
                }
            }
        } else {
            self.vendor_esc_pressed = false;
        }

        if let Some(mut session) = self.crafting_session.take() {
            let action = crafting_ui::draw(
                ui,
                &mut session,
                &self.crafting_registry,
                &self.item_registry,
                &self.inventory,
                self.campaign.gold,
                self.player_stats.experience.level,
            );
            let mut keep = true;
            match action {
                CraftingAction::None => {}
                CraftingAction::Select(id) => {
                    session.selected_recipe_id = Some(id);
                }
                CraftingAction::Craft(id) => {
                    if let Some(recipe) = self.crafting_registry.get(&id).cloned() {
                        self.handle_craft(&recipe);
                    }
                }
                CraftingAction::Close => {
                    keep = false;
                }
            }
            if keep {
                self.crafting_session = Some(session);
            }
        }

        let dialog_click: Option<usize> = if let Some(session_ref) = self.dialog_session.as_ref() {
            let ctx = crate::game::dialogs::DialogContext {
                quest_manager: &self.quest_manager,
                inventory: &self.inventory,
                gold: self.campaign.gold,
                player_level: self.player_stats.experience.level,
                has_key_red: self.campaign.has_key_red,
            };
            crate::ui::DialogUi::draw(ui, session_ref, &ctx)
        } else {
            None
        };

        if self.dialog_session.is_some() {
            match dialog_click {
                Some(crate::ui::dialog::CLOSE_SENTINEL) => {
                    self.dialog_session = None;
                }
                Some(idx) => {
                    let (effects, ended) = if let Some(sess) = self.dialog_session.as_mut() {
                        sess.choose(idx)
                    } else {
                        (Vec::new(), false)
                    };
                    if ended {
                        self.dialog_session = None;
                    }
                    for effect in effects {
                        match effect {
                            DialogEffect::StartQuest(qid) => {
                                let rewards = self.quest_manager.start_quest(&qid);
                                self.hud.push(format!("📜 Quest started: {}", qid));
                                for reward in rewards {
                                    match reward {
                                        Reward::Xp(amt) => {
                                            let levels = self.player_stats.gain_xp(amt);
                                            self.hud.push(format!("+{} XP", amt));
                                            if levels > 0 {
                                                self.hud.push(format!(
                                                    "⭐ LEVEL UP → Lv {}",
                                                    self.player_stats.experience.level
                                                ));
                                            }
                                        }
                                        Reward::Gold(amt) => {
                                            self.campaign.gold += amt;
                                            self.hud.push(format!("+{} Gold", amt));
                                        }
                                        Reward::Item { id, count } => {
                                            self.try_pickup(&id, count);
                                        }
                                    }
                                }
                            }
                            DialogEffect::CompleteQuest(qid) => {
                                let rewards = self.quest_manager.complete_quest(&qid);
                                if !rewards.is_empty() {
                                    self.hud.push(format!("⭐ Quest completed: {}", qid));
                                    for reward in rewards {
                                        match reward {
                                            Reward::Xp(amt) => {
                                                let levels = self.player_stats.gain_xp(amt);
                                                self.hud.push(format!("+{} XP", amt));
                                                if levels > 0 {
                                                    self.hud.push(format!(
                                                        "⭐ LEVEL UP → Lv {}",
                                                        self.player_stats.experience.level
                                                    ));
                                                }
                                            }
                                            Reward::Gold(amt) => {
                                                self.campaign.gold += amt;
                                                self.hud.push(format!("+{} Gold", amt));
                                            }
                                            Reward::Item { id, count } => {
                                                self.try_pickup(&id, count);
                                            }
                                        }
                                    }
                                }
                            }
                            DialogEffect::GiveItem { id, count } => {
                                self.try_pickup(&id, count);
                            }
                            DialogEffect::GiveGold(amt) => {
                                self.campaign.gold += amt;
                                self.hud.push(format!("+{} Gold", amt));
                            }
                            DialogEffect::GiveXp(amt) => {
                                let levels = self.player_stats.gain_xp(amt);
                                self.hud.push(format!("+{} XP", amt));
                                if levels > 0 {
                                    self.hud.push(format!(
                                        "⭐ LEVEL UP → Lv {}",
                                        self.player_stats.experience.level
                                    ));
                                }
                            }
                            DialogEffect::OpenVendor(vendor_id) => {
                                if let Some(def) = self.vendor_registry.get(&vendor_id).cloned() {
                                    let npc_entity = self
                                        .dialog_session
                                        .as_ref()
                                        .map(|s| s.npc_entity)
                                        .unwrap_or(crate::ecs::Entity::INVALID);
                                    self.vendor_session =
                                        Some(VendorSession::new(&def, npc_entity));
                                    self.dialog_session = None;
                                    self.hud.push(format!("💰 Trading with {}", def.title));
                                } else {
                                    log::warn!("OpenVendor: '{}' not found", vendor_id);
                                }
                            }
                            DialogEffect::OpenCrafting(bench_id) => {
                                if self.crafting_registry.is_empty() {
                                    self.hud.push("No recipes loaded");
                                } else {
                                    let npc_entity = self
                                        .dialog_session
                                        .as_ref()
                                        .map(|s| s.npc_entity)
                                        .unwrap_or(crate::ecs::Entity::INVALID);
                                    self.crafting_session = Some(CraftingSession::new(
                                        &bench_id, npc_entity, &self.crafting_registry,
                                    ));
                                    self.dialog_session = None;
                                    self.hud.push("🔨 Forge opened");
                                }
                            }
                        }
                    }
                }
                None => {}
            }
        } else if self.last_play_mode && !self.demo_paused {
            let cam_pos = self.camera.position();
            let cam_dir = self.camera.forward();
            if crate::game::dialogs::find_npc_in_front(
                world, cam_pos, cam_dir, 3.0, 0.85,
            ).is_some() {
                let sw = ui.screen_w();
                let sh = ui.screen_h();
                ui.text_centered(
                    sw * 0.5, sh * 0.5 + 50.0,
                    "[E]  Talk", 18.0, [1.0, 0.9, 0.5, 1.0],
                );
            }
        }
    }
}

// ============================================================
// Helpers — RNG
// ============================================================

fn rand01_side() -> f32 {
    use std::cell::Cell;
    thread_local! {
        static S: Cell<u32> = const { Cell::new(0xC0FFEE) };
    }
    S.with(|s| {
        let mut v = s.get();
        v = v.wrapping_mul(1664525).wrapping_add(1013904223);
        s.set(v);
        ((v >> 8) & 0xFFFFFF) as f32 / 16777215.0
    })
}

fn rand01_crit() -> f32 {
    use std::cell::Cell;
    thread_local! {
        static S: Cell<u32> = const { Cell::new(0xBEEF_CAFE) };
    }
    S.with(|s| {
        let mut v = s.get();
        v = v.wrapping_mul(1664525).wrapping_add(1013904223);
        s.set(v);
        ((v >> 8) & 0xFFFFFF) as f32 / 16777215.0
    })
}

// ============================================================
// Character Sheet UI
// ============================================================

fn draw_character_sheet(ui: &mut crate::ui::UiLayer, stats: &mut PlayerStats) {
    let sw = ui.screen_w();
    let sh = ui.screen_h();
    let pw = 460.0;
    let ph = 360.0;
    let px = sw * 0.5 - pw * 0.5;
    let py = sh * 0.5 - ph * 0.5;

    ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.5]);
    ui.rect(px - 2.0, py - 2.0, pw + 4.0, ph + 4.0, [0.55, 0.6, 0.75, 1.0]);
    ui.rect(px, py, pw, ph, [0.13, 0.15, 0.20, 0.98]);

    ui.text_centered(sw * 0.5, py + 22.0, "CHARACTER", 24.0, [1.0, 0.9, 0.6, 1.0]);
    let lvl = format!(
        "Level {} · {} total XP · {} unspent",
        stats.experience.level, stats.experience.total_xp, stats.unspent_points,
    );
    ui.text_centered(sw * 0.5, py + 50.0, &lvl, 12.0, [0.75, 0.8, 0.9, 1.0]);

    ui.rect(px + 16.0, py + 68.0, pw - 32.0, 1.0, [0.4, 0.45, 0.55, 0.8]);

    let mut y = py + 82.0;
    let line_h = 46.0;
    let mut action: Option<usize> = None;

    for i in 0..5 {
        let value = stats.attributes.get(i);
        ui.text(px + 20.0, y + 6.0, STAT_NAMES[i], 16.0, [1.0, 1.0, 1.0, 1.0]);
        let val_text = format!("{:>3}", value);
        ui.text(px + 200.0, y + 6.0, &val_text, 16.0, [1.0, 0.95, 0.6, 1.0]);
        ui.text(px + 20.0, y + 26.0, STAT_DESCRIPTIONS[i], 10.0, [0.6, 0.65, 0.75, 0.9]);
        if stats.unspent_points > 0
            && ui.button(px + pw - 60.0, y + 6.0, 40.0, 26.0, "+")
        {
            action = Some(i);
        }
        y += line_h;
    }

    if let Some(idx) = action {
        stats.spend_point(idx);
    }

    ui.text_centered(sw * 0.5, py + ph - 22.0,
        "Tab — close · Esc — pause", 11.0, [0.6, 0.65, 0.75, 0.8]);
}

// ============================================================
// Materials
// ============================================================

fn add_materials(renderer: &mut Renderer) {
    renderer.add_material("arena_floor",
        Material::new([0.42, 0.46, 0.50, 1.0]).with_metallic_roughness(0.0, 0.92));
    renderer.add_material("arena_wall",
        Material::new([0.38, 0.40, 0.44, 1.0]).with_metallic_roughness(0.0, 0.88));
    renderer.add_material("arena_platform",
        Material::new([0.55, 0.58, 0.62, 1.0]).with_metallic_roughness(0.0, 0.8));
    renderer.add_material("arena_column",
        Material::new([0.68, 0.65, 0.58, 1.0]).with_metallic_roughness(0.1, 0.7));
    renderer.add_material("arena_crate",
        Material::new([0.65, 0.45, 0.25, 1.0]).with_metallic_roughness(0.0, 0.85));
    renderer.add_material("arena_barrel",
        Material::new([0.55, 0.30, 0.15, 1.0]).with_metallic_roughness(0.3, 0.6));
    renderer.add_material("arena_door",
        Material::new([0.45, 0.30, 0.20, 1.0]).with_metallic_roughness(0.1, 0.7));
    renderer.add_material("arena_marker",
        Material::new([1.0, 0.9, 0.4, 1.0]).with_metallic_roughness(0.0, 0.5)
            .with_emissive([2.5, 2.2, 0.6]));
    renderer.add_material("emissive",
        Material::new([1.0, 1.0, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5)
            .with_emissive([2.5, 2.2, 0.6]));
    renderer.add_material("emissive_warm",
        Material::new([1.0, 0.7, 0.4, 1.0]).with_metallic_roughness(0.0, 0.5)
            .with_emissive([3.0, 1.5, 0.4]));
    renderer.add_material("emissive_cold",
        Material::new([0.5, 0.75, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5)
            .with_emissive([0.8, 1.5, 3.0]));
    renderer.add_material("flat_red",
        Material::new([1.0, 0.35, 0.35, 1.0]).with_metallic_roughness(0.0, 0.5));
    renderer.add_material("flat_blue",
        Material::new([0.35, 0.55, 1.0, 1.0]).with_metallic_roughness(0.0, 0.5));
    renderer.add_material("gold",
        Material::new([1.0, 0.85, 0.3, 1.0]).with_metallic_roughness(1.0, 0.25));
    renderer.add_material("glass",
        Material::new([0.7, 0.85, 1.0, 0.35]).with_metallic_roughness(0.2, 0.05)
            .with_alpha_mode(AlphaMode::Blend));
    renderer.add_material("rpg_dungeon",
        Material::new([0.13, 0.11, 0.16, 1.0]).with_metallic_roughness(0.0, 0.95));
    renderer.add_material("foliage",
        Material::new([0.20, 0.45, 0.15, 1.0]).with_metallic_roughness(0.0, 0.95));
    renderer.add_material("bark",
        Material::new([0.30, 0.20, 0.12, 1.0]).with_metallic_roughness(0.0, 0.9));
    renderer.add_material("ground",
        Material::new([0.35, 0.38, 0.30, 1.0]).with_metallic_roughness(0.0, 0.95));
}