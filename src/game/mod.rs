pub mod ai;
pub mod animation;
pub mod animation_blend;
pub mod audio;
pub mod components;
pub mod crafting;
pub mod decals;
pub mod dialogs;
pub mod items;
pub mod lights;
pub mod locale;
pub mod quests;
pub mod rpg;
pub mod stats;
pub mod timers;
pub mod vendors;
pub mod weapons;
pub mod world_gen;

pub use ai::{
    AiAgent, AiState, AiTarget, DebugPath, Enemy, NoiseEvent, NoiseKind, PatrolPath,
};
pub use animation::{
    AnimationEvent, AnimationEventTriggered, AnimationEvents, AnimationEventsFile,
    AnimationRuntime,
};
pub use animation_blend::{
    AnimationBlendPlayer, BlendOutput, BlendSample, BlendTree1D,
};
pub use audio::{attenuation, source_position, AudioBus, AudioSource};
pub use components::{
    AnimationPlayer, Chase, Elevator, ElevatorState, Health, Interactable, MaterialHandle,
    MeshHandle, Name, Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint,
    Transform, Trigger, TriggerAction, Velocity, Visible,
    HealthPickup, AmmoPickup, RootMotion,
};
pub use crafting::{
    CraftingRegistry, CraftingSession, RecipeDef, RecipeInput, RecipeOutput,
};
pub use decals::Decal;
pub use items::{
    roll_loot, Equipment, Inventory, Item, ItemKind, ItemRegistry, ItemStack,
    LootEntry, LootRegistry, LootTable, Rarity,
};
pub use lights::{DirectionalLight, PointLight};
pub use stats::{
    Attributes, Experience, PlayerStats, STAT_DESCRIPTIONS, STAT_NAMES,
};
pub use timers::{Timer, TimerFinished, TimerMode, TimerSystem};
pub use vendors::{
    VendorDef, VendorItem, VendorNpc, VendorRegistry, VendorSession,
};
pub use weapons::{
    apply_spread, Weapon, WeaponKind, WeaponRegistry, WeaponRuntime,
};
pub use world_gen::{Biome, WorldGen, WorldGenConfig, WorldGenResult}; // <-- ЭКСПОРТ

use glam::Mat4;

use crate::ecs::{Entity, World};

pub fn world_matrix(world: &World, entity: Entity) -> Mat4 {
    let mut mat = world
        .get::<Transform>(entity)
        .map(|t| t.matrix())
        .unwrap_or(Mat4::IDENTITY);

    let mut current = entity;
    let mut depth = 0;
    while let Some(Parent(p)) = world.get::<Parent>(current).copied() {
        if depth > 32 {
            break;
        }
        let parent_mat = world
            .get::<Transform>(p)
            .map(|t| t.matrix())
            .unwrap_or(Mat4::IDENTITY);
        mat = parent_mat * mat;
        current = p;
        depth += 1;
    }
    mat
}

pub fn world_position(world: &World, entity: Entity) -> Option<glam::Vec3> {
    world.get::<Transform>(entity)?;
    Some(world_matrix(world, entity).transform_point3(glam::Vec3::ZERO))
}