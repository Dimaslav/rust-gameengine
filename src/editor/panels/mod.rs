//! Панели редактора.
//!
//! Структура после Недели 2 (часть 1):
//! * DTO: `Stats`, `UiAssets`, `AudioSnapshot` — живут здесь, чтобы `App`
//!   импортировал их из одного места.
//! * `top_bar`, `left_panel`, `right_panel`, `overlays` — каркас.
//! * `outliner`, `stats`, `audio`, `bookmarks`, `terrain`, `palette` —
//!   панели, вынесенные из `ui.rs`.
//! * `details`, `renderer`, `content`, `scripts`, `command_palette`,
//!   `inspector_audio` — пока ещё в старых модулях, перенесутся в части 2.

pub mod audio;
pub mod bookmarks;
pub mod left_panel;
pub mod outliner;
pub mod overlays;
pub mod palette;
pub mod right_panel;
pub mod stats;
pub mod terrain;
pub mod top_bar;

use egui::Context;

use crate::assets::AssetDatabase;
use crate::ecs::World;
use crate::editor::{EditorAction, EditorState};
use crate::game::audio::AudioBus;
use crate::render::{Material, PostFx};
use crate::scripting::ScriptSystem;

// ============================================================
// DTO (перенесены из editor::ui)
// ============================================================

pub struct Stats {
    pub fps: f32,
    pub frame_time_max_ms: f32,
    pub hitches: u64,
    pub entities: usize,
    pub draws: usize,
    pub instances: usize,
    pub dir_lights: usize,
    pub point_lights: usize,
    pub lod_counts: [usize; 4],
    pub lod_triangles: [usize; 4],
    pub sel_entities: usize,
    pub sel_triangles: usize,
    pub sel_vertices: usize,
    pub extra_lines: Vec<(String, String)>,
}

pub struct UiAssets<'a> {
    pub mesh_names: &'a [String],
    pub material_names: &'a [String],
    pub texture_list: &'a [(String, u32, u32, bool)],
    pub selected_material: Option<(String, Material)>,
}

#[derive(Default)]
pub struct AudioSnapshot {
    pub bus_volumes: Vec<(AudioBus, f32)>,
    pub sound_names: Vec<String>,
    pub active_count: usize,
    pub available: bool,
    pub selected_source_entity: Option<crate::ecs::Entity>,
}

// ============================================================
// PanelContext — задел на Шаг 2 (egui_tiles)
// ============================================================
//
// На Неделе 2 не используется. Экземпляр **нельзя** создавать через
// `std::mem::zeroed()` — для `&T` это UB. На Шаге 2 поля будут
// инициализироваться реальными ссылками из `App::redraw`.
pub struct PanelContext<'a> {
    pub renderer: &'a crate::render::Renderer,
    pub assets: &'a UiAssets<'a>,
    pub audio: &'a AudioSnapshot,
    pub scripts: &'a ScriptSystem,
    pub asset_db: &'a mut AssetDatabase,
    pub postfx: &'a mut PostFx,
    pub stats: &'a Stats,
}

// ============================================================
// draw_all — точка входа
// ============================================================

#[allow(clippy::too_many_arguments)]
pub fn draw_all(
    ctx: &Context,
    state: &mut EditorState,
    world: &mut World,
    postfx: &mut PostFx,
    stats: &Stats,
    assets: &UiAssets<'_>,
    audio: &AudioSnapshot,
    asset_db: &mut AssetDatabase,
    scripts: &ScriptSystem,
) -> Option<EditorAction> {
    let mut action: Option<EditorAction> = None;

    // ============================================================
    // 0. Оверлеи и контекстные меню
    // ============================================================
    overlays::draw_box_select_overlay(ctx, state);
    overlays::draw_context_menu(ctx, state, world, &mut action);

    if state.command_palette_open {
        crate::editor::ui::draw_command_palette(ctx, state, &mut action);
    }

    // ============================================================
    // 1. Верхние полосы
    // ============================================================
    top_bar::draw(ctx, state, stats, &mut action);

    if !state.play.active {
        if state.layout.show_palette_bar {
            palette::draw(ctx, state);
        }
        if state.layout.show_terrain_bar {
            terrain::draw(ctx, state, &mut action);
        }
    }

    // ============================================================
    // 2. Боковые панели
    // ============================================================
    if !state.play.active {
        left_panel::draw(ctx, state, world, assets, stats, &mut action);
        right_panel::draw(ctx, state, world, assets, postfx, &mut action);

        bookmarks::draw(ctx, state, &mut action);
        audio::draw(ctx, state, audio, &mut action);
    }

    // AudioSource inspector — пока из старого модуля (переедет в details).
    crate::editor::inspector_audio::draw_window(ctx, state, world, audio, &mut action);

    // ============================================================
    // 3. Bottom / content / scripts — пока из старых модулей
    // ============================================================
    crate::editor::content_browser::draw_window(
        ctx,
        &mut state.layout.show_content_browser,
        asset_db,
    );

    let mut show_scripts = state.layout.show_script_panel;
    crate::editor::script_panel::draw_window(
        ctx,
        &mut show_scripts,
        state,
        world,
        scripts,
        &mut action,
    );
    state.layout.show_script_panel = show_scripts;

    // ============================================================
    // 4. Pause overlay
    // ============================================================
    if state.play.active && state.play.paused {
        overlays::draw_pause_overlay(ctx, state, &mut action);
    }

    // ============================================================
    // 5. Hint popup
    // ============================================================
    if let Some((text, ttl)) = &state.hint {
        let alpha = ttl.min(1.0).max(0.0);
        egui::Area::new(egui::Id::new("editor_hint"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style())
                    .fill(egui::Color32::from_rgba_unmultiplied(
                        20, 20, 30, (220.0 * alpha) as u8,
                    ))
                    .inner_margin(egui::Margin::symmetric(12, 6))
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new(text).color(
                            egui::Color32::from_rgba_unmultiplied(
                                255, 230, 180, (255.0 * alpha) as u8,
                            ),
                        ));
                    });
            });
    }

    action
}