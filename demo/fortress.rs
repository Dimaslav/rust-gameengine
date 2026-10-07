//! "The Fallen Citadel" — сюжетная кампания из 5 актов.
//!
//! Демонстрирует: физику, AI, триггеры, диалоги, стадии, победу,
//! аптечки, ключи, ворота, save/load, перезарядку оружия.
//!
//! ## История фиксов
//!
//! * bugfix #2  — ворота останавливаются на `open_y` через `GateMotion`.
//! * bugfix #5  — события анимаций обрабатываются (`on_animation_event`).
//! * bugfix #6  — HUD тикается реальным `dt` (`last_dt`).
//! * bugfix #7  — AI / Audio / Timer сохраняются в сцену.
//! * bugfix #8  — `demo_ammo_reserve` восстанавливается без эвристики.
//! * bugfix #10 — Play стартует из сохранённой позиции, но с fallback
//!                на Act 1 при дефолтном `saved_position`.
//! * bugfix #R  — нет второго `Escape`-toggle, пауза синхронизируется
//!                с engine через `Game::on_pause_changed`. R даёт
//!                обратную связь во всех edge-случаях.
//! * bugfix hud-dup — стрельба на ЛКМ (не Space), авто-огонь через
//!                `mouse_down`, `ShotFired`-событие для эффектов.
//! * bugfix bridge — визуальные мосты между актами, счётчик убийств
//!                скрыт, когда акт не требует убийств.

use std::any::Any;
use std::collections::{HashMap, HashSet};

use glam::{Quat, Vec3};
use winit::event::MouseButton;
use winit::keyboard::KeyCode;

use crate::ecs::{Entity, System, World};
use crate::engine::{Game, Input, InputMap, Key, WorldLoadRequest};
use crate::game::ai::{AiAgent, AiState, AiTarget, DebugPath, Enemy};
use crate::game::animation::{
    AnimationEventTriggered, AnimationEvents, AnimationRuntime,
};
use crate::game::audio::AudioSource;
use crate::game::components::*;
use crate::game::decals::Decal;
use crate::game::lights::{DirectionalLight, PointLight};
use crate::game::timers::{Timer, TimerFinished, TimerSystem};
use crate::game::AudioBus;
use crate::physics::Collider;
use crate::render::{
    skinning::AnimationClip, AlphaMode, Camera3D, DebugView, GpuLight, GpuPointLight,
    InstanceData, LineBatch, LineVertex, Material, Mesh, MeshDraw, PostFx, Renderer, Skeleton,
};
use crate::ui::Hud;

use super::primitives::*;
use super::primitives::{stop_gate_if_reached, GateMotion};

// ============================================================
// Layout
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
// Systems
// ============================================================

struct RotationSystem;
impl System for RotationSystem {
    fn update(&mut self, world: &mut World, dt: f32) {
        let entities: Vec<_> = world.query::<Spinner>().map(|(e, _)| e).collect();
        for e in entities {
            let spinner = world.get::<Spinner>(e).copied();
            if let (Some(s), Some(t)) = (spinner, world.get_mut::<Transform>(e)) {
                let axis = s.axis.normalize_or_zero();
                if axis.length_squared() < 1e-6 { continue; }
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
    fn title(&self) -> &'static str {
        match self {
            Stage::Act1 => "ACT I — Пробуждение",
            Stage::Act2 => "ACT II — Двор",
            Stage::Act3 => "ACT III — Лобби",
            Stage::Act4 => "ACT IV — Подземелье",
            Stage::Act5 => "ACT V — Побег",
            Stage::Victory => "ПОБЕДА",
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
            objective: "Убей 2 стражников".to_string(),
            processed: HashSet::new(),
            dialog_shown: HashSet::new(),
            checkpoints: HashSet::new(),
            dialog: Some(("Добро пожаловать в Павшую Цитадель.".to_string(), 4.0)),
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
// Game
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

    // Player state
    demo_health: f32,
    demo_health_max: f32,
    demo_ammo: u32,
    demo_ammo_max: u32,
    demo_ammo_reserve: u32,
    demo_ammo_reserve_max: u32,
    demo_reloading: bool,
    demo_reload_timer: f32,
    demo_reload_duration: f32,
    demo_fire_cooldown: f32,
    demo_paused: bool,
    demo_show_debug: bool,
    last_target_hp: f32,

    hud: Hud,

    campaign: CampaignState,

    // Save/Load
    save_manager: crate::scene::SaveManager,
    pending_world_request: Option<WorldLoadRequest>,
    show_save_panel: bool,
    save_slots_cache: Vec<(u32, Option<crate::scene::SaveHeader>)>,
    save_slots_cache_dirty: bool,
    pending_autosave: bool,
    pending_save_slot: Option<u32>,

    prev_world_matrices: HashMap<Entity, glam::Mat4>,

    was_in_play_mode: bool,

    /// Реальный dt последнего кадра — для HUD-уведомлений (bugfix #6).
    last_dt: f32,
}

impl FortressDemo {
    pub fn new() -> Self {
        Self {
            camera: Camera3D::new(16.0 / 9.0),
            systems: vec![
                Box::new(TimerSystem),
                Box::new(RotationSystem),
                Box::new(MovementSystem),
            ],
            postfx: PostFx {
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
                fog_density: 0.008,
                fog_height_base: 0.0,
                fog_height_falloff: 0.06,
                vignette_strength: 0.15,
                film_grain: 0.03,
                chromatic_aberration: 0.0,
                shadow_bias: 0.0015,
                shadow_normal_bias: 3.0,
                shadow_fade_start: 150.0,
                shadow_fade_end: 200.0,
                lod_bias: 1.0,
                lod_distances: [30.0, 80.0, 200.0, 500.0],
                taa_strength: 1.0,
                taa_sharpening: 0.1,
                volumetric_density: 0.006,
                volumetric_scattering: 0.45,
                volumetric_phase_g: 0.6,
            },
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
            demo_ammo: 30,
            demo_ammo_max: 30,
            demo_ammo_reserve: 90,
            demo_ammo_reserve_max: 120,
            demo_reloading: false,
            demo_reload_timer: 0.0,
            demo_reload_duration: 1.5,
            demo_fire_cooldown: 0.0,
            demo_paused: false,
            demo_show_debug: true,
            last_target_hp: 100.0,

            hud: Hud::new(),
            campaign: CampaignState::new(),

            save_manager: crate::scene::SaveManager::new("saves"),
            pending_world_request: None,
            show_save_panel: false,
            save_slots_cache: Vec::new(),
            save_slots_cache_dirty: true,
            pending_autosave: false,
            pending_save_slot: None,

            prev_world_matrices: HashMap::new(),
            was_in_play_mode: false,

            last_dt: 1.0 / 60.0,
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
                        log::info!("Saved to slot {}", slot);
                    }
                    Err(e) => {
                        self.hud.push(format!("Save failed: {}", e));
                        log::error!("Save slot {} failed: {}", slot, e);
                    }
                }
            }
            Err(e) => {
                self.hud.push(format!("Snapshot failed: {}", e));
                log::error!("Snapshot for save slot {} failed: {}", slot, e);
            }
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
                log::info!("Requested load of slot {}", slot);
            }
            Err(e) => {
                self.hud.push(format!("Load failed: {}", e));
                log::error!("Load slot {} failed: {}", slot, e);
            }
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
}

// ============================================================
// Building
// ============================================================

impl FortressDemo {
    fn build(&mut self, world: &mut World) {
        self.build_act1(world);
        self.build_act2(world);
        self.build_act3(world);
        self.build_act4(world);
        self.build_act5(world);
        // ИЗМЕНЕНО (bugfix bridge): добавляем визуальные мосты
        // между актами. Раньше между краями полов были дыры
        // (Act1 заканчивается на X=-50, Act2 начинается на X=-30;
        // Act2 заканчивается на X=30, Act3 начинается на X=40).
        self.build_bridges(world);
        self.build_global_lighting(world);
        self.build_ai_target(world);
        self.bell_entity = Some(self.build_bell(world));

        log::info!(
            "Campaign built: 5 acts, {} entities total",
            world.len()
        );

        self.navmesh_bake_requested = true;
    }

    /// Мосты между актами. Устраняют «дыры», через которые игрок
    /// раньше шёл по невидимой плоскости Y=0.
    fn build_bridges(&mut self, world: &mut World) {
        // --- Act1 → Act2 ---
        // Act1 floor: X ∈ [-70, -50]. Act2 floor: X ∈ [-30, 30].
        // Разрыв: X ∈ [-50, -30], длина 20.
        static_box(world, "Bridge_Act1_Act2", "arena_floor",
            Vec3::new(-40.0, 0.0, 0.0),
            Vec3::new(20.0, 0.5, 8.0),
            Vec3::splat(0.5), 2.0);

        wall(world, "Bridge_A1A2_Rail_N",
            Vec3::new(-50.0, 0.0, -4.0),
            Vec3::new(-30.0, 0.0, -4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        wall(world, "Bridge_A1A2_Rail_S",
            Vec3::new(-50.0, 0.0, 4.0),
            Vec3::new(-30.0, 0.0, 4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        torch(world, "Bridge_A1A2_Torch1", Vec3::new(-45.0, 3.0, 0.0));
        torch(world, "Bridge_A1A2_Torch2", Vec3::new(-35.0, 3.0, 0.0));

        // --- Act2 → Act3 ---
        // Act2 floor: X ∈ [-30, 30]. Act3 floor: X ∈ [40, 70].
        // Разрыв: X ∈ [30, 40], длина 10.
        static_box(world, "Bridge_Act2_Act3", "arena_floor",
            Vec3::new(35.0, 0.0, 0.0),
            Vec3::new(10.0, 0.5, 8.0),
            Vec3::splat(0.5), 2.0);

        wall(world, "Bridge_A2A3_Rail_N",
            Vec3::new(30.0, 0.0, -4.0),
            Vec3::new(40.0, 0.0, -4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        wall(world, "Bridge_A2A3_Rail_S",
            Vec3::new(30.0, 0.0, 4.0),
            Vec3::new(40.0, 0.0, 4.0),
            0.0, 1.0, 0.3, "arena_wall", 2.0);
        torch(world, "Bridge_A2A3_Torch", Vec3::new(35.0, 3.0, 0.0));
    }

    fn build_act1(&mut self, world: &mut World) {
        let c = ACT1_CENTER;
        let h = ACT1_HALF;

        static_box(world, "Act1_Floor", "arena_floor",
            c, Vec3::new(h * 2.0, 0.5, h * 2.0),
            Vec3::splat(0.5), 2.5);

        wall(world, "Act1_Wall_W",
            c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_N",
            c + Vec3::new(-h, 0.0, -h), c + Vec3::new(h, 0.0, -h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_S",
            c + Vec3::new(-h, 0.0, h), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_E1",
            c + Vec3::new(h, 0.0, -h), c + Vec3::new(h, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act1_Wall_E2",
            c + Vec3::new(h, 0.0, 3.0), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        let gate_pos = c + Vec3::new(h, 2.0, 0.0);
        let gate = locked_gate(world, "gate_east_1", gate_pos,
            Vec3::new(0.5, 4.0, 6.0), "arena_door");
        self.gate_1 = Some(gate);

        torch(world, "Act1_Torch_1", c + Vec3::new(-h + 1.0, 3.0, -h + 1.0));
        torch(world, "Act1_Torch_2", c + Vec3::new(-h + 1.0, 3.0,  h - 1.0));

        let p1 = vec![
            c + Vec3::new(-6.0, 0.0, -6.0),
            c + Vec3::new( 6.0, 0.0, -6.0),
            c + Vec3::new( 6.0, 0.0,  6.0),
            c + Vec3::new(-6.0, 0.0,  6.0),
        ];
        enemy(world, "Act1_Enemy_1", c + Vec3::new(-5.0, 0.0, 0.0), EnemyKind::Patrol, Some(p1.clone()));
        enemy(world, "Act1_Enemy_2", c + Vec3::new( 5.0, 0.0, 0.0), EnemyKind::Patrol, Some(p1));

        checkpoint(world, "checkpoint_1", c + Vec3::new(-h + 3.0, 0.02, 0.0));
        health_pickup(world, "health_a1", c + Vec3::new(-7.0, 0.5, 7.0), 30.0);
        ammo_pickup(world, "ammo_a1", c + Vec3::new(-7.0, 0.5, -7.0), 20);
        objective_marker(world, "objective_gate1", gate_pos + Vec3::new(0.0, 3.0, 0.0));
        dialog_zone(world, "dialog_intro", c + Vec3::new(-h + 4.0, 1.0, 0.0), 3.0);

        blood_decal(world, c + Vec3::new(-3.0, 0.02, 2.0), 1.5, 0.7);
        blood_decal(world, c + Vec3::new( 4.0, 0.02, -4.0), 1.8, 0.8);
    }

    fn build_act2(&mut self, world: &mut World) {
        let c = ACT2_CENTER;
        let h = ACT2_HALF;

        static_box(world, "Act2_Floor", "arena_floor",
            c, Vec3::new(h * 2.0, 0.5, h * 2.0),
            Vec3::splat(0.5), 2.0);

        wall(world, "Act2_Wall_W1",
            c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-h, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_W2",
            c + Vec3::new(-h, 0.0, 3.0), c + Vec3::new(-h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_E1",
            c + Vec3::new(h, 0.0, -h), c + Vec3::new(h, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_E2",
            c + Vec3::new(h, 0.0, 3.0), c + Vec3::new(h, 0.0, h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_N",
            c + Vec3::new(-h, 0.0, -h), c + Vec3::new(h, 0.0, -h),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Wall_S",
            c + Vec3::new(-h, 0.0, h), c + Vec3::new(h, 0.0, h),
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
                *center + Vec3::Y * 3.0,
                Vec3::new(6.0, 6.0, 6.0),
                Vec3::splat(0.5), 2.0);
            static_box(world, format!("Act2_Tower_{}_top", side), "arena_wall",
                *center + Vec3::Y * 6.4,
                Vec3::new(6.6, 0.4, 6.6),
                Vec3::splat(0.5), 1.0);
            torch(world, format!("Act2_Torch_{}", side), *center + Vec3::Y * 7.2);
        }

        let col_r = 12.0;
        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            let x = a.cos() * col_r;
            let z = a.sin() * col_r;
            static_mesh(world, format!("Act2_Col_{}", i), "cylinder", "arena_column",
                c + Vec3::new(x, 2.5, z), Quat::IDENTITY,
                Vec3::new(0.9, 5.0, 0.9),
                Some(Collider::aabb(Vec3::splat(0.5))));
        }

        for i in 0..3u32 {
            let r = 5.0 - i as f32 * 1.3;
            let y = i as f32 * 0.5 + 0.25;
            static_mesh(world, format!("Act2_Altar_Step_{}", i), "cylinder", "arena_platform",
                c + Vec3::Y * y, Quat::IDENTITY,
                Vec3::new(r * 2.0, 0.5, r * 2.0),
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
        ammo_pickup(world, "ammo_a2_1", c + Vec3::new(20.0, 0.5, -20.0), 30);
        ammo_pickup(world, "ammo_a2_2", c + Vec3::new(-24.0, 0.5, 0.0), 30);

        dialog_zone(world, "dialog_act2", c + Vec3::new(-h + 4.0, 1.0, 0.0), 3.0);

        for (x, z, s, a) in [
            (-12.0, -10.0, 1.8, 0.85),
            (10.0, -13.0, 2.0, 0.8),
            (15.0, 10.0, 1.6, 0.75),
            (-8.0, 15.0, 1.9, 0.85),
            (5.0, 5.0, 1.3, 0.7),
        ] {
            blood_decal(world, c + Vec3::new(x, 0.02, z), s, a);
        }
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
            center + Vec3::new(half, 0.0, -half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Treasure_S",
            center + Vec3::new(-half, 0.0, half),
            center + Vec3::new(half, 0.0, half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act2_Treasure_E",
            center + Vec3::new(half, 0.0, -half),
            center + Vec3::new(half, 0.0, half),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        enemy(world, "Act2_Treasure_Guard1",
            center + Vec3::new(2.0, 0.0, -2.0), EnemyKind::Elite, None);
        enemy(world, "Act2_Treasure_Guard2",
            center + Vec3::new(2.0, 0.0, 2.0), EnemyKind::Elite, None);

        chest(world, "Act2_Chest_A", center + Vec3::new(4.0, 0.25, -3.0), 200);
        chest(world, "Act2_Chest_B", center + Vec3::new(4.0, 0.25, 0.0), 350);

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
            c, Vec3::new(hx * 2.0, 0.5, hz * 2.0),
            Vec3::splat(0.5), 1.5);

        wall(world, "Act3_Wall_W1",
            c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(-hx, 0.0, -3.0),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_W2",
            c + Vec3::new(-hx, 0.0, 3.0), c + Vec3::new(-hx, 0.0, hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_E",
            c + Vec3::new(hx, 0.0, -hz), c + Vec3::new(hx, 0.0, hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_N",
            c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(hx, 0.0, -hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);
        wall(world, "Act3_Wall_S",
            c + Vec3::new(-hx, 0.0, hz), c + Vec3::new(hx, 0.0, hz),
            0.0, WALL_H, WALL_T, "arena_wall", 2.0);

        let portal_pos = c + Vec3::new(hx - 5.0, 1.5, 0.0);
        let act4_dest = ACT4_CENTER + Vec3::new(0.0, 1.0, 10.0);
        portal(world, "portal_to_dungeon", portal_pos, act4_dest, [0.55, 0.25, 0.9, 0.75]);
        objective_marker(world, "objective_portal", portal_pos + Vec3::Y * 2.0);

        enemy(world, "Act3_Guard", c + Vec3::new(-3.0, 0.0, 0.0), EnemyKind::Elite, None);

        health_pickup(world, "health_a3", c + Vec3::new(-8.0, 0.5, 5.0), 50.0);
        ammo_pickup(world, "ammo_a3_1", c + Vec3::new(-8.0, 0.5, -5.0), 40);
        ammo_pickup(world, "ammo_a3_2", c + Vec3::new(0.0, 0.5, 6.0), 40);

        checkpoint(world, "checkpoint_3", c + Vec3::new(-hx + 3.0, 0.02, 0.0));
        dialog_zone(world, "dialog_act3", c + Vec3::new(-hx + 4.0, 1.0, 0.0), 3.0);

        torch(world, "Act3_Torch_1", c + Vec3::new(-hx + 2.0, 3.5, -hz + 2.0));
        torch(world, "Act3_Torch_2", c + Vec3::new(-hx + 2.0, 3.5, hz - 2.0));
    }

    fn build_act4(&mut self, world: &mut World) {
        let c = ACT4_CENTER;
        let h = ACT4_HALF;

        static_box(world, "Act4_Floor", "rpg_dungeon",
            c, Vec3::new(h * 2.0, 0.5, h * 2.0),
            Vec3::splat(0.5), 2.0);

        static_box(world, "Act4_Ceiling", "arena_wall",
            c + Vec3::Y * 6.0, Vec3::new(h * 2.0, 0.5, h * 2.0),
            Vec3::splat(0.5), 2.0);

        wall(world, "Act4_Wall_W",
            c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-h, 0.0, h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_E",
            c + Vec3::new(h, 0.0, -h), c + Vec3::new(h, 0.0, h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_S",
            c + Vec3::new(-h, 0.0, h), c + Vec3::new(h, 0.0, h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_N1",
            c + Vec3::new(-h, 0.0, -h), c + Vec3::new(-3.0, 0.0, -h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);
        wall(world, "Act4_Wall_N2",
            c + Vec3::new(3.0, 0.0, -h), c + Vec3::new(h, 0.0, -h),
            0.0, 6.0, WALL_T, "rpg_dungeon", 2.0);

        let gate_boss_pos = c + Vec3::new(0.0, 2.5, -h);
        let gate_boss = locked_gate(world, "gate_boss", gate_boss_pos,
            Vec3::new(6.0, 5.0, 0.5), "rpg_dungeon");
        self.gate_boss = Some(gate_boss);

        crystal(world, "Act4_Crystal_A", c + Vec3::new(-10.0, 1.5, -5.0), [0.35, 0.6, 1.0]);
        crystal(world, "Act4_Crystal_B", c + Vec3::new( 10.0, 1.5, -5.0), [0.6, 0.35, 1.0]);
        crystal(world, "Act4_Crystal_C", c + Vec3::new( 0.0, 1.5,  10.0), [0.35, 1.0, 0.85]);

        enemy(world, "Act4_Wolf_1", c + Vec3::new(-10.0, 0.0, 5.0), EnemyKind::Patrol, None);
        enemy(world, "Act4_Wolf_2", c + Vec3::new( 10.0, 0.0, 5.0), EnemyKind::Patrol, None);
        enemy(world, "Act4_Wolf_3", c + Vec3::new( 0.0, 0.0, -10.0), EnemyKind::Patrol, None);

        chest(world, "Act4_Chest_A", c + Vec3::new(-14.0, 0.25, 14.0), 750);
        chest(world, "Act4_Chest_B", c + Vec3::new( 14.0, 0.25, 14.0), 1000);

        let back_dest = ACT3_CENTER + Vec3::new(0.0, 1.0, 0.0);
        portal(world, "portal_back_3",
            c + Vec3::new(-15.0, 1.5, 15.0), back_dest, [0.9, 0.5, 0.2, 0.75]);

        checkpoint(world, "checkpoint_4", c + Vec3::new(0.0, 0.02, 10.0));
        dialog_zone(world, "dialog_act4", c + Vec3::new(0.0, 1.0, 8.0), 4.0);

        for i in 0..6 {
            let a = i as f32 / 6.0 * std::f32::consts::TAU;
            rune_decal(
                world,
                c + Vec3::new(a.cos() * 8.0, 0.02, a.sin() * 8.0),
                1.2,
                [0.4, 0.3, 0.9, 0.6],
            );
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
            Vec3::new(room_cx, c.y, room_cz),
            EnemyKind::Boss, None);
        self.boss_entity = Some(boss);

        crystal(world, "Boss_Crystal_1",
            Vec3::new(room_cx - 5.0, c.y + 2.0, room_cz - 4.0), [1.0, 0.3, 0.3]);
        crystal(world, "Boss_Crystal_2",
            Vec3::new(room_cx + 5.0, c.y + 2.0, room_cz - 4.0), [1.0, 0.3, 0.3]);

        for i in 0..8 {
            let a = i as f32 / 8.0 * std::f32::consts::TAU;
            rune_decal(
                world,
                Vec3::new(room_cx + a.cos() * 6.0, c.y + 0.02, room_cz + a.sin() * 6.0),
                1.5,
                [1.0, 0.2, 0.2, 0.7],
            );
        }
    }

    fn build_act5(&mut self, world: &mut World) {
        let c = ACT5_CENTER;
        let hx = ACT5_HALF_X;
        let hz = ACT5_HALF_Z;

        static_box(world, "Act5_Floor", "arena_platform",
            c, Vec3::new(hx * 2.0, 0.5, hz * 2.0),
            Vec3::splat(0.5), 1.0);

        static_box(world, "Act5_Ceiling", "arena_wall",
            c + Vec3::Y * 6.0, Vec3::new(hx * 2.0, 0.5, hz * 2.0),
            Vec3::splat(0.5), 1.5);

        wall(world, "Act5_Wall_W",
            c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(-hx, 0.0, hz),
            0.0, 6.0, WALL_T, "arena_column", 2.0);
        wall(world, "Act5_Wall_E",
            c + Vec3::new(hx, 0.0, -hz), c + Vec3::new(hx, 0.0, hz),
            0.0, 6.0, WALL_T, "arena_column", 2.0);
        wall(world, "Act5_Wall_N",
            c + Vec3::new(-hx, 0.0, -hz), c + Vec3::new(hx, 0.0, -hz),
            0.0, 6.0, WALL_T, "arena_column", 2.0);

        let exit_pos = c + Vec3::new(0.0, 1.5, -hz + 3.0);
        exit_zone(world, "exit_zone", exit_pos, 2.5);

        for i in 0..4 {
            let x = if i % 2 == 0 { -hx + 2.0 } else { hx - 2.0 };
            let z = if i < 2 { -hz + 2.0 } else { hz - 2.0 };
            crystal(world, format!("Act5_Crystal_{}", i),
                c + Vec3::new(x, 2.0, z),
                [1.0, 0.8, 0.4]);
        }

        checkpoint(world, "checkpoint_5", c + Vec3::new(0.0, 0.02, hz - 3.0));
    }

    fn build_global_lighting(&mut self, world: &mut World) {
        let sun = world.spawn();
        world.insert(sun, Name("Sun".into()));
        world.insert(sun, Transform::at(Vec3::new(0.0, 40.0, 0.0)));
        let mut l = DirectionalLight::sun();
        l.direction = Vec3::new(0.45, 1.0, 0.35);
        l.intensity = 2.4;
        l.color = [1.0, 0.95, 0.85];
        world.insert(sun, l);

        let fill = world.spawn();
        world.insert(fill, Name("Fill".into()));
        world.insert(fill, Transform::at(Vec3::new(0.0, 10.0, 0.0)));
        let mut f = DirectionalLight::fill();
        f.intensity = 0.35;
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

    fn open_gate(&self, world: &mut World, gate: Option<Entity>) {
        let Some(g) = gate else { return };
        // Скорость берём из GateMotion, чтобы `stop_gate_if_reached`
        // знал, где останавливаться (bugfix #2).
        let speed = world.get::<GateMotion>(g).map(|m| m.speed).unwrap_or(4.0);
        if let Some(v) = world.get_mut::<Velocity>(g) {
            v.value = Vec3::new(0.0, speed, 0.0);
        }
        if let Some(src) = world.get_mut::<AudioSource>(g) {
            src.playing = true;
        }
        log::info!("Gate opened: entity #{}", g);
    }

    /// Останавливает ворота, доехавшие до `GateMotion::open_y`
    /// (bugfix #2). Раньше MovementSystem двигал их бесконечно.
    fn tick_gates(&mut self, world: &mut World) {
        for gate in [self.gate_1, self.gate_2, self.gate_4, self.gate_boss] {
            if let Some(g) = gate {
                if stop_gate_if_reached(world, g) {
                    log::info!("Gate #{} stopped at open_y", g);
                }
            }
        }
    }

    fn process_triggers(&mut self, world: &mut World) {
        let fired: Vec<(Entity, String)> = world
            .query::<Trigger>()
            .filter(|(e, t)| t.fired && !self.campaign.processed.contains(e))
            .filter_map(|(e, _)| {
                let name = world.get::<Name>(e)?.0.clone();
                Some((e, name))
            })
            .collect();

        for (e, name) in fired {
            self.campaign.processed.insert(e);
            self.on_trigger(world, e, &name);
        }
    }

    fn on_trigger(&mut self, world: &mut World, e: Entity, name: &str) {
        if name == "key_red" {
            self.campaign.has_key_red = true;
            self.hud.push("🔑 Красный ключ получен");
            self.campaign.show_dialog("Ключ от подземелья — в руках!", 3.5);
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
            self.demo_health = (self.demo_health + amount).min(self.demo_health_max);
            self.hud.push(format!("+{:.0} HP", amount));
            if let Some(t) = world.get_mut::<Transform>(e) {
                t.position.y = -500.0;
            }
            return;
        }

        if name.starts_with("ammo_") {
            let amount = world
                .get::<Tint>(e)
                .map(|t| (t.0[0] * 100.0).max(10.0) as u32)
                .unwrap_or(20);
            let before = self.demo_ammo_reserve;
            self.demo_ammo_reserve = (self.demo_ammo_reserve + amount)
                .min(self.demo_ammo_reserve_max);
            let added = self.demo_ammo_reserve - before;
            self.hud.push(format!("+{} reserve ammo", added));
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

        if name == "dialog_intro" {
            if self.campaign.dialog_shown.insert(name.to_string()) {
                self.campaign.show_dialog(
                    "Ты пробудился в руинах. Пробейся через стражу!",
                    4.0,
                );
            }
            return;
        }
        if name == "dialog_act2" {
            if self.campaign.dialog_shown.insert(name.to_string()) {
                self.campaign.show_dialog(
                    "Двор полон врагов. Найди ключ в сокровищнице.",
                    4.0,
                );
            }
            return;
        }
        if name == "dialog_act3" {
            if self.campaign.dialog_shown.insert(name.to_string()) {
                self.campaign.show_dialog(
                    "Впереди — портал. Возьми аптечку и патроны.",
                    4.0,
                );
            }
            return;
        }
        if name == "dialog_act4" {
            if self.campaign.dialog_shown.insert(name.to_string()) {
                self.campaign.show_dialog(
                    "Это подземелье Владыки. Готовься к бою.",
                    4.0,
                );
            }
            return;
        }

        if name == "exit_zone" {
            if self.campaign.stage != Stage::Victory {
                self.trigger_victory();
            }
            return;
        }

        log::debug!("Campaign trigger fired: {}", name);
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
        self.campaign.objective = "Победа!".to_string();
        self.hud.push("🏆 VICTORY!");
        log::info!(
            "Victory at t={:.1}s, kills={}",
            self.campaign.playtime,
            self.campaign.kills_total
        );
    }

    fn check_stage_transitions(&mut self, world: &mut World) {
        match self.campaign.stage {
            Stage::Act1 => {
                if self.campaign.kills_in_stage >= self.campaign.kills_required {
                    self.open_gate(world, self.gate_1);
                    self.hide_objective(world, "objective_gate1");
                    self.campaign.stage = Stage::Act2;
                    self.campaign.kills_in_stage = 0;
                    // В Act2 прогресс через ключ, а не через убийства.
                    self.campaign.kills_required = 0;
                    self.campaign.objective = "Найди красный ключ в сокровищнице".to_string();
                    self.campaign.show_dialog("Ворота открыты. Впереди — двор.", 4.0);
                }
            }
            Stage::Act2 => {
                if self.campaign.has_key_red {
                    self.open_gate(world, self.gate_2);
                    self.campaign.stage = Stage::Act3;
                    self.campaign.kills_in_stage = 0;
                    self.campaign.kills_required = 0;
                    self.campaign.objective = "Войди в портал лобби".to_string();
                    self.campaign.show_dialog("Ключ открыл путь в лобби.", 4.0);
                }
            }
            Stage::Act3 => {
                let p = self.camera.position();
                if p.y < -5.0 {
                    self.campaign.stage = Stage::Act4;
                    self.campaign.kills_in_stage = 0;
                    self.campaign.kills_required = 0;
                    self.campaign.objective = "Убей Владыку Цитадели".to_string();
                    self.hide_objective(world, "objective_portal");
                }
            }
            Stage::Act4 => {
                let boss_dead = self.boss_entity
                    .map(|b| !world.entities().contains(&b))
                    .unwrap_or(true);
                if boss_dead {
                    self.open_gate(world, self.gate_4);
                    self.campaign.stage = Stage::Act5;
                    self.campaign.objective = "Покинь цитадель".to_string();
                    self.campaign.show_dialog("Владыка пал. Свобода ждёт!", 5.0);
                }
            }
            Stage::Act5 | Stage::Victory => {}
        }
    }

    /// Обработчик анимационных событий (bugfix #5).
    ///
    /// Раньше результат `animation_runtime.advance_all` шёл в
    /// `let _ = ...`, и все маркеры (footstep, hit, spawn_vfx)
    /// молча терялись.
    fn on_animation_event(&mut self, world: &World, ev: AnimationEventTriggered) {
        let pos = crate::game::world_position(world, ev.entity)
            .unwrap_or(glam::Vec3::ZERO);
        match ev.name.as_str() {
            "footstep" => {
                log::debug!(
                    "[ANIM] footstep on #{}, clip '{}', at ({:.1},{:.1},{:.1})",
                    ev.entity, ev.clip, pos.x, pos.y, pos.z,
                );
            }
            "hit" => {
                log::debug!(
                    "[ANIM] hit on #{}, clip '{}', at ({:.1},{:.1},{:.1})",
                    ev.entity, ev.clip, pos.x, pos.y, pos.z,
                );
                self.hud.push("⚔ Hit!");
            }
            "spawn_vfx" => {
                log::debug!(
                    "[ANIM] spawn_vfx on #{}, payload={:?}, at ({:.1},{:.1},{:.1})",
                    ev.entity, ev.payload, pos.x, pos.y, pos.z,
                );
            }
            "open_door" => {
                self.hud.push("🚪 Door opens");
            }
            _ => {
                log::debug!(
                    "[ANIM] event '{}' on #{} (clip '{}')",
                    ev.name, ev.entity, ev.clip,
                );
            }
        }
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

        add_materials(renderer);

        if let Ok(loaded) = crate::render::load_gltf_into(
            renderer, "assets/animated.glb", "anim"
        ) {
            self.skeletons = loaded.skeletons;
            self.animations = loaded.animations;
        }

        if let Ok(ev) = AnimationEvents::from_file("assets/animated.anim_events.ron") {
            self.animation_events = ev;
        }
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
        // bugfix #6: сохраняем dt для HUD.
        self.last_dt = dt;

        // === Build (один раз) ===
        if !self.built {
            self.built = true;
            self.build(world);

            self.camera.target = ACT1_CENTER;
            self.camera.distance = 24.0;
            self.camera.yaw = -0.6;
            self.camera.pitch = 0.75;
        }

        // === Play mode enter ===
        if input.play_mode && !self.was_in_play_mode {
            // bugfix #10: PlayState::default() даёт saved_position =
            // (0.0, 1.7, 45.0) — пустая точка в стороне от сцены.
            // Если позиция выглядит как дефолтная, переопределяем на
            // ACT1_CENTER. Если игрок явно сохранил позицию — оставляем.
            let p = self.camera.first_person_pos;
            let looks_default = p.x.abs() < 1.0 && (p.z - 45.0).abs() < 1.0;
            if looks_default {
                self.camera.first_person_pos =
                    ACT1_CENTER + Vec3::new(0.0, 1.7, 0.0);
                log::info!("Play: default spawn detected, moved to Act 1");
            }
            self.camera.yaw = std::f32::consts::PI;
            self.camera.pitch = 0.0;
            log::info!(
                "Play entered at ({:.1}, {:.1}, {:.1})",
                self.camera.first_person_pos.x,
                self.camera.first_person_pos.y,
                self.camera.first_person_pos.z,
            );
        }
        self.was_in_play_mode = input.play_mode;

        // === Autosave / Manual save ===
        if self.pending_autosave {
            self.pending_autosave = false;
            self.save_slot(0, world, renderer);
        }
        if let Some(slot) = self.pending_save_slot.take() {
            self.save_slot(slot, world, renderer);
        }

        // === Misc input ===
        if input.pressed("reload_shaders") {
            if let Err(e) = renderer.reload_shaders() {
                log::error!("Shader reload: {}", e);
            }
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
        // bugfix #R: убран toggle demo_paused по Escape. Engine сам
        // обрабатывает Escape и вызывает Game::on_pause_changed.

        self.demo_fire_cooldown = (self.demo_fire_cooldown - dt).max(0.0);

        // ============================================================
        // Ammo & reload
        // ============================================================

        // (1) AUTO-START.
        if !self.demo_reloading
            && self.demo_ammo == 0
            && self.demo_ammo_reserve > 0
            && !self.demo_paused
            && self.campaign.stage != Stage::Victory
        {
            self.demo_reloading = true;
            self.demo_reload_timer = self.demo_reload_duration;
            self.hud.push("Reloading…");
            log::info!(
                "[RELOAD] auto-start: mag 0/{}, reserve {}",
                self.demo_ammo_max, self.demo_ammo_reserve
            );
        }

        // (2) TICK.
        if self.demo_reloading {
            self.demo_reload_timer -= dt;
            if self.demo_reload_timer <= 0.0 {
                self.demo_reloading = false;
                self.demo_reload_timer = 0.0;

                let need = self.demo_ammo_max.saturating_sub(self.demo_ammo);
                let take = need.min(self.demo_ammo_reserve);
                self.demo_ammo += take;
                self.demo_ammo_reserve -= take;

                if take > 0 {
                    self.hud.push(format!(
                        "Reloaded {}/{}",
                        self.demo_ammo, self.demo_ammo_max
                    ));
                    log::info!(
                        "[RELOAD] done: mag {}/{}, reserve {}",
                        self.demo_ammo, self.demo_ammo_max, self.demo_ammo_reserve
                    );
                } else {
                    self.hud.push("Out of ammo");
                    log::info!("[RELOAD] done: no reserve left");
                }
            }
        }

        // (3) MANUAL R.
        //
        // bugfix #R: обратная связь во всех edge-случаях.
        if !self.demo_paused
            && input.key_pressed(KeyCode::KeyR)
            && !self.demo_reloading
            && self.campaign.stage != Stage::Victory
        {
            if self.demo_ammo >= self.demo_ammo_max {
                self.hud.push("Magazine full");
                log::info!(
                    "[RELOAD] skipped: mag already full ({}/{})",
                    self.demo_ammo, self.demo_ammo_max
                );
            } else if self.demo_ammo_reserve == 0 {
                self.hud.push("No reserve ammo");
                log::info!("[RELOAD] skipped: no reserve ammo");
            } else {
                self.demo_reloading = true;
                self.demo_reload_timer = self.demo_reload_duration;
                self.hud.push("Reloading…");
                log::info!(
                    "[RELOAD] manual R: mag {}/{}, reserve {}",
                    self.demo_ammo, self.demo_ammo_max, self.demo_ammo_reserve
                );
            }
        }

        // (4) FIRE.
        //
        // bugfix hud-dup: триггер — ЛКМ, авто-огонь через mouse_down.
        // Урон/эффекты — через ShotFired-событие, потому что у
        // FortressDemo нет доступа к AudioSystem и particle-системе App.
        let lmb = input.mouse_down(MouseButton::Left);
        if !self.demo_paused
            && lmb
            && !self.demo_reloading
            && self.demo_fire_cooldown <= 0.0
            && self.demo_ammo > 0
            && self.campaign.stage != Stage::Victory
        {
            self.demo_ammo -= 1;
            self.demo_fire_cooldown = 0.15;
            log::debug!(
                "[FIRE] mag {}/{}, reserve {}",
                self.demo_ammo, self.demo_ammo_max, self.demo_ammo_reserve
            );

            let origin = self.camera.position();
            let dir = self.camera.forward();
            world.send(crate::engine::ShotFired {
                origin,
                direction: dir,
            });
        }

        // === Camera (editor) ===
        if !input.play_mode && !input.editor_flying && !self.demo_paused {
            let lmb = input.mouse_down(winit::event::MouseButton::Left) && !input.editor_captured;
            if lmb {
                let (dx, dy) = input.mouse_delta;
                if self.dragging { self.camera.orbit(dx * 0.005, dy * 0.005); }
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
            if pan != (0.0, 0.0) { self.camera.pan(pan.0, pan.1); }
        }

        // === Campaign ===
        if !self.demo_paused && self.campaign.stage != Stage::Victory {
            self.campaign.playtime += dt;
        }
        self.campaign.tick_dialog(dt);

        // === AI target ===
        if input.play_mode {
            if let Some(t) = self.ai_target_entity {
                let p = self.camera.position();
                if let Some(tr) = world.get_mut::<Transform>(t) {
                    tr.position = Vec3::new(p.x, p.y - 0.8, p.z);
                }
                world.insert(t, Visible(false));
            }
        } else {
            if let Some(t) = self.ai_target_entity {
                world.insert(t, Visible(true));
            }
        }

        // === Damage accumulator ===
        if let Some(t) = self.ai_target_entity {
            if let Some(h) = world.get_mut::<Health>(t) {
                let damage = (self.last_target_hp - h.current).max(0.0);
                if damage > 0.001 {
                    self.demo_health = (self.demo_health - damage).max(0.0);
                    self.hud.push(format!("-{:.0} HP", damage));
                }
                h.current = self.demo_health_max;
                self.last_target_hp = self.demo_health_max;
            }
        }

        self.process_triggers(world);
        self.check_stage_transitions(world);

        // === Bell ===
        let finished: Vec<Entity> = world
            .read_events::<TimerFinished>()
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

        // === Systems ===
        for sys in self.systems.iter_mut() {
            sys.update(world, dt);
        }

        // bugfix #2: останавливаем ворота после MovementSystem.
        self.tick_gates(world);

        // bugfix #5: события анимаций больше не отбрасываются.
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

    fn apply_postfx(&mut self, postfx: PostFx) { self.postfx = postfx; }

    fn on_play_enter(&mut self, _world: &World) -> Option<Box<dyn Any>> {
        Some(Box::new(()))
    }
    fn on_play_exit(&mut self, _state: Box<dyn Any>) {}

    /// bugfix #R: синхронизация pause-flag с engine. Раньше
    /// FortressDemo имел собственный demo_paused, который
    /// рассинхронизировался с editor.state.play.paused (двойной
    /// Escape оставлял demo_paused = true после снятия паузы).
    fn on_pause_changed(&mut self, paused: bool) {
        if self.demo_paused != paused {
            self.demo_paused = paused;
            log::info!("FortressDemo: pause = {}", paused);
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
            ammo: u32,
            ammo_reserve: u32,
        }

        let snap = StateSnapshot {
            campaign: &self.campaign,
            camera_pos: self.camera.first_person_pos.to_array(),
            camera_target: self.camera.target.to_array(),
            camera_distance: self.camera.distance,
            camera_yaw: self.camera.yaw,
            camera_pitch: self.camera.pitch,
            hp: self.demo_health,
            ammo: self.demo_ammo,
            ammo_reserve: self.demo_ammo_reserve,
        };
        match ron::ser::to_string(&snap) {
            Ok(s) => Some(s),
            Err(e) => {
                log::warn!("save_game_state failed: {}", e);
                None
            }
        }
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
            ammo: u32,
            #[serde(default)]
            ammo_reserve: u32,
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
                self.demo_ammo = snap.ammo;
                // bugfix #8: 0 = «патронов нет» (не «не сериализовано»).
                self.demo_ammo_reserve = snap.ammo_reserve;
                self.demo_reloading = false;
                self.demo_reload_timer = 0.0;
                self.hud.push("📂 Game loaded");
                log::info!(
                    "Game state loaded: stage {:?}, kills {}, key {}",
                    self.campaign.stage,
                    self.campaign.kills_total,
                    self.campaign.has_key_red,
                );
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

    fn on_kill(&mut self, _world: &mut World, _target: Entity) {
        self.campaign.kills_total += 1;
        self.campaign.kills_in_stage += 1;
        self.hud.push(format!(
            "Kill #{} (stage: {}/{})",
            self.campaign.kills_total,
            self.campaign.kills_in_stage,
            self.campaign.kills_required
        ));
    }

    fn collect_draws(&mut self, world: &mut World, renderer: &Renderer) -> Vec<MeshDraw> {
        use std::collections::HashMap;
        type BucketKey = (String, String, [u32; 4], bool, bool, [u32; 2]);
        let mut buckets: HashMap<BucketKey, Vec<InstanceData>> = HashMap::new();
        let planes = self.camera.frustum_planes();
        let cam_pos = self.camera.position();

        let lod_bias = self.postfx.lod_bias.max(0.01);
        let lod_dists = self.postfx.lod_distances;

        let mut new_prev: HashMap<Entity, glam::Mat4> = HashMap::new();

        let entities: Vec<_> = world.entities().to_vec();
        for e in entities {
            if let Some(v) = world.get::<Visible>(e) {
                if !v.0 { continue; }
            }
            if world.has::<Decal>(e) { continue; }

            let (Some(_t), Some(m), Some(mat)) = (
                world.get::<Transform>(e),
                world.get::<MeshHandle>(e),
                world.get::<MaterialHandle>(e),
            ) else { continue };

            let model = crate::game::world_matrix(world, e);
            let prev_model = self.prev_world_matrices.get(&e).copied().unwrap_or(model);
            new_prev.insert(e, model);

            let Some(mesh) = renderer.meshes.get(&m.0) else { continue };

            let (world_center, world_radius) = mesh.world_bounds(&model);

            if self.show_culling {
                let mut visible = true;
                for p in &planes {
                    let d = p.x * world_center.x + p.y * world_center.y
                          + p.z * world_center.z + p.w;
                    if d < -world_radius { visible = false; break; }
                }
                if !visible { continue; }
            }

            let dist = (world_center - cam_pos).length();
            let dist_eff = (dist - world_radius).max(0.0) / lod_bias;

            let lod_level = if mesh.lods.is_empty() { 0 }
                else if dist_eff < lod_dists[0] { 0 }
                else if dist_eff < lod_dists[1] || mesh.lods.len() < 1 { 1 }
                else if dist_eff < lod_dists[2] || mesh.lods.len() < 2 { 2.min(mesh.lods.len()) }
                else { 3.min(mesh.lods.len()) };

            let mesh_name = if lod_level == 0 { m.0.clone() }
                else { format!("{}__lod{}", m.0, lod_level - 1) };

            let material = renderer.materials.get(&mat.0)
                .unwrap_or_else(|| renderer.materials_default());
            let blend = material.alpha_mode == AlphaMode::Blend;
            let double_sided = material.double_sided;

            let color = world.get::<Tint>(e).map(|t| t.0)
                .unwrap_or([1.0, 1.0, 1.0, 1.0]);
            let tiling_size = world.get::<TextureTiling>(e).map(|t| t.size)
                .unwrap_or(1.0);

            let (scale, _, _) = model.to_scale_rotation_translation();
            let scale_abs = scale.abs();
            let local_extent = (mesh.aabb_max - mesh.aabb_min).abs();
            let we = local_extent * scale_abs;
            let tile = tiling_size.max(0.001);
            let mut uvx = we.x.max(we.z) / tile;
            let mut uvy = we.y / tile;
            if uvy < 0.001 { uvy = uvx; }
            if uvx < 0.001 { uvx = uvy; }

            let key = (
                mesh_name,
                mat.0.clone(),
                color.map(f32::to_bits),
                blend,
                double_sided,
                [(uvx * 1000.0) as i32 as u32, (uvy * 1000.0) as i32 as u32],
            );
            let inst = InstanceData::new_full(model, prev_model, color, [uvx, uvy]);
            buckets.entry(key).or_default().push(inst);
        }

        self.prev_world_matrices = new_prev;

        buckets.into_iter()
            .map(|((mesh, material_name, _, blend, double_sided, _), instances)| MeshDraw {
                mesh,
                instances,
                texture: Some(material_name),
                blend,
                double_sided,
            })
            .collect()
    }

    fn collect_lines(
        &mut self,
        world: &mut World,
        renderer: &Renderer,
        selected: &[Entity],
    ) -> Vec<LineVertex> {
        let mut batch = LineBatch::new();

        if self.show_grid {
            batch.grid(120.0, 2.0, [0.15, 0.18, 0.22, 0.5], [0.35, 0.40, 0.48, 0.7], 5);
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
            if let (Some(_), Some(mh)) = (world.get::<Transform>(e), world.get::<MeshHandle>(e)) {
                if let Some(mesh) = renderer.meshes.get(&mh.0) {
                    let model = crate::game::world_matrix(world, e);
                    let (center, radius) = mesh.world_bounds(&model);
                    batch.sphere_wireframe(center, radius * 1.05, [1.0, 0.85, 0.2, 1.0], 24);
                }
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
        // bugfix #6: реальный dt вместо фиксированных 1/60.
        self.hud.tick(self.last_dt);

        let is_victory = self.campaign.stage == Stage::Victory;

        // === Debug overlay (левый верх) ===
        if self.demo_show_debug {
            self.hud.debug_overlay(ui, 60.0, world.len(), self.camera.position());
        }

        // === Crosshair ===
        if !is_victory {
            self.hud.crosshair(ui, self.demo_ammo > 0);
        }

        // === Health (слева снизу) ===
        self.hud.health_bar(ui, self.demo_health, self.demo_health_max);

        // === Ammo (справа снизу). ЕДИНСТВЕННЫЙ блок ammo. ===
        let ammo_text = format!(
            "AMMO {}/{} · reserve {}",
            self.demo_ammo, self.demo_ammo_max, self.demo_ammo_reserve
        );
        {
            let size = 20.0;
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            let tw = ui.text_width(&ammo_text, size);
            let pad = 12.0;
            let box_w = tw + pad * 2.0;
            let box_h = size + pad * 1.6;
            let x = sw - 24.0 - box_w;
            let y = sh - 24.0 - box_h;

            let color = if self.demo_reloading {
                [1.0, 0.85, 0.4, 1.0]
            } else if self.demo_ammo == 0 && self.demo_ammo_reserve == 0 {
                [1.0, 0.3, 0.3, 1.0]
            } else if self.demo_ammo == 0 {
                [1.0, 0.5, 0.4, 1.0]
            } else {
                [1.0, 1.0, 0.85, 1.0]
            };

            ui.rect(x, y, box_w, box_h, [0.0, 0.0, 0.0, 0.55]);
            ui.text(x + pad, y + (box_h - size) * 0.5, &ammo_text, size, color);

            if self.demo_reloading {
                let progress = 1.0
                    - (self.demo_reload_timer / self.demo_reload_duration).clamp(0.0, 1.0);
                let bar_w = box_w;
                let bar_h = 6.0;
                let bar_y = y - bar_h - 6.0;

                ui.rect(x, bar_y, bar_w, bar_h, [0.15, 0.15, 0.18, 0.9]);
                ui.rect(x, bar_y, bar_w * progress, bar_h, [0.6, 0.9, 1.0, 1.0]);
                ui.text_centered(
                    x + box_w * 0.5,
                    bar_y - 10.0,
                    "RELOADING",
                    11.0,
                    [0.8, 0.9, 1.0, 1.0],
                );
            }
        }

        // === "OUT OF AMMO" в центре ===
        if self.demo_ammo == 0 && self.demo_ammo_reserve == 0 && !is_victory {
            ui.text_centered(
                ui.screen_w() * 0.5,
                ui.screen_h() * 0.5 + 60.0,
                "OUT OF AMMO",
                28.0,
                [1.0, 0.3, 0.3, 0.9],
            );
        }

        // === Notifications ===
        self.hud.notifications(ui);

        // === Objective (top center) ===
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

        // === Right top: kills + key ===
        //
        // bugfix bridge: счётчик убийств скрыт, когда акт не требует
        // убийств (kills_required == 0). В Act2+ прогресс идёт через
        // ключ / портал / босса, и «Kills 0/4» вводил в заблуждение.
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
                ui.rect(sw - kw - 40.0, y - 6.0, kw + 24.0, size + 12.0, [0.0, 0.0, 0.0, 0.55]);
                ui.text(sw - kw - 28.0, y, &kills_txt, size, [1.0, 0.85, 0.85, 1.0]);
                y += size + 10.0;
            }

            if self.campaign.has_key_red {
                let k = "🔑 Red Key";
                let kww = ui.text_width(k, size);
                ui.rect(sw - kww - 40.0, y - 6.0, kww + 24.0, size + 12.0, [0.0, 0.0, 0.0, 0.55]);
                ui.text(sw - kww - 28.0, y, k, size, [1.0, 0.4, 0.4, 1.0]);
            }
        }

        // === Dialog ===
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

        // === Victory screen ===
        if is_victory {
            let sw = ui.screen_w();
            let sh = ui.screen_h();
            ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.7]);

            ui.text_centered(sw * 0.5, sh * 0.3, "VICTORY", 64.0, [1.0, 0.85, 0.4, 1.0]);
            ui.text_centered(
                sw * 0.5,
                sh * 0.42,
                "The Fallen Citadel",
                24.0,
                [0.8, 0.85, 0.95, 1.0],
            );

            let stats = format!(
                "Kills: {}  ·  Time: {:.1}s",
                self.campaign.kills_total,
                self.campaign.victory_time.unwrap_or(0.0)
            );
            ui.text_centered(sw * 0.5, sh * 0.55, &stats, 20.0, [1.0, 1.0, 1.0, 1.0]);
        }

        // === Pause menu ===
        if self.demo_paused {
            if self.save_slots_cache_dirty {
                self.refresh_save_slots();
            }

            let sw = ui.screen_w();
            let sh = ui.screen_h();
            ui.rect(0.0, 0.0, sw, sh, [0.0, 0.0, 0.0, 0.65]);

            let pw = 500.0;
            let ph = 420.0;
            let px = (sw - pw) * 0.5;
            let py = (sh - ph) * 0.5;
            ui.rect(px - 2.0, py - 2.0, pw + 4.0, ph + 4.0, [0.55, 0.6, 0.75, 1.0]);
            ui.rect(px, py, pw, ph, [0.13, 0.15, 0.20, 0.98]);

            ui.text_centered(sw * 0.5, py + 30.0, "PAUSED", 28.0, [1.0, 0.9, 0.6, 1.0]);
            ui.text_centered(
                sw * 0.5,
                py + 58.0,
                self.campaign.stage.title(),
                14.0,
                [0.7, 0.75, 0.85, 1.0],
            );

            let bw = 220.0;
            let bh = 36.0;
            let bx = sw * 0.5 - bw * 0.5;

            if ui.button(bx, py + 82.0, bw, bh, "Resume") {
                self.demo_paused = false;
            }
            if ui.button(bx, py + 122.0, bw, bh, "Heal +25") {
                self.demo_health = (self.demo_health + 25.0).min(self.demo_health_max);
                self.hud.push("Healed +25");
            }
            if ui.button(bx, py + 162.0, bw, bh, "Refill Ammo") {
                self.demo_ammo = self.demo_ammo_max;
                self.demo_ammo_reserve = self.demo_ammo_reserve_max;
                self.demo_reloading = false;
                self.demo_reload_timer = 0.0;
                self.hud.push("Ammo + reserve refilled");
            }

            ui.text_centered(
                sw * 0.5,
                py + 218.0,
                "SAVE / LOAD",
                16.0,
                [0.8, 0.85, 1.0, 1.0],
            );

            let slot_w = 420.0;
            let slot_h = 32.0;
            let slot_x = sw * 0.5 - slot_w * 0.5;
            let mut y = py + 245.0;

            let slots = self.save_slots_cache.clone();
            let mut action: Option<(u32, &'static str)> = None;

            for (slot, header) in &slots {
                let label = match header {
                    Some(h) => format!(
                        "Slot {}  ·  {}  ·  {} kills  ·  {:.0}s",
                        slot + 1,
                        h.stage_title,
                        h.kills,
                        h.playtime_secs,
                    ),
                    None => format!("Slot {}  ·  — empty —", slot + 1),
                };

                let btn_w = 44.0;
                let gap = 4.0;

                if ui.button(slot_x, y, btn_w, slot_h, "S") {
                    action = Some((*slot, "save"));
                }
                if header.is_some()
                    && ui.button(slot_x + btn_w + gap, y, btn_w, slot_h, "L")
                {
                    action = Some((*slot, "load"));
                }
                if header.is_some()
                    && ui.button(slot_x + (btn_w + gap) * 2.0, y, btn_w, slot_h, "X")
                {
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

            ui.text_centered(
                sw * 0.5,
                py + ph - 18.0,
                "S = Save · L = Load · X = Delete",
                11.0,
                [0.6, 0.65, 0.75, 0.8],
            );
        } else if !is_victory {
            let text = "Esc — пауза · ЛКМ — выстрел · Space — прыжок · R — перезарядка · F1 — debug · WASD — движение";
            ui.text_centered(
                ui.screen_w() * 0.5,
                ui.screen_h() - 24.0,
                text,
                12.0,
                [1.0, 1.0, 1.0, 0.5],
            );
        }
    }
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
    renderer.add_material("gold",
        Material::new([1.0, 0.85, 0.3, 1.0]).with_metallic_roughness(1.0, 0.25));
    renderer.add_material("glass",
        Material::new([0.7, 0.85, 1.0, 0.35]).with_metallic_roughness(0.2, 0.05)
            .with_alpha_mode(AlphaMode::Blend));
    renderer.add_material("rpg_dungeon",
        Material::new([0.13, 0.11, 0.16, 1.0]).with_metallic_roughness(0.0, 0.95));
}