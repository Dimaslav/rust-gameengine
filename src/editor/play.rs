use crate::ecs::Entity;
use crate::game::components::CharacterController;
use crate::render::camera::CameraMode;
use glam::Vec3;

pub struct PlayState {
    /// `true` — игра в play-режиме. Если `false`, `controller` и
    /// health/ammo не используются.
    pub active: bool,
    /// `true` — игра на паузе. Имеет смысл **только** при `active == true`.
    /// `active == false && paused == true` — неопределённое состояние;
    /// `exit_play` принудительно выставляет `paused = false`.
    pub paused: bool,
    /// Позиция «глаз» для спавна / респавна. Обновляется каждый кадр.
    pub saved_position: Vec3,

    // === C1.3: runtime отделён в CharacterController ===
    /// Runtime-состояние персонажа (velocity, coyote, jump buffer,
    /// bob, ground_normal, crouching). Сбрасывается при входе в
    /// play-mode через `CharacterController::new(&config)`.
    pub controller: CharacterController,

    // === Config: движение ===
    /// Высота глаз над ступнями в положении стоя (м).
    pub eye_height: f32,
    /// Радиус полусфер капсулы (м).
    pub player_radius: f32,
    /// **Полная** высота капсулы, включая обе полусферы (м).
    /// Должна быть `>= 2 * player_radius` (см. `CharacterConfig::validate`).
    pub player_height: f32,
    /// Скорость ходьбы (м/с).
    pub walk_speed: f32,
    /// Скорость бега (м/с).
    pub run_speed: f32,
    /// Скорость прыжка (м/с).
    pub jump_speed: f32,
    /// Гравитация (м/с²).
    pub gravity: f32,
    /// Резервный «пол» на высоте Y. Не часть геометрии сцены.
    pub floor_y: f32,
    /// Чувствительность мыши (множитель).
    pub look_sensitivity: f32,

    // === Camera (Step 10.2) ===
    /// Режим камеры: `FirstPerson` / `ThirdPerson`. `Fly`/`Orbit`
    /// используются редактором, `move_character` их игнорирует.
    pub camera_mode: CameraMode,
    pub tps_distance: f32,
    pub tps_shoulder_offset: f32,
    pub tps_pivot_height: f32,

    // === Config: движение (продолжение) ===
    /// Coyote-время (сек). См. `CharacterConfig`.
    pub coyote_time: f32,
    /// Jump buffer (сек).
    pub jump_buffer_time: f32,
    /// Время разгона на земле (сек).
    pub ground_accel_tau: f32,
    /// Время разгона в воздухе (сек).
    pub air_accel_tau: f32,
    /// Максимальная высота «спрыгивания» вниз (м).
    pub step_down_max: f32,
    /// Косинус максимального угла склона.
    pub slope_walk_limit_cos: f32,
    /// Скорость скольжения на крутом склоне (м/с).
    pub slope_slide_speed: f32,
    /// Высота глаз в приседе (м).
    pub crouch_height: f32,
    /// Множитель скорости в приседе.
    pub crouch_speed_mult: f32,
    /// Сила расталкивания динамических тел. `1.0` — базовое.
    pub push_strength: f32,

    // === Health / ammo / combat ===
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
    /// Highlight-entity под прицелом. `Entity` поколенческий: перед
    /// использованием проверяй `world.is_alive()`.
    pub highlight: Option<Entity>,

    // === Head-bob ===
    pub bob_enabled: bool,
    pub bob_amplitude: f32,

    // === HUD ===
    pub show_crosshair: bool,
    pub show_hud: bool,
    pub show_health: bool,
    pub show_ammo: bool,
}

impl Default for PlayState {
    fn default() -> Self {
        let eye_height = 1.7;
        let max_health = 100.0;
        let max_ammo = 30;
        Self {
            active: false,
            paused: false,
            saved_position: Vec3::new(0.0, eye_height, 45.0),

            // C1.3: controller с дефолтным config.
            controller: CharacterController::default(),

            eye_height,
            player_radius: 0.35,
            player_height: 1.8,
            walk_speed: 6.0,
            run_speed: 12.0,
            jump_speed: 6.0,
            gravity: 20.0,
            floor_y: 0.0,
            look_sensitivity: 0.0025,

            camera_mode: CameraMode::FirstPerson,
            tps_distance: 4.0,
            tps_shoulder_offset: 0.5,
            tps_pivot_height: -0.2,

            coyote_time: 0.15,
            jump_buffer_time: 0.15,
            ground_accel_tau: 0.06,
            air_accel_tau: 0.30,
            step_down_max: 0.5,
            slope_walk_limit_cos: std::f32::consts::FRAC_1_SQRT_2,
            slope_slide_speed: 6.0,
            crouch_height: 1.0,
            crouch_speed_mult: 0.5,
            push_strength: 1.0,

            health: max_health,
            max_health,
            ammo: max_ammo,
            max_ammo,
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

            show_crosshair: true,
            show_hud: true,
            show_health: true,
            show_ammo: true,
        }
    }
}