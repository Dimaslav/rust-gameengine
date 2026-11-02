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
pub mod scripts;
pub mod settings_panel;
pub mod stats;
pub mod terrain;
pub mod top_bar;
pub mod viewport_toolbar;
pub mod workspace_tabs;

use egui::Context;

use crate::assets::AssetDatabase;
use crate::ecs::World;
use crate::editor::{EditorAction, EditorState};
use crate::game::audio::AudioBus;
use crate::render::{Material, PostFx};
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

    // === Этап 0 ===
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

pub struct PanelContext<'a> {
    pub renderer: &'a crate::render::Renderer,
    pub assets: &'a UiAssets<'a>,
    pub audio: &'a AudioSnapshot,
    pub scripts: &'a ScriptSystem,
    pub asset_db: &'a mut AssetDatabase,
    pub postfx: &'a mut PostFx,
    pub stats: &'a Stats,
}

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

    overlays::draw_box_select_overlay(ctx, state);
    overlays::draw_context_menu(ctx, state, world, &mut action);

    if state.command_palette_open {
        command_palette::draw(ctx, state, &mut action);
    }

    // ИСПРАВЛЕНО: в egui 0.37 SidePanel и TopBottomPanel удалены.
    // Мы создаём корневой Area, заполняем его размером экрана и 
    // последовательно вырезаем из него панели через Panel::top/left/right.
    egui::Area::new(egui::Id::unique("editor_root_area"))
        .fixed_pos(egui::pos2(0.0, 0.0))
        .order(egui::Order::Background)
        .show(ctx, |ui| {
            ui.set_min_size(ui.ctx().content_rect().size());

            // 1. Верхние панели
            top_bar::draw(ui, state, stats, &mut action);

            if !state.play.active {
                if state.layout.show_palette_bar {
                    palette::draw(ui, state);
                }
                if state.layout.show_terrain_bar {
                    terrain::draw(ui, state, &mut action);
                }
            }

            // 2. Боковые панели
            if !state.play.active {
                left_panel::draw(ui, state, world, assets, stats, &mut action);
                right_panel::draw(ui, state, world, assets, postfx, &mut action);
            }

            // 3. CentralPanel забирает всё оставшееся место (вьюпорт)
            egui::CentralPanel::default()
                .frame(egui::Frame::NONE)
                .show(ui, |_ui| {});
        });

    if !state.play.active {
        viewport_toolbar::draw(ctx, state);
        bookmarks::draw(ctx, state, &mut action);
        audio::draw(ctx, state, audio, &mut action);
    }

    inspector_audio::draw_window(ctx, state, world, audio, &mut action);

    let show_content = &mut state.layout.show_content_browser;
    let thumbnails = &mut state.thumbnails;
    content::draw_window(ctx, show_content, asset_db, thumbnails);

    let mut show_scripts = state.layout.show_script_panel;
    scripts::draw_window(
        ctx,
        &mut show_scripts,
        state,
        &*world,
        scripts,
        &mut action,
    );
    state.layout.show_script_panel = show_scripts;

    settings_panel::draw_window(ctx, state, stats);

    if state.play.active && state.play.paused {
        overlays::draw_pause_overlay(ctx, state, &mut action);
    }

    if let Some((text, ttl)) = &state.hint {
        let alpha = ttl.min(1.0).max(0.0);
        egui::Area::new(egui::Id::unique("editor_hint"))
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