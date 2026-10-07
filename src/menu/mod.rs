//! Меню приложения (main menu + settings).

pub mod main_menu;
pub mod settings_menu;

use crate::app_state::{AppMode, AppSettings};

#[derive(Default)]
pub struct MenuActions {
    pub new_game: bool,
    pub continue_game: bool,
    pub open_settings: bool,
    pub quit: bool,
    pub back: bool,
    pub apply_settings: bool,
}

pub fn draw(
    ctx: &egui::Context,
    mode: &mut AppMode,
    settings: &mut AppSettings,
    has_save: bool,
) -> MenuActions {
    let mut actions = MenuActions::default();

    // Затемнённый фон
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Background,
        egui::Id::new("menu_bg"),
    ));
    painter.rect_filled(
        screen,
        egui::CornerRadius::ZERO,
        egui::Color32::from_rgba_unmultiplied(8, 10, 16, 235),
    );

    match mode {
        AppMode::MainMenu => main_menu::draw(ctx, has_save, &mut actions),
        AppMode::Settings { .. } => settings_menu::draw(ctx, settings, &mut actions),
        AppMode::InGame => {}
    }

    actions
}