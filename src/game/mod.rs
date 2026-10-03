pub mod components;
pub mod rpg;

pub use components::{
    AnimationPlayer, Chase, Health, Interactable, MaterialHandle, MeshHandle, Name, Parent,
    SkeletonHandle, Spinner, TextureTiling, Tint, Transform, Trigger, TriggerAction, Velocity,
    Visible,
};

use glam::Mat4;
use crate::ecs::{Entity, World};

/// Обход цепочки Parent и вычисление мировой матрицы.
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
    let _ = world.get::<Transform>(entity)?;
    Some(world_matrix(world, entity).transform_point3(glam::Vec3::ZERO))
}