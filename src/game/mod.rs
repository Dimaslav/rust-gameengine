pub mod ai;
pub mod animation;
pub mod audio;
pub mod components;
pub mod decals;
pub mod items;
pub mod lights;
pub mod rpg;
pub mod stats;
pub mod timers;
pub mod weapons;

pub use ai::{
    AiAgent, AiState, AiTarget, DebugPath, Enemy, NoiseEvent, NoiseKind, PatrolPath,
};
pub use animation::{
    AnimationEvent, AnimationEventTriggered, AnimationEvents, AnimationEventsFile,
    AnimationRuntime,
};
pub use items::{
    roll_loot, Equipment, Inventory, Item, ItemKind, ItemRegistry, ItemStack,
    LootEntry, LootRegistry, LootTable, Rarity,
};
pub use audio::{attenuation, source_position, AudioBus, AudioSource};
pub use decals::Decal;
pub use components::{
    AnimationPlayer, Chase, Elevator, ElevatorState, Health, Interactable, MaterialHandle,
    MeshHandle, Name, Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint,
    Transform, Trigger, TriggerAction, Velocity, Visible,
};
pub use stats::{
    Attributes, Experience, PlayerStats, STAT_DESCRIPTIONS, STAT_NAMES,
};
pub use lights::{DirectionalLight, PointLight};
pub use timers::{Timer, TimerFinished, TimerMode, TimerSystem};
pub use weapons::{
    apply_spread, Weapon, WeaponKind, WeaponRegistry, WeaponRuntime,
};

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
        if depth > 32 { break; }
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