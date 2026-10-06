pub mod components;
pub mod decals;
pub mod lights;
pub mod rpg;

pub use decals::Decal;
pub use components::{
    AnimationPlayer, Chase, Elevator, ElevatorState, Health, Interactable, MaterialHandle,
    MeshHandle, Name, Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint,
    Transform, Trigger, TriggerAction, Velocity, Visible,
};
pub use lights::{DirectionalLight, PointLight};

use glam::Mat4;
use crate::ecs::{Entity, World};

/// Обход цепочки Parent и вычисление мировой матрицы.
///
/// `world_matrix` **никогда** не возвращает `None`: если у entity нет
/// собственного `Transform`, берётся `Mat4::IDENTITY`. Это сознательно —
/// функция вызывается из рендера, физики, gizmo, где «нет Transform»
/// означает «позиция по умолчанию», а не «нечего считать».
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

/// ИЗМЕНЕНО (#17): явная проверка вместо `let _ = ...?`.
///
/// Прежний код:
///
/// ```ignore
/// let _ = world.get::<Transform>(entity)?;
/// Some(world_matrix(world, entity).transform_point3(glam::Vec3::ZERO))
/// ```
///
/// Работал корректно, но выглядел как трюк ради применения `?`:
/// «получить Transform, выбросить результат, но если нет — выйти».
/// Явный statement говорит то же самое без удивления при чтении.
///
/// Семантика не изменилась: если у entity нет собственного
/// `Transform`, возвращаем `None`. Это отличается от `world_matrix`,
/// которая в этом случае вернула бы `IDENTITY` и дала бы
/// `Vec3::ZERO` — визуально то же, но семантически вводит в
/// заблуждение: `Some(ZERO)` читалось бы как «позиция есть, она в
/// начале координат». Для «нет Transform = нет позиции» (например,
/// при проверке существования) нужен именно `None`.
///
/// NB: `world_matrix` для entity **с** `Transform`, но **без**
/// `Parent`, вернёт её матрицу. Для entity с `Parent` — умножит
/// цепочку вверх до 32 уровней (защита от циклов).
pub fn world_position(world: &World, entity: Entity) -> Option<glam::Vec3> {
    world.get::<Transform>(entity)?;
    Some(world_matrix(world, entity).transform_point3(glam::Vec3::ZERO))
}