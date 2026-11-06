pub mod audio;
pub mod bookmarks;
pub mod command_palette;
pub mod content;
pub mod details;
pub mod inspector_audio;
pub mod left_panel;
pub mod outliner;
pub mod overlays;
pub mod palette;
pub mod renderer;
pub mod right_panel;
pub mod scene_validation;
pub mod scripts;
pub mod settings_panel;
pub mod stats;
pub mod terrain;
pub mod top_bar;
pub mod viewport_toolbar;
pub mod workspace_tabs;

use crate::assets::AssetDatabase;
use crate::ecs::World;
use crate::editor::{EditorAction, EditorState};
use crate::game::audio::AudioBus;
use crate::render::Material;
use crate::scripting::ScriptSystem;

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

    pub update_ms: f32,
    pub physics_ms: f32,
    pub ai_ms: f32,
    pub scripts_ms: f32,
    pub collect_ms: f32,
    pub render_ms: f32,
    pub egui_ms: f32,
    pub total_cpu_ms: f32,
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

// FIX (P2.2): удалена неиспользуемая структура `PanelContext`.
// Она была объявлена, но нигде не использовалась — попытка упаковать
// аргументы `draw_all`, которая не удалась.

#[allow(clippy::too_many_arguments)]
pub fn draw_all(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    world: &mut World,
    postfx: &mut crate::render::PostFx,
    stats: &Stats,
    assets: &UiAssets<'_>,
    audio: &AudioSnapshot,
    asset_db: &mut AssetDatabase,
    scripts: &ScriptSystem,
) -> (Option<EditorAction>, egui::Rect) {
    let mut action: Option<EditorAction> = None;

    overlays::draw_box_select_overlay(ui.ctx(), state);
    overlays::draw_context_menu(ui.ctx(), state, world, &mut action);

    if state.command_palette_open {
        command_palette::draw(ui.ctx(), state, &mut action);
    }

    // FIX #11: панели — напрямую на `ui`, без `egui::Area`.
    // В egui 0.34+ `Panel::show` принимает `&mut Ui`, а не `&Context`.
    top_bar::draw(ui, state, stats, &mut action);

    if !state.play.active {
        if state.layout.show_palette_bar {
            palette::draw(ui, state);
        }
        if state.layout.show_terrain_bar {
            terrain::draw(ui, state, &mut action);
        }
    }

    if !state.play.active {
        left_panel::draw(ui, state, world, assets, stats, &mut action);
        right_panel::draw(ui, state, world, assets, postfx, &mut action);
    }

    let central = egui::containers::panel::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(ui, |_ui| {});
    let viewport_rect = central.response.rect;

    if !state.play.active {
        // FIX (P3.1): передаём viewport_rect, чтобы тулбар был в углу
        // viewport'а, а не всего экрана.
        viewport_toolbar::draw(ui.ctx(), viewport_rect, state);
        bookmarks::draw(ui.ctx(), state, &mut action);
        audio::draw(ui.ctx(), state, audio, &mut action);
    }

    inspector_audio::draw_window(ui.ctx(), state, world, audio, &mut action);

    let show_content = &mut state.layout.show_content_browser;
    let thumbnails = &mut state.thumbnails;
    content::draw_window(ui.ctx(), show_content, asset_db, thumbnails);

    let mut show_scripts = state.layout.show_script_panel;
    scripts::draw_window(
        ui.ctx(),
        &mut show_scripts,
        state,
        &*world,
        scripts,
        &mut action,
    );
    state.layout.show_script_panel = show_scripts;

    settings_panel::draw_window(ui.ctx(), state, stats);
    scene_validation::draw_window(ui.ctx(), state, world, &mut action);

    if state.play.active && state.play.paused {
        overlays::draw_pause_overlay(ui.ctx(), state, &mut action);
    }

    if let Some((text, ttl)) = &state.hint {
        let alpha = ttl.clamp(0.0, 1.0);
        egui::Area::new(egui::Id::unique("editor_hint"))
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 80.0))
            .interactable(false)
            .show(ui.ctx(), |ui| {
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

    (action, viewport_rect)
}