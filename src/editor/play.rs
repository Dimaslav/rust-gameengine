//! Состояние Play-режима.

use glam::Vec3;

pub struct PlayState {
    pub active: bool,
    pub saved_position: Vec3,
    pub vertical_velocity: f32,
    pub on_ground: bool,

    // Движение
    pub eye_height: f32,
    pub player_radius: f32,
    pub player_height: f32,
    pub walk_speed: f32,
    pub run_speed: f32,
    pub jump_speed: f32,
    pub gravity: f32,

    /// Чувствительность мыши в Play. Умножается на delta.
    pub look_sensitivity: f32,

    // Head bob
    pub bob_enabled: bool,
    pub bob_amplitude: f32,
    /// Пройденное расстояние с момента старта bob-цикла.
    pub bob_distance: f32,
    /// Текущий сдвиг по Y для bob. Сглаживается.
    pub bob_current: f32,

    // HUD
    pub show_crosshair: bool,
    pub show_hud: bool,
}

impl Default for PlayState {
    fn default() -> Self {
        Self {
            active: false,
            saved_position: Vec3::new(0.0, 1.7, -8.0),
            vertical_velocity: 0.0,
            on_ground: true,
            eye_height: 1.7,
            player_radius: 0.35,
            player_height: 1.8,
            walk_speed: 6.0,
            run_speed: 12.0,
            jump_speed: 6.0,
            gravity: 20.0,
            look_sensitivity: 0.0025,
            bob_enabled: false, // выключено по умолчанию — см. ниже
            bob_amplitude: 0.03,
            bob_distance: 0.0,
            bob_current: 0.0,
            show_crosshair: true,
            show_hud: true,
        }
    }
}