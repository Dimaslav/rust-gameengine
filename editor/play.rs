//! Состояние Play-режима.

use crate::ecs::Entity;
use glam::Vec3;

pub struct PlayState {
    pub active: bool,
    pub paused: bool,

    pub saved_position: Vec3,
    pub vertical_velocity: f32,
    pub on_ground: bool,

    // === Движение ===
    pub eye_height: f32,
    pub player_radius: f32,
    pub player_height: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub jump_speed: f32,
    pub gravity: f32,
    pub floor_y: f32,
    pub look_sensitivity: f32,

    // === ИЗМЕНЕНО (Фаза 5): платформерный контроллер ===
    //
    // `horizontal_velocity` — XZ-скорость, накопленная между кадрами.
    // Раньше движение было instant (`motion = dir * speed * dt`), что
    // не давало air control. Теперь velocity интерполируется к
    // target с time constant, зависящим от on_ground.
    pub horizontal_velocity: Vec3,

    /// Время после схода с платформы, когда прыжок ещё срабатывает.
    pub coyote_time: f32,
    /// Текущий таймер coyote. Сброс в coyote_time при on_ground.
    pub coyote_timer: f32,

    /// Окно, в течение которого нажатие прыжка «запоминается» и
    /// сработает при следующем касании земли.
    pub jump_buffer_time: f32,
    pub jump_buffer_timer: f32,

    /// Time constant (сек) интерполяции velocity на земле.
    /// Меньше = резче разгон/торможение.
    pub ground_accel_tau: f32,
    /// То же для воздуха. Больше = меньше контроля в воздухе.
    pub air_accel_tau: f32,

    /// Максимальная высота, на которую персонаж «прилипает» к
    /// поверхности при спуске с уступа (step-down).
    pub step_down_max: f32,

    /// Порог «walkable slope»: cos(угла). cos(45°) ≈ 0.707.
    /// Если ground_normal.y ниже этого порога — начинается slide.
    pub slope_walk_limit_cos: f32,

    /// Скорость скольжения по вертикальному склону (m/s).
    pub slope_slide_speed: f32,

    /// Для отладки / HUD.
    pub ground_normal: Vec3,

    // === Crouch (приседание) ===
    pub crouch_height: f32,
    pub crouch_speed_mult: f32,
    pub current_eye_height: f32,
    pub crouching: bool,

    pub push_strength: f32,

    // === Бой / HUD ===
    pub health: f32,
    pub max_health: f32,
    pub ammo: u32,
    pub max_ammo: u32,
    pub damage_per_shot: f32,
    pub gun_range: f32,
    pub bullet_speed: f32,
    pub fire_cooldown: f32,
    pub fire_cooldown_max: f32,
    pub interact_cooldown: f32,
    pub interact_cooldown_max: f32,
    pub interact_distance: f32,

    pub highlight: Option<Entity>,

    // === Head bob ===
    pub bob_enabled: bool,
    pub bob_amplitude: f32,
    pub bob_distance: f32,
    pub bob_current: f32,

    // === HUD ===
    pub show_crosshair: bool,
    pub show_hud: bool,
    pub show_health: bool,
    pub show_ammo: bool,
}

impl Default for PlayState {
    fn default() -> Self {
        Self {
            active: false,
            paused: false,
            saved_position: Vec3::new(0.0, 1.7, 45.0),
            vertical_velocity: 0.0,
            on_ground: true,

            eye_height: 1.7,
            player_radius: 0.35,
            player_height: 1.8,
            walk_speed: 6.0,
            run_speed: 12.0,
            jump_speed: 6.0,
            gravity: 20.0,
            floor_y: 0.0,
            look_sensitivity: 0.0025,

            horizontal_velocity: Vec3::ZERO,
            coyote_time: 0.15,
            coyote_timer: 0.0,
            jump_buffer_time: 0.15,
            jump_buffer_timer: 0.0,
            ground_accel_tau: 0.06,
            air_accel_tau: 0.30,
            step_down_max: 0.5,
            // cos(45°) = 0.7071
            slope_walk_limit_cos: 0.7071,
            slope_slide_speed: 6.0,
            ground_normal: Vec3::Y,

            crouch_height: 1.0,
            crouch_speed_mult: 0.5,
            current_eye_height: 1.7,
            crouching: false,

            push_strength: 1.0,

            health: 100.0,
            max_health: 100.0,
            ammo: 30,
            max_ammo: 30,
            damage_per_shot: 25.0,
            gun_range: 100.0,
            bullet_speed: 80.0,
            fire_cooldown: 0.0,
            fire_cooldown_max: 0.15,
            interact_cooldown: 0.0,
            interact_cooldown_max: 0.25,
            interact_distance: 5.0,
            highlight: None,

            bob_enabled: false,
            bob_amplitude: 0.03,
            bob_distance: 0.0,
            bob_current: 0.0,

            show_crosshair: true,
            show_hud: true,
            show_health: true,
            show_ammo: true,
        }
    }
}