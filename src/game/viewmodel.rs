//! View-model оружия: отрисовка ствола от первого лица.
//!
//! Простейшая реализация: оружие — обычная ECS-сущность, которая
//! каждый кадр двигается к позиции «камера + camera-space offset».
//! Отдельного depth-буфера нет, поэтому ствол может визуально
//! протыкать стены вплотную. Для MVP — приемлемо.

use glam::{EulerRot, Quat, Vec3};

use crate::ecs::{Entity, World};
use crate::game::components::{
    MaterialHandle, MeshHandle, Name, Transform, Tint, Visible,
};
use crate::game::weapons::Weapon;
use crate::render::Camera3D;

/// Параметры sway/bob, привязанные к runtime-состоянию игрока.
#[derive(Debug, Clone, Copy)]
pub struct ViewModelMotion {
    /// Сглаженное смещение от движения мыши (sway).
    pub sway: Vec3,
    /// Текущая фаза bob'а (накапливается при ходьбе).
    pub bob_phase: f32,
    /// Текущая амплитуда bob'а (0 стоя, 1 при ходу, 1.4 при бегу).
    pub bob_amp: f32,
    /// Текущая отдача (0..1), интерполируется к 0.
    pub recoil: f32,
    /// Текущий draw-таймер (0 = поднято, 1 = опущено).
    pub draw_t: f32,
    /// Прошлая mouse_delta — для вычисления velocity.
    pub last_mouse: (f32, f32),
}

impl Default for ViewModelMotion {
    fn default() -> Self {
        Self {
            sway: Vec3::ZERO,
            bob_phase: 0.0,
            bob_amp: 0.0,
            recoil: 0.0,
            draw_t: 0.0,
            last_mouse: (0.0, 0.0),
        }
    }
}

/// Спавн сущности оружия.
///
/// Возвращает `Entity`, которую надо хранить в `FortressDemo`,
/// чтобы каждый кадр двигать её к камере.
pub fn spawn_viewmodel(
    world: &mut World,
    weapon: &Weapon,
    initial_camera_pos: Vec3,
) -> Option<Entity> {
    let mesh = weapon.mesh.as_ref()?;
    let material = weapon.material.as_ref()?;

    let e = world.spawn();
    world.insert(e, Name(format!("ViewModel_{}", weapon.id)));
    world.insert(e, Transform::at(initial_camera_pos).with_scale(weapon.scale));
    world.insert(e, MeshHandle(mesh.clone()));
    world.insert(e, MaterialHandle(material.clone()));
    world.insert(e, Visible(false)); // покажем при входе в Play
    Some(e)
}

/// Заменить текущий view-model на другой.
///
/// Если у оружия нет меша — старая сущность просто скрывается.
pub fn set_viewmodel_weapon(
    world: &mut World,
    entity: Entity,
    weapon: &Weapon,
) {
    if let Some(mesh) = &weapon.mesh {
        world.insert(entity, MeshHandle(mesh.clone()));
    }
    if let Some(mat) = &weapon.material {
        world.insert(entity, MaterialHandle(mat.clone()));
    }
    if let Some(t) = world.get_mut::<Transform>(entity) {
        t.scale = Vec3::splat(weapon.scale);
    }
}

/// Обновить позицию оружия под камеру. Вызывается каждый кадр из
/// `FortressDemo::update`.
///
/// * `ads_blend` — 0 hip, 1 ads.
/// * `motion` — sway/bob/recoil/draw состояние.
/// * `dt` — для интерполяции motion.
pub fn update_viewmodel(
    world: &mut World,
    entity: Entity,
    weapon: &Weapon,
    camera: &Camera3D,
    ads_blend: f32,
    motion: &mut ViewModelMotion,
    mouse_delta: (f32, f32),
    player_velocity_xz: f32,
    is_moving_on_ground: bool,
    dt: f32,
) {
    // === Sway: мышь тянет ствол с задержкой ===
    let mouse_delta_norm = Vec3::new(
        -mouse_delta.0 * 0.0006,
        mouse_delta.1 * 0.0006,
        0.0,
    );
    motion.sway = motion.sway.lerp(mouse_delta_norm, (dt * 10.0).min(1.0));
    // Sway возвращается в 0, если мышь не двигается
    motion.sway *= 1.0 - (dt * 3.0).min(1.0);
    motion.last_mouse = mouse_delta;

    // === Bob: накапливаем фазу при движении ===
    let target_amp = if is_moving_on_ground {
        (player_velocity_xz / 6.0).clamp(0.0, 1.4)
    } else {
        0.0
    };
    motion.bob_amp += (target_amp - motion.bob_amp) * (dt * 6.0).min(1.0);
    motion.bob_phase += dt * (6.0 + player_velocity_xz * 0.8);

    let bob = Vec3::new(
        motion.bob_phase.sin() * 0.012 * motion.bob_amp,
        (motion.bob_phase * 2.0).sin() * 0.008 * motion.bob_amp,
        0.0,
    );

    // === Recoil: затухает к 0 ===
    motion.recoil = (motion.recoil - dt * 8.0).max(0.0);

    // === Draw: при смене оружия опускаем вниз ===
    motion.draw_t = (motion.draw_t - dt * 3.0).max(0.0);
    let draw_offset = Vec3::new(
        0.0,
        -motion.draw_t * motion.draw_t * 0.5,
        -motion.draw_t * 0.15,
    );

    // === Базовые offset'ы ===
    let hip = Vec3::from_array(weapon.hip_pos);
    let ads = Vec3::from_array(weapon.ads_pos);
    let mut offset = hip.lerp(ads, ads_blend);

    // Отдача: сдвигаем ствол назад и вверх
    offset.z += motion.recoil * weapon.kick_back;
    offset.y += motion.recoil * weapon.kick_back * 0.3;

    offset += motion.sway;
    offset += bob;
    offset += draw_offset;

    // === Rotation: hip_rot → ads_rot ===
    let hip_rot = Vec3::from_array(weapon.hip_rot);
    let ads_rot = Vec3::from_array(weapon.ads_rot);
    let local_rot_euler = hip_rot.lerp(ads_rot, ads_blend);

    // === Собираем world transform ===
    // Позиция: camera.position + offset, повёрнутый под yaw/pitch.
    //
    // Камера смотрит вдоль `forward()`. Offset в camera-space
    // (x=вправо, y=вверх, z=вперёд) переводим в world:
    //   world_offset = right * offset.x + up * offset.y + (-forward) * offset.z
    //     (потому что в camera-space "вперёд" = -Z).
    let cam_pos = camera.position();
    let cam_right = camera.right();
    let cam_up = camera.up();
    let cam_forward = camera.forward();

    let world_offset =
        cam_right * offset.x + cam_up * offset.y + cam_forward * (-offset.z);
    let new_pos = cam_pos + world_offset;

    // Ориентация: base yaw/pitch камеры + локальный euler.
    let base_rot = Quat::from_euler(EulerRot::YXZ, camera.yaw, camera.pitch, 0.0);
    let local_rot = Quat::from_euler(
        EulerRot::XYZ,
        local_rot_euler[0],
        local_rot_euler[1],
        local_rot_euler[2],
    );
    let final_rot = base_rot * local_rot;

    if let Some(t) = world.get_mut::<Transform>(entity) {
        t.position = new_pos;
        t.rotation = final_rot;
    }
}

/// Показать / скрыть view-model.
pub fn set_viewmodel_visible(world: &mut World, entity: Entity, visible: bool) {
    if !visible {
        world.insert(entity, Visible(false));
    } else {
        world.remove::<Visible>(entity);
    }
}