//! Панели редактора.

use std::collections::{HashMap, HashSet};

use crate::ecs::{Entity, World};
use crate::editor::camera_bookmarks::CameraBookmark;
use crate::editor::gizmo::GizmoMode;
use crate::editor::palette::{PaletteItem, PaletteState};
use crate::editor::play::PlayState;
use crate::editor::{EditorAction, EditorState};
use crate::game::audio::{AudioBus, AudioSource};
use crate::game::components::{
    AnimationPlayer, Chase, Elevator, ElevatorState, Health, Interactable, MaterialHandle,
    MeshHandle, Name, Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint,
    Transform, Trigger, TriggerAction, Velocity, Visible,
};
use crate::game::lights::{DirectionalLight, PointLight};
use crate::physics::{BodyType, Collider, PhysicsMaterial, RigidBody};
use crate::render::{AlphaMode, Material, PostFx, Tonemapper};
use glam::Vec3;

// ============================================================
// Публичные типы
// ============================================================

#[derive(Default)]
pub struct UiState {
    pub show_renderer_panel: bool,
    pub show_stats_panel: bool,
    pub show_hierarchy_panel: bool,
    pub show_inspector_panel: bool,
    pub show_audio_panel: bool,
    pub left_panel_width: f32,
    pub right_panel_width: f32,
    pub component_filter: String,
    pub initialized: bool,
    pub command_palette_open: bool,
    pub command_palette_query: String,
    pub command_palette_selected: usize,
    pub show_bookmarks_panel: bool,
    pub show_content_browser: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            show_renderer_panel: true, show_stats_panel: true,
            show_hierarchy_panel: true, show_inspector_panel: true,
            show_audio_panel: false,
            left_panel_width: 260.0, right_panel_width: 340.0,
            component_filter: String::new(), initialized: false,
            command_palette_open: false, command_palette_query: String::new(),
            command_palette_selected: 0, show_bookmarks_panel: false,
            show_content_browser: false,
        }
    }
}

pub struct Stats {
    pub fps: f32, pub frame_time_max_ms: f32, pub hitches: u64,
    pub entities: usize, pub draws: usize, pub instances: usize,
    pub dir_lights: usize, pub point_lights: usize,
    pub lod_counts: [usize; 4], pub lod_triangles: [usize; 4],
    pub sel_entities: usize, pub sel_triangles: usize, pub sel_vertices: usize,
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
    pub selected_source_entity: Option<Entity>,
}

// ============================================================
// Точка входа
// ============================================================

pub fn draw(
    ctx: &egui::Context, state: &mut UiState, editor: &mut EditorState,
    world: &mut World, postfx: &mut PostFx, stats: &Stats, assets: &UiAssets<'_>,
    audio: &AudioSnapshot,
    asset_db: &mut crate::assets::AssetDatabase,
) -> Option<EditorAction> {
    let mut action: Option<EditorAction> = None;
    initialize_from_settings(state, editor);
    draw_box_select_overlay(ctx, editor);
    draw_context_menu(ctx, editor, world, &mut action);
    if state.command_palette_open { draw_command_palette(ctx, state, &mut action); }
    draw_top_bar(ctx, state, editor, stats, &mut action);
    if !editor.play.active {
        draw_palette_bar(ctx, editor);
        draw_left_panel(ctx, state, editor, world, assets, stats, &mut action);
        draw_right_panel(ctx, state, editor, world, assets, postfx, &mut action);
        draw_bookmarks_panel(ctx, state, editor, &mut action);
        draw_audio_panel(ctx, state, audio, &mut action);
    }
    crate::editor::inspector_audio::draw_window(ctx, editor, world, audio, &mut action);

    crate::editor::content_browser::draw_window(
        ctx,
        &mut state.show_content_browser,
        asset_db,
    );

    if editor.play.active {
        if editor.play.paused {
            draw_pause_overlay(ctx, editor, &mut action);
        }
    }

    // === editor sprint-1: hint overlay ===
    if let Some((text, ttl)) = &editor.hint {
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

fn initialize_from_settings(state: &mut UiState, editor: &EditorState) {
    if state.initialized { return; }
    let s = &editor.settings;
    state.show_renderer_panel = s.show_renderer_panel;
    state.show_stats_panel = s.show_stats_panel;
    state.show_hierarchy_panel = s.show_hierarchy_panel;
    state.show_inspector_panel = s.show_inspector_panel;
    state.show_audio_panel = s.show_audio_panel;
    state.left_panel_width = s.left_panel_width;
    state.right_panel_width = s.right_panel_width;
    state.initialized = true;
}

// ============================================================
// Оверлеи
// ============================================================

fn draw_box_select_overlay(ctx: &egui::Context, editor: &EditorState) {
    let Some(bs) = editor.box_select else { return; };
    let ppp = ctx.pixels_per_point();
    let rect = egui::Rect::from_two_pos(
        egui::pos2(bs.start.0 / ppp, bs.start.1 / ppp),
        egui::pos2(bs.current.0 / ppp, bs.current.1 / ppp),
    );
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground, egui::Id::new("box_select_layer"),
    ));
    painter.rect_filled(rect, egui::CornerRadius::ZERO,
        egui::Color32::from_rgba_unmultiplied(90, 180, 255, 32));
    painter.rect_stroke(rect, egui::CornerRadius::ZERO,
        egui::Stroke::new(1.5_f32, egui::Color32::from_rgba_unmultiplied(120, 200, 255, 230)),
        egui::StrokeKind::Inside);
}

fn draw_pause_overlay(
    ctx: &egui::Context,
    _editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground, egui::Id::new("pause_dim_layer"),
    ));
    painter.rect_filled(screen, egui::CornerRadius::ZERO,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160));

    egui::Area::new(egui::Id::new("pause_overlay"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(32, 24))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new("PAUSED")
                                .heading()
                                .strong()
                                .color(egui::Color32::from_rgb(230, 200, 130)),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Esc — resume · F9 — stop")
                                .small()
                                .weak(),
                        );
                        ui.add_space(16.0);
                        if ui.button("▶ Resume (Esc)").clicked() {
                            *action = Some(EditorAction::TogglePlay);
                        }
                        ui.add_space(6.0);
                        if ui.button("■ Stop (F9)").clicked() {
                            *action = Some(EditorAction::TogglePlay);
                        }
                    });
                });
        });
}

fn draw_context_menu(ctx: &egui::Context, editor: &mut EditorState,
    _world: &mut World, action: &mut Option<EditorAction>) {
    let Some(pos) = editor.context_menu_pos else { return; };
    let ppp = ctx.pixels_per_point();
    let p = egui::pos2(pos.0 / ppp, pos.1 / ppp);
    let area = egui::Area::new(egui::Id::new("viewport_ctx_menu"))
        .fixed_pos(p).order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(200.0);
                let has_sel = !editor.selected.is_empty();
                if ui.add_enabled(has_sel, egui::Button::new("Focus (F)")).clicked() {
                    *action = Some(EditorAction::FocusSelected); editor.context_menu_pos = None;
                }
                if ui.add_enabled(has_sel, egui::Button::new("Duplicate (Ctrl+D)")).clicked() {
                    *action = Some(EditorAction::Duplicate); editor.context_menu_pos = None;
                }
                if ui.add_enabled(has_sel, egui::Button::new("Delete")).clicked() {
                    *action = Some(EditorAction::DeleteSelected); editor.context_menu_pos = None;
                }
                ui.separator();
                if ui.add_enabled(has_sel, egui::Button::new("Copy (Ctrl+C)")).clicked() {
                    *action = Some(EditorAction::CopyEntity); editor.context_menu_pos = None;
                }
                if ui.add_enabled(!editor.clipboard_entities.is_empty(), egui::Button::new("Paste (Ctrl+V)")).clicked() {
                    *action = Some(EditorAction::PasteEntity); editor.context_menu_pos = None;
                }
                ui.separator();
                if ui.add_enabled(has_sel, egui::Button::new("Make Material Unique")).clicked() {
                    *action = Some(EditorAction::MakeMaterialUnique); editor.context_menu_pos = None;
                }
                ui.separator();
                if ui.add_enabled(has_sel, egui::Button::new("Export FBX (selection)")).clicked() {
                    *action = Some(EditorAction::ExportFbxSelected); editor.context_menu_pos = None;
                }
                if ui.button("Import FBX…").clicked() {
                    *action = Some(EditorAction::ImportFbx); editor.context_menu_pos = None;
                }
            });
        });
    let menu_rect = area.response.rect;
    let clicked_outside = ctx.input(|i| {
        let primary = i.pointer.primary_clicked();
        let pos = i.pointer.interact_pos();
        match (primary, pos) { (true, Some(p)) => !menu_rect.contains(p), _ => false }
    });
    let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
    if clicked_outside || esc { editor.context_menu_pos = None; }
}

// ============================================================
// Верхняя панель
// ============================================================

fn draw_top_bar(ctx: &egui::Context, state: &mut UiState, editor: &mut EditorState,
    stats: &Stats, action: &mut Option<EditorAction>) {
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Rust Engine 3D");
            if ui.button("⌘").on_hover_text("Command palette (Ctrl+P)").clicked() {
                state.command_palette_open = true;
                state.command_palette_query.clear();
                state.command_palette_selected = 0;
            }
            ui.separator();
            if ui.add_enabled(editor.undo.can_undo(), egui::Button::new("↶ Undo")).on_hover_text("Ctrl+Z").clicked() {
                *action = Some(EditorAction::Undo);
            }
            if ui.add_enabled(editor.undo.can_redo(), egui::Button::new("↷ Redo")).on_hover_text("Ctrl+Y").clicked() {
                *action = Some(EditorAction::Redo);
            }
            ui.separator();
            if editor.play.active {
                if editor.play.paused {
                    if ui.button("▶ Resume").on_hover_text("Esc").clicked() {
                        *action = Some(EditorAction::TogglePlay);
                    }
                    if ui.button("■ Stop").on_hover_text("F9").clicked() {
                        *action = Some(EditorAction::TogglePlay);
                    }
                } else {
                    let _ = ui.button("⏸ Pause (Esc)");
                    if ui.button("■ Stop").on_hover_text("F9").clicked() {
                        *action = Some(EditorAction::TogglePlay);
                    }
                }
            } else if ui.button("▶ Play").on_hover_text("Run game (F9)").clicked() {
                *action = Some(EditorAction::TogglePlay);
            }
            ui.separator();
            if !editor.play.active {
                let m = &mut editor.gizmo.mode;
                if ui.selectable_label(*m == GizmoMode::Translate, "T").on_hover_text("Translate (1)").clicked() { *m = GizmoMode::Translate; }
                if ui.selectable_label(*m == GizmoMode::Rotate, "R").on_hover_text("Rotate (2)").clicked() { *m = GizmoMode::Rotate; }
                if ui.selectable_label(*m == GizmoMode::Scale, "S").on_hover_text("Scale (3)").clicked() { *m = GizmoMode::Scale; }
                ui.toggle_value(&mut editor.gizmo.snap_enabled, "Snap").on_hover_text("Snap без Ctrl (0.5 м / 15°)");
                ui.separator();
            }
            if ui.add_enabled(!editor.selected.is_empty(), egui::Button::new("Copy (Ctrl+C)")).clicked() {
                *action = Some(EditorAction::CopyEntity);
            }
            if ui.add_enabled(!editor.clipboard_entities.is_empty(), egui::Button::new("Paste (Ctrl+V)")).clicked() {
                *action = Some(EditorAction::PasteEntity);
            }
            ui.separator();
            if ui.button("New").on_hover_text("New empty scene").clicked() { *action = Some(EditorAction::NewScene); }
            if ui.button("Save").clicked() { *action = Some(EditorAction::Save); }
            if ui.button("Load").clicked() { *action = Some(EditorAction::Load); }
            ui.add(egui::TextEdit::singleline(&mut editor.save_path).desired_width(140.0).hint_text("scene.ron"));

            let recents = editor.settings.recent_scenes.clone();
            ui.menu_button("⏷", |ui| {
                if recents.is_empty() {
                    ui.label(egui::RichText::new("no recent scenes").weak().italics());
                } else {
                    for p in &recents {
                        if ui.button(p).clicked() {
                            *action = Some(EditorAction::LoadPath(p.clone())); ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Clear list").clicked() {
                        editor.settings.recent_scenes.clear(); ui.close();
                    }
                }
            }).response.on_hover_text("Recent scenes");

            ui.separator();
            if ui.button("FBX All").on_hover_text("Экспорт всей сцены в .fbx").clicked() {
                *action = Some(EditorAction::ExportFbxAll);
            }
            if ui.add_enabled(!editor.selected.is_empty(), egui::Button::new("FBX Sel")).on_hover_text("Экспорт выделения в .fbx").clicked() {
                *action = Some(EditorAction::ExportFbxSelected);
            }
            if ui.button("FBX Import").on_hover_text("Импорт ASCII или binary FBX").clicked() {
                *action = Some(EditorAction::ImportFbx);
            }
            ui.separator();
            ui.label(format!("FPS: {:.1}", stats.fps));
            ui.separator();
            ui.label(format!("Entities: {}", stats.entities));
            if editor.play.active {
                ui.separator();
                if editor.play.paused {
                    ui.label(egui::RichText::new("⏸ PAUSED").strong().color(egui::Color32::from_rgb(230, 200, 130)));
                } else {
                    ui.label(egui::RichText::new("● PLAY").strong().color(egui::Color32::from_rgb(230, 90, 90)));
                }
            } else if editor.flying {
                ui.separator();
                ui.label(egui::RichText::new("✦ FLY").strong().color(egui::Color32::from_rgb(90, 180, 230)));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut state.show_renderer_panel, "Renderer");
                ui.toggle_value(&mut state.show_inspector_panel, "Inspector");
                ui.toggle_value(&mut state.show_hierarchy_panel, "Hierarchy");
                ui.toggle_value(&mut state.show_stats_panel, "Stats");
                ui.toggle_value(&mut state.show_bookmarks_panel, "📷");
                ui.toggle_value(&mut state.show_audio_panel, "🔊");
                ui.toggle_value(&mut state.show_content_browser, "📁")
                    .on_hover_text("Content Browser");
            });
        });
    });
}

// ============================================================
// Аудио-панель
// ============================================================

fn draw_audio_panel(
    ctx: &egui::Context,
    state: &mut UiState,
    audio: &AudioSnapshot,
    action: &mut Option<EditorAction>,
) {
    if !state.show_audio_panel { return; }
    let mut open = state.show_audio_panel;

    egui::Window::new("🔊 Audio")
        .open(&mut open)
        .default_pos(egui::pos2(320.0, 380.0))
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            if !audio.available {
                ui.label(
                    egui::RichText::new("audio unavailable")
                        .italics()
                        .color(egui::Color32::from_rgb(230, 130, 130)),
                );
                ui.label(
                    egui::RichText::new(
                        "Could not open the default output device. \
                         Check your system audio settings.",
                    )
                    .small()
                    .weak(),
                );
                return;
            }

            ui.label(egui::RichText::new("Buses").strong());
            ui.label(
                egui::RichText::new("Master applies on top of the others.")
                    .small()
                    .weak()
                    .italics(),
            );
            ui.add_space(2.0);

            for (bus, vol) in &audio.bus_volumes {
                let mut v = *vol;
                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(format!("{:<6}", bus.name()))
                            .monospace(),
                    );
                    let r = ui.add(
                        egui::Slider::new(&mut v, 0.0..=1.0)
                            .show_value(true)
                            .fixed_decimals(2),
                    );
                    if r.changed() {
                        *action = Some(EditorAction::SetBusVolume(*bus, v));
                    }
                    if ui
                        .small_button("↺")
                        .on_hover_text(format!("Reset to {:.2}", bus.default_volume()))
                        .clicked()
                    {
                        *action = Some(EditorAction::SetBusVolume(*bus, bus.default_volume()));
                    }
                });
            }

            ui.separator();

            ui.horizontal(|ui| {
                if ui
                    .button("➕ Load sound…")
                    .on_hover_text("wav / ogg / flac / mp3")
                    .clicked()
                {
                    *action = Some(EditorAction::LoadSound);
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("active: {}", audio.active_count))
                        .small()
                        .weak(),
                );
            });

            ui.separator();
            ui.label(format!("{} sounds loaded", audio.sound_names.len()));

            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if audio.sound_names.is_empty() {
                        ui.label(
                            egui::RichText::new("(no sounds)")
                                .weak()
                                .italics(),
                        );
                        return;
                    }
                    for name in &audio.sound_names {
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("▶")
                                .on_hover_text("Preview on SFX bus")
                                .clicked()
                            {
                                *action = Some(EditorAction::PreviewSound(name.clone()));
                            }
                            ui.label(
                                egui::RichText::new(name).monospace(),
                            );
                        });
                    }
                });

            ui.separator();
            ui.label(
                egui::RichText::new(
                    "Hint: select an entity with AudioSource to edit it."
                )
                .small()
                .weak()
                .italics(),
            );
        });

    state.show_audio_panel = open;
}

// ============================================================
// Палитра
// ============================================================

fn draw_palette_bar(ctx: &egui::Context, editor: &mut EditorState) {
    egui::TopBottomPanel::top("palette_bar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("🖌 Brush:").strong());
            for &item in PaletteItem::all_primitives() { brush_button(ui, &mut editor.palette, item); }

            ui.separator();
            ui.label("💡 Lights:");
            for &item in PaletteItem::all_lights() { brush_button(ui, &mut editor.palette, item); }

            ui.separator();
            ui.label("🎨 Decals:");
            for &item in PaletteItem::all_decals() { brush_button(ui, &mut editor.palette, item); }

            ui.separator();
            ui.label("Presets:");
            for &item in PaletteItem::all_presets() { brush_button(ui, &mut editor.palette, item); }

            ui.separator();

            if editor.palette.active.is_some() {
                let is_light = editor.palette.active.map(|i| i.is_light()).unwrap_or(false);
                if ui.button("✖ Clear (Esc)").on_hover_text("Снять кисть").clicked() {
                    editor.palette.active = None;
                }
                ui.checkbox(&mut editor.palette.keep_active, "Keep");
                if !is_light {
                    ui.checkbox(&mut editor.palette.snap_to_grid, "Snap").on_hover_text("Ctrl+клик форсирует snap");
                    ui.add(egui::DragValue::new(&mut editor.palette.grid_step)
                        .speed(0.01).range(0.05..=10.0).prefix("step: "));
                }
            } else {
                ui.label(egui::RichText::new("no brush").weak().italics());
                ui.label(egui::RichText::new("— выбери примитив/свет и кликай по земле").weak().small());
            }
        });
    });
}

// ============================================================
// Левая панель
// ============================================================

fn draw_left_panel(ctx: &egui::Context, state: &mut UiState, editor: &mut EditorState,
    world: &mut World, assets: &UiAssets<'_>, stats: &Stats, action: &mut Option<EditorAction>) {
    if !(state.show_stats_panel || state.show_hierarchy_panel) { return; }
    let panel_response = egui::SidePanel::left("left_panel")
        .default_width(state.left_panel_width).resizable(true)
        .show(ctx, |ui| {
            if state.show_stats_panel { draw_stats_section(ui, stats); }
            if state.show_hierarchy_panel {
                ui.separator();
                draw_hierarchy_section(ui, editor, world, action);
                ui.separator();
                draw_prefabs_section(ui, editor, action);
                ui.separator();
                draw_assets_section(ui, assets, action);
            }
        });
    state.left_panel_width = panel_response.response.rect.width();
}

fn draw_stats_section(ui: &mut egui::Ui, stats: &Stats) {
    ui.heading("Stats"); ui.separator();
    ui.label(format!("FPS: {:.1}", stats.fps));
    ui.label(format!("Frame: {:.2} ms", if stats.fps > 0.1 { 1000.0 / stats.fps } else { 0.0 }));
    ui.label(format!("Frame max: {:.2} ms", stats.frame_time_max_ms));
    ui.label(format!("Hitches: {}", stats.hitches));
    ui.separator();
    ui.label(format!("Entities: {}", stats.entities));
    ui.label(format!("Draws: {}", stats.draws));
    ui.label(format!("Instances: {}", stats.instances));
    ui.separator();
    ui.label(format!("Dir lights: {}", stats.dir_lights));
    ui.label(format!("Point lights: {}", stats.point_lights));
    ui.separator();

    ui.label("Selection");
    ui.label(format!("Entities: {}", stats.sel_entities));
    ui.label(format!("Triangles (LOD0): {}", stats.sel_triangles));
    ui.label(format!("Vertices (LOD0): {}", stats.sel_vertices));
    ui.separator();
    ui.label("LOD");
    ui.label(format!("LOD0: {} · LOD1: {} · LOD2: {} · LOD3: {}",
        stats.lod_counts[0], stats.lod_counts[1], stats.lod_counts[2], stats.lod_counts[3]));
    if stats.lod_triangles.iter().any(|&t| t > 0) {
        ui.label(format!("Tris: {} / {} / {} / {}",
            stats.lod_triangles[0], stats.lod_triangles[1],
            stats.lod_triangles[2], stats.lod_triangles[3]));
    }
}

// ============================================================
// Дерево иерархии (editor sprint-1)
// ============================================================

struct HierarchySnapshot {
    children_of: HashMap<Entity, Vec<Entity>>,
    roots: Vec<Entity>,
    visible: HashSet<Entity>,
    names: HashMap<Entity, String>,
    filter_active: bool,
}

#[derive(Clone, Copy)]
struct HierarchyRow {
    entity: Entity,
    depth: usize,
}

impl HierarchySnapshot {
    fn build(world: &World, filter: &str) -> Self {
        let all: Vec<Entity> = world.entities().to_vec();
        let all_set: HashSet<Entity> = all.iter().copied().collect();

        let filter_active = !filter.trim().is_empty();
        let filter_lower = filter.to_lowercase();

        let mut names: HashMap<Entity, String> = HashMap::with_capacity(all.len());
        let mut visible: HashSet<Entity> = HashSet::with_capacity(all.len());

        for &e in &all {
            let name = world
                .get::<Name>(e)
                .map(|n| n.0.clone())
                .unwrap_or_else(|| {
                    let mesh = world
                        .get::<MeshHandle>(e)
                        .map(|m| m.0.clone())
                        .unwrap_or_else(|| "?".to_string());
                    format!("#{} {}", e, mesh)
                });

            if !filter_active || name.to_lowercase().contains(&filter_lower) {
                visible.insert(e);
            }
            names.insert(e, name);
        }

        let mut children_of: HashMap<Entity, Vec<Entity>> = HashMap::new();
        let mut has_parent: HashSet<Entity> = HashSet::new();

        for &e in &all {
            if let Some(Parent(p)) = world.get::<Parent>(e).copied() {
                if all_set.contains(&p) && p != e {
                    children_of.entry(p).or_default().push(e);
                    has_parent.insert(e);
                }
            }
        }

        let mut roots: Vec<Entity> = all
            .iter()
            .copied()
            .filter(|e| !has_parent.contains(e))
            .collect();
        roots.sort();

        for kids in children_of.values_mut() {
            kids.sort_by(|a, b| {
                let na = names.get(a).cloned().unwrap_or_default();
                let nb = names.get(b).cloned().unwrap_or_default();
                na.cmp(&nb)
            });
        }

        if filter_active {
            let snapshot: Vec<Entity> = visible.iter().copied().collect();
            for mut e in snapshot {
                for _ in 0..64 {
                    match world.get::<Parent>(e).copied() {
                        Some(Parent(p)) if all_set.contains(&p) => {
                            visible.insert(p);
                            e = p;
                        }
                        _ => break,
                    }
                }
            }
        }

        Self { children_of, roots, visible, names, filter_active }
    }

    fn children_of(&self, e: Entity) -> &[Entity] {
        self.children_of.get(&e).map(|v| v.as_slice()).unwrap_or(&[])
    }

    fn is_visible(&self, e: Entity) -> bool {
        !self.filter_active || self.visible.contains(&e)
    }

    fn visible_count(&self) -> usize {
        self.flatten(|_| true).len()
    }

    fn flatten(&self, is_expanded: impl Fn(Entity) -> bool) -> Vec<HierarchyRow> {
        let mut out = Vec::new();
        for &root in &self.roots {
            if !self.is_visible(root) {
                continue;
            }
            self.flatten_rec(root, 0, &is_expanded, &mut out);
        }
        out
    }

    fn flatten_rec(
        &self,
        e: Entity,
        depth: usize,
        is_expanded: &impl Fn(Entity) -> bool,
        out: &mut Vec<HierarchyRow>,
    ) {
        out.push(HierarchyRow { entity: e, depth });
        if !is_expanded(e) {
            return;
        }
        for &c in self.children_of(e) {
            if self.is_visible(c) {
                self.flatten_rec(c, depth + 1, is_expanded, out);
            }
        }
    }

    fn flat_order(&self) -> Vec<Entity> {
        self.flatten(|_| true).into_iter().map(|r| r.entity).collect()
    }
}

fn draw_hierarchy_section(ui: &mut egui::Ui, editor: &mut EditorState,
    world: &mut World, action: &mut Option<EditorAction>) {
    ui.heading("Hierarchy"); ui.separator();

    ui.horizontal(|ui| {
        if ui.button("+ Cube").clicked() { *action = Some(EditorAction::AddCube); }
        if ui.button("+ Sphere").clicked() { *action = Some(EditorAction::AddSphere); }
    });
    ui.horizontal(|ui| {
        if ui.add_enabled(!editor.selected.is_empty(), egui::Button::new("Duplicate (Ctrl+D)")).clicked() {
            *action = Some(EditorAction::Duplicate);
        }
        if ui.add_enabled(!editor.selected.is_empty(), egui::Button::new("Delete")).clicked() {
            *action = Some(EditorAction::DeleteSelected);
        }
    });

    ui.horizontal(|ui| {
        if ui.small_button("⤢ Expand").on_hover_text("Раскрыть всё дерево").clicked() {
            for &e in world.entities() {
                editor.hierarchy_expanded.insert(e);
            }
        }
        if ui.small_button("⤡ Collapse").on_hover_text("Свернуть всё дерево").clicked() {
            editor.hierarchy_expanded.clear();
        }
        if !editor.selected.is_empty() {
            if ui.small_button("→ Selected").on_hover_text("Развернуть родителей выбранного").clicked() {
                let selected: Vec<Entity> = editor.selected.clone();
                for e in selected {
                    let mut cur = e;
                    for _ in 0..64 {
                        editor.hierarchy_expanded.insert(cur);
                        match world.get::<Parent>(cur).copied() {
                            Some(Parent(p)) => cur = p,
                            None => break,
                        }
                    }
                }
            }
        }
    });

    ui.separator();
    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(egui::TextEdit::singleline(&mut editor.search_filter).desired_width(120.0).hint_text("name…"));
        if !editor.search_filter.is_empty() && ui.small_button("✖").clicked() { editor.search_filter.clear(); }
    });

    let snapshot = HierarchySnapshot::build(world, &editor.search_filter);

    let total = snapshot.visible_count();
    let sel_count = editor.selected.len();
    if sel_count > 1 { ui.label(format!("{} shown · {} selected", total, sel_count)); }
    else { ui.label(format!("{} shown", total)); }

    let anchor = editor.primary();

    egui::ScrollArea::vertical().auto_shrink([false; 2]).max_height(340.0)
        .show(ui, |ui| {
            let rows = snapshot.flatten(
                |e| editor.hierarchy_expanded.contains(&e),
            );
            for row in rows {
                draw_hierarchy_row(
                    ui, editor, world, row.entity, &snapshot, anchor, action, row.depth,
                );
            }
        });
}

fn draw_hierarchy_row(ui: &mut egui::Ui, editor: &mut EditorState, world: &mut World,
    e: Entity, snapshot: &HierarchySnapshot, anchor: Option<Entity>,
    action: &mut Option<EditorAction>, depth: usize) {
    let is_selected = editor.is_selected(e);
    let is_locked = editor.is_locked(world, e);
    let has_children = !snapshot.children_of(e).is_empty();
    let is_expanded = editor.hierarchy_expanded.contains(&e);

    ui.horizontal(|ui| {
        if depth > 0 {
            ui.add_space(depth as f32 * 14.0);
        }

        if has_children {
            let label = if is_expanded { "▼" } else { "▶" };
            if ui.add(egui::Button::new(label).frame(false).small()).clicked() {
                if is_expanded {
                    editor.hierarchy_expanded.remove(&e);
                } else {
                    editor.hierarchy_expanded.insert(e);
                }
            }
        } else {
            ui.add_space(16.0);
        }

        let solo_active = editor.solo == Some(e);
        let solo_icon = if solo_active { "◉" } else { "○" };
        let solo_resp = ui
            .add(egui::Button::new(solo_icon).frame(false).small())
            .on_hover_text("Solo: показать только это и потомков");
        if solo_resp.clicked() {
            editor.solo = if solo_active { None } else { Some(e) };
        }

        let lock_icon = if is_locked { "🔒" } else { "🔓" };
        let lock_resp = ui
            .add(egui::Button::new(lock_icon).frame(false).small())
            .on_hover_text("Lock: запретить выбор и перемещение");
        if lock_resp.clicked() {
            if is_locked {
                editor.locked.remove(&e);
            } else {
                editor.locked.insert(e);
            }
        }

        let visible = world.get::<Visible>(e).map(|v| v.0).unwrap_or(true);
        let eye_label = if visible { "👁" } else { "✖" };
        let eye_resp = ui.add(egui::Button::new(eye_label).frame(false).small())
            .on_hover_text("Toggle visibility");
        if eye_resp.clicked() {
            editor.undo_requested = true;
            if visible { world.insert(e, Visible(false)); } else { world.remove::<Visible>(e); }
        }

        if editor.renaming == Some(e) {
            draw_hierarchy_rename(ui, editor, world, e);
            return;
        }

        let label = snapshot.names.get(&e).cloned()
            .unwrap_or_else(|| entity_display_name(world, e));
        let badges = component_badges(world, e);

        let text_color = if is_locked {
            ui.visuals().weak_text_color()
        } else {
            ui.visuals().text_color()
        };

        let drag_id = egui::Id::new(("hier_drag", e));
        let drag_inner = ui.dnd_drag_source(
            drag_id,
            e,
            |ui| {
                let resp = ui.add(
                    egui::Label::new(
                        egui::RichText::new(&label).color(text_color),
                    )
                    .sense(egui::Sense::click_and_drag()),
                );
                if !badges.is_empty() {
                    ui.add_space(2.0);
                    ui.label(egui::RichText::new(&badges).small().weak().monospace());
                }
                resp
            },
        );
        let drag_resp = drag_inner.response;

        if let Some(payload) = drag_resp.dnd_release_payload::<Entity>() {
            let dragged = *payload;
            if dragged != e && !would_create_cycle(world, dragged, e) {
                editor.undo_requested = true;
                world.insert(dragged, Parent(e));
                editor.hierarchy_expanded.insert(e);
                log::info!("Reparented #{} → parent #{}", dragged, e);
            }
        }

        let clicked = drag_resp.clicked();
        let double_clicked = drag_resp.double_clicked();

        if is_selected {
            let rect = drag_resp.rect.expand2(egui::vec2(2.0, 1.0));
            ui.painter().rect_filled(
                rect,
                egui::CornerRadius::same(2),
                ui.visuals().selection.bg_fill.linear_multiply(0.5),
            );
        }

        if clicked {
            if is_locked {
                editor.hud_hint("Entity is locked (unlock 🔒 to select)");
            } else {
                let modifiers = ui.input(|i| i.modifiers);
                if modifiers.ctrl || modifiers.command {
                    editor.toggle_select(e);
                } else if modifiers.shift {
                    let flat = snapshot.flat_order();
                    editor.select_range(&flat, anchor.unwrap_or(e), e);
                } else {
                    editor.select_single(e);
                }
            }
        }

        if double_clicked && !is_locked {
            editor.select_single(e);
            editor.renaming = Some(e);
            editor.rename_buffer = snapshot.names.get(&e).cloned()
                .unwrap_or_else(|| entity_display_name(world, e));
        }

        drag_resp.context_menu(|ui| {
            hierarchy_context_menu(ui, editor, world, e, action);
        });
    });
}

fn draw_hierarchy_rename(ui: &mut egui::Ui, editor: &mut EditorState, world: &mut World, e: Entity) {
    let response = ui.add(egui::TextEdit::singleline(&mut editor.rename_buffer).desired_width(160.0));
    response.request_focus();
    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
    if enter || response.lost_focus() {
        let new_name = editor.rename_buffer.trim().to_string();
        if !new_name.is_empty() {
            editor.undo_requested = true;
            world.insert(e, Name(new_name));
        }
        editor.renaming = None;
        editor.rename_buffer.clear();
    } else if esc {
        editor.renaming = None;
        editor.rename_buffer.clear();
    }
}

fn hierarchy_context_menu(ui: &mut egui::Ui, editor: &mut EditorState, world: &mut World,
    e: Entity, action: &mut Option<EditorAction>) {
    if ui.button("Rename").clicked() {
        editor.renaming = Some(e);
        editor.rename_buffer = world.get::<Name>(e).map(|n| n.0.clone())
            .unwrap_or_else(|| entity_display_name(world, e));
        ui.close();
    }
    if ui.button("Focus (F)").clicked() {
        editor.select_single(e); *action = Some(EditorAction::FocusSelected); ui.close();
    }
    if ui.button("Duplicate").clicked() {
        editor.select_single(e); *action = Some(EditorAction::Duplicate); ui.close();
    }
    if ui.button("Delete").clicked() {
        editor.select_single(e); *action = Some(EditorAction::DeleteSelected); ui.close();
    }
    ui.separator();
    if ui.button("Lock / Unlock").clicked() {
        if editor.locked.contains(&e) {
            editor.locked.remove(&e);
        } else {
            editor.locked.insert(e);
        }
        ui.close();
    }
    if editor.solo == Some(e) {
        if ui.button("Unsolo").clicked() {
            editor.solo = None;
            ui.close();
        }
    } else if ui.button("Solo").clicked() {
        editor.solo = Some(e);
        ui.close();
    }
    ui.separator();
    if ui.button("Export FBX…").clicked() {
        editor.select_single(e); *action = Some(EditorAction::ExportFbxSelected); ui.close();
    }
    ui.separator();
    if world.has::<Parent>(e) && ui.button("Clear Parent").clicked() {
        editor.undo_requested = true; world.remove::<Parent>(e); ui.close();
    }
}

fn draw_prefabs_section(ui: &mut egui::Ui, editor: &mut EditorState, action: &mut Option<EditorAction>) {
    ui.heading("Prefabs"); ui.separator();
    ui.horizontal(|ui| {
        ui.label("📁");
        ui.add(egui::TextEdit::singleline(&mut editor.prefabs_dir).desired_width(140.0).hint_text("prefabs"));
        if ui.small_button("⟳").on_hover_text("Refresh list").clicked() { *action = Some(EditorAction::RefreshPrefabs); }
    });
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.add(egui::TextEdit::singleline(&mut editor.prefab_save_name).desired_width(120.0).hint_text("my_prefab"));
        let can_save = !editor.selected.is_empty() && !editor.prefab_save_name.trim().is_empty();
        if ui.add_enabled(can_save, egui::Button::new("💾 Save Sel")).on_hover_text("Сохранить выделение как .prefab.ron").clicked() {
            *action = Some(EditorAction::SavePrefab);
        }
    });
    ui.separator();
    let prefab_count = editor.prefab_list.len();
    if prefab_count == 0 {
        ui.label(egui::RichText::new("no prefabs found").weak().italics());
        ui.label(egui::RichText::new("Выдели объекты → введи имя → Save Sel").small().weak());
    } else {
        ui.label(format!("{} files", prefab_count));
        egui::ScrollArea::vertical().auto_shrink([false; 2]).max_height(200.0).show(ui, |ui| {
            for (idx, path) in editor.prefab_list.iter().enumerate() {
                let display = crate::scene::prefab::prefab_display_name(path);
                let resp = ui.button(format!("📦 {}", display)).on_hover_text(format!("Spawn {}", path.display()));
                if resp.clicked() { *action = Some(EditorAction::InstantiatePrefab(idx as u32)); }
                resp.context_menu(|ui| {
                    if ui.button("Spawn here").clicked() {
                        *action = Some(EditorAction::InstantiatePrefab(idx as u32)); ui.close();
                    }
                    if ui.button("Show path").clicked() { log::info!("{}", path.display()); ui.close(); }
                });
            }
        });
    }
}

fn draw_assets_section(ui: &mut egui::Ui, assets: &UiAssets<'_>, action: &mut Option<EditorAction>) {
    ui.heading("Assets"); ui.separator();

    ui.horizontal_wrapped(|ui| {
        if ui
            .button("➕ Load sRGB (color)…")
            .on_hover_text(
                "Albedo / base color / emissive.\n\
                 GPU decodes the stored sRGB bytes to linear at sample time.",
            )
            .clicked()
        {
            *action = Some(EditorAction::LoadTextures);
        }
        if ui
            .button("➕ Load Linear (data)…")
            .on_hover_text(
                "Normal maps, metallic-roughness, AO, height/displacement, masks.\n\
                 No sRGB decode — bytes are used as-is.",
            )
            .clicked()
        {
            *action = Some(EditorAction::LoadTexturesLinear);
        }
    });
    ui.label(
        egui::RichText::new("sRGB for color, Linear for data. Wrong choice = wrong shading.")
            .small()
            .weak()
            .italics(),
    );

    ui.separator();
    let tex_count = assets.texture_list.len();
    if tex_count == 0 {
        ui.label(egui::RichText::new("no textures loaded").weak().italics());
        return;
    }
    ui.label(format!("{} textures", tex_count));

    egui::ScrollArea::vertical().auto_shrink([false; 2]).max_height(180.0).show(ui, |ui| {
        for (name, w, h, is_srgb) in assets.texture_list {
            let tag = if *is_srgb { "[sRGB]" } else { "[Linear]" };
            let tag_color = if *is_srgb {
                egui::Color32::from_rgb(230, 200, 130)
            } else {
                egui::Color32::from_rgb(150, 200, 230)
            };

            let resp = ui
                .horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(tag)
                            .small()
                            .monospace()
                            .color(tag_color),
                    );
                    ui.button(format!("🖼 {}", name))
                })
                .inner
                .on_hover_text(format!(
                    "{} × {} pixels\nLoaded as: {}\nRight-click to remove",
                    w, h,
                    if *is_srgb { "sRGB (color)" } else { "Linear (data)" },
                ));
            resp.context_menu(|ui| {
                if ui.button("Remove texture").clicked() {
                    *action = Some(EditorAction::RemoveTexture(name.clone()));
                    ui.close();
                }
                if ui.button("Copy name").clicked() {
                    let text = name.clone();
                    ui.output_mut(|o| {
                        o.commands.push(egui::OutputCommand::CopyText(text));
                    });
                    ui.close();
                }
            });
        }
    });
}

// ============================================================
// Правая панель
// ============================================================

fn draw_right_panel(ctx: &egui::Context, state: &mut UiState, editor: &mut EditorState,
    world: &mut World, assets: &UiAssets<'_>, postfx: &mut PostFx, action: &mut Option<EditorAction>) {
    if !(state.show_inspector_panel || state.show_renderer_panel) { return; }
    let panel_response = egui::SidePanel::right("right_panel")
        .default_width(state.right_panel_width).resizable(true)
        .show(ctx, |ui| {
            if state.show_inspector_panel { draw_inspector_panel(ui, editor, world, assets, action); }
            if state.show_renderer_panel {
                ui.separator();
                egui::CollapsingHeader::new("Renderer").default_open(true)
                    .show(ui, |ui| { draw_renderer_panel(ui, postfx, editor, action); });
            }
        });
    state.right_panel_width = panel_response.response.rect.width();
}

fn draw_inspector_panel(ui: &mut egui::Ui, editor: &mut EditorState, world: &mut World,
    assets: &UiAssets<'_>, action: &mut Option<EditorAction>) {
    ui.heading("Inspector"); ui.separator();
    ui.horizontal(|ui| {
        ui.label("Gizmo:");
        let m = &mut editor.gizmo.mode;
        if ui.selectable_label(*m == GizmoMode::Translate, "T (1)").clicked() { *m = GizmoMode::Translate; }
        if ui.selectable_label(*m == GizmoMode::Rotate, "R (2)").clicked() { *m = GizmoMode::Rotate; }
        if ui.selectable_label(*m == GizmoMode::Scale, "S (3)").clicked() { *m = GizmoMode::Scale; }
        ui.toggle_value(&mut editor.gizmo.snap_enabled, "Snap");
    });
    ui.label(egui::RichText::new("Alt+drag: duplicate · Ctrl: snap").small().weak());
    ui.separator();

    let n = editor.selected.len();
    if n == 0 { ui.label("Nothing selected"); return; }
    if n > 1 {
        ui.label(format!("{} objects selected", n)); ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Focus (F)").clicked() { *action = Some(EditorAction::FocusSelected); }
            if ui.button("Duplicate").clicked() { *action = Some(EditorAction::Duplicate); }
            if ui.button("Delete").clicked() { *action = Some(EditorAction::DeleteSelected); }
            if ui.button("FBX Sel").clicked() { *action = Some(EditorAction::ExportFbxSelected); }
        });
        ui.separator();
        draw_multi_edit(ui, world, editor);
        ui.separator();
        if ui.button("Deselect all").clicked() { editor.selected.clear(); }
        return;
    }

    let e = editor.selected[0];
    if !world.entities().contains(&e) {
        editor.selected.clear();
        ui.label("Selection removed");
        return;
    }
    draw_inspector(ui, world, e, editor, assets, action);
}

// ============================================================
// Bookmarks
// ============================================================

fn draw_bookmarks_panel(ctx: &egui::Context, state: &mut UiState,
    editor: &mut EditorState, action: &mut Option<EditorAction>) {
    if !state.show_bookmarks_panel { return; }
    let mut open = state.show_bookmarks_panel;
    egui::Window::new("📷 Camera Bookmarks").open(&mut open)
        .default_pos(egui::pos2(680.0, 120.0)).default_width(260.0).resizable(true)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("Ctrl+1..9 — save · Alt+1..9 — goto").small().weak());
            ui.separator();
            for i in 0..9 {
                ui.horizontal(|ui| {
                    ui.label(format!("Slot {}:", i + 1));
                    if editor.settings.camera_bookmarks.is_empty(i) {
                        ui.label(egui::RichText::new("(empty)").weak().italics());
                    } else if ui.small_button("↺").on_hover_text("Goto").clicked() {
                        *action = Some(EditorAction::GotoCameraBookmark(i));
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.small_button("💾").on_hover_text("Save here").clicked() {
                            *action = Some(EditorAction::SaveCameraBookmark(i));
                        }
                    });
                });
            }
        });
    state.show_bookmarks_panel = open;
}

// ============================================================
// Command Palette
// ============================================================

#[derive(Clone)]
struct PaletteCommand { label: &'static str, action: EditorAction }

fn palette_commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand { label: "File · Save scene", action: EditorAction::Save },
        PaletteCommand { label: "File · Load scene", action: EditorAction::Load },
        PaletteCommand { label: "File · New scene", action: EditorAction::NewScene },
        PaletteCommand { label: "Audio · Load sound…", action: EditorAction::LoadSound },
        PaletteCommand { label: "AI · Bake navmesh", action: EditorAction::BakeNavmesh },
        PaletteCommand { label: "FBX · Export all", action: EditorAction::ExportFbxAll },
        PaletteCommand { label: "FBX · Export selected", action: EditorAction::ExportFbxSelected },
        PaletteCommand { label: "FBX · Import…", action: EditorAction::ImportFbx },
        PaletteCommand { label: "Textures · Load sRGB (color)…", action: EditorAction::LoadTextures },
        PaletteCommand { label: "Textures · Load Linear (normal/data)…", action: EditorAction::LoadTexturesLinear },
        PaletteCommand { label: "Scene · Add cube", action: EditorAction::AddCube },
        PaletteCommand { label: "Scene · Add sphere", action: EditorAction::AddSphere },
        PaletteCommand { label: "Scene · Duplicate selection", action: EditorAction::Duplicate },
        PaletteCommand { label: "Scene · Delete selection", action: EditorAction::DeleteSelected },
        PaletteCommand { label: "Scene · Select all", action: EditorAction::SelectAll },
        PaletteCommand { label: "Scene · Deselect all", action: EditorAction::DeselectAll },
        PaletteCommand { label: "Scene · Invert selection", action: EditorAction::InvertSelection },
        PaletteCommand { label: "Scene · Cleanup empty entities", action: EditorAction::CleanupEmptyEntities },
        PaletteCommand { label: "Scene · Copy selection", action: EditorAction::CopyEntity },
        PaletteCommand { label: "Scene · Paste", action: EditorAction::PasteEntity },
        PaletteCommand { label: "Camera · Focus on selection", action: EditorAction::FocusSelected },
        PaletteCommand { label: "Camera · Front view", action: EditorAction::CameraPreset(0) },
        PaletteCommand { label: "Camera · Back view", action: EditorAction::CameraPreset(1) },
        PaletteCommand { label: "Camera · Right view", action: EditorAction::CameraPreset(2) },
        PaletteCommand { label: "Camera · Left view", action: EditorAction::CameraPreset(3) },
        PaletteCommand { label: "Camera · Top view", action: EditorAction::CameraPreset(4) },
        PaletteCommand { label: "Camera · Bottom view", action: EditorAction::CameraPreset(5) },
        PaletteCommand { label: "Camera · Isometric view", action: EditorAction::CameraPreset(6) },
        PaletteCommand { label: "Edit · Undo", action: EditorAction::Undo },
        PaletteCommand { label: "Edit · Redo", action: EditorAction::Redo },
        PaletteCommand { label: "Play · Toggle play mode", action: EditorAction::TogglePlay },
        PaletteCommand { label: "Play · Spawn player here", action: EditorAction::SpawnPlayerHere },
        PaletteCommand { label: "Prefab · Save selection…", action: EditorAction::SavePrefab },
        PaletteCommand { label: "Prefab · Refresh list", action: EditorAction::RefreshPrefabs },
    ]
}

fn fuzzy_match(query: &str, label: &str) -> Option<usize> {
    if query.is_empty() { return Some(0); }
    let q = query.to_lowercase();
    let l = label.to_lowercase();
    l.find(&q)
}

fn draw_command_palette(ctx: &egui::Context, state: &mut UiState, action: &mut Option<EditorAction>) {
    let mut close = false;
    egui::Area::new(egui::Id::new("command_palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(ctx.screen_rect().center().x - 250.0, ctx.screen_rect().top() + 100.0))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).inner_margin(egui::Margin::same(12)).show(ui, |ui| {
                ui.set_min_width(500.0); ui.set_max_width(500.0);
                ui.label(egui::RichText::new("Command palette").strong().small());
                let response = ui.add(egui::TextEdit::singleline(&mut state.command_palette_query)
                    .desired_width(f32::INFINITY).hint_text("Type to filter commands…"));
                response.request_focus();
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                let commands = palette_commands();
                let query = state.command_palette_query.clone();
                let mut filtered: Vec<(usize, &PaletteCommand)> = commands.iter()
                    .filter_map(|c| fuzzy_match(&query, c.label).map(|pos| (pos, c))).collect();
                filtered.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(b.1.label)));
                if !filtered.is_empty() {
                    if up { state.command_palette_selected = state.command_palette_selected.saturating_sub(1); }
                    if down { state.command_palette_selected = (state.command_palette_selected + 1).min(filtered.len() - 1); }
                    if state.command_palette_selected >= filtered.len() { state.command_palette_selected = filtered.len() - 1; }
                } else { state.command_palette_selected = 0; }
                if enter && !filtered.is_empty() {
                    let cmd = filtered[state.command_palette_selected].1.clone();
                    *action = Some(cmd.action); close = true;
                }
                if esc { close = true; }
                ui.add_space(6.0);
                egui::ScrollArea::vertical().max_height(320.0).auto_shrink([false; 2]).show(ui, |ui| {
                    if filtered.is_empty() {
                        ui.label(egui::RichText::new("no matching commands").weak().italics());
                    }
                    for (idx, (_, cmd)) in filtered.iter().enumerate() {
                        let selected = idx == state.command_palette_selected;
                        let resp = ui.selectable_label(selected, cmd.label);
                        if resp.clicked() { *action = Some(cmd.action.clone()); close = true; }
                        if resp.hovered() { state.command_palette_selected = idx; }
                    }
                });
            });
        });
    if close {
        state.command_palette_open = false;
        state.command_palette_query.clear();
        state.command_palette_selected = 0;
    }
}

// ============================================================
// Inspector
// ============================================================

fn draw_inspector(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState,
    assets: &UiAssets<'_>, action: &mut Option<EditorAction>) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("#{}", e)).weak().monospace());
        if let Some(n) = world.get_mut::<Name>(e) {
            let r = ui.add(egui::TextEdit::singleline(&mut n.0).desired_width(160.0));
            if r.gained_focus() { editor.undo_requested = true; }
        } else if ui.button("+ Name").clicked() {
            editor.undo_requested = true;
            world.insert(e, Name(format!("Entity_{}", e)));
        }
        if ui.button("Focus (F)").clicked() { *action = Some(EditorAction::FocusSelected); }
        if ui.button("Spawn Player Here").clicked() { *action = Some(EditorAction::SpawnPlayerHere); }
        if ui.button("Export FBX").clicked() { *action = Some(EditorAction::ExportFbxSelected); }
    });
    ui.horizontal(|ui| {
        let mut visible = world.get::<Visible>(e).map(|v| v.0).unwrap_or(true);
        if ui.checkbox(&mut visible, "Visible").changed() {
            editor.undo_requested = true;
            if visible { world.remove::<Visible>(e); } else { world.insert(e, Visible(false)); }
        }
    });
    draw_component_chips(ui, world, e, editor);
    ui.separator();

    if world.has::<Transform>(e) { inspector_transform(ui, world, e, editor); }
    if world.has::<DirectionalLight>(e) { inspector_directional_light(ui, world, e, editor); }
    if world.has::<PointLight>(e) { inspector_point_light(ui, world, e, editor); }
    if world.has::<crate::game::decals::Decal>(e) { inspector_decal(ui, world, e, editor); }
    if world.has::<Parent>(e) || editor.selected.len() >= 2 { inspector_hierarchy(ui, world, e, editor); }
    if world.has::<MeshHandle>(e) || world.has::<MaterialHandle>(e) { inspector_geometry(ui, world, e, assets); }
    if world.has::<TextureTiling>(e) { inspector_texture_tiling(ui, world, e, editor); }
    if world.has::<Elevator>(e) { inspector_elevator(ui, world, e, editor); }
    if world.has::<SlidingDoor>(e) { inspector_sliding_door(ui, world, e, editor); }
    if world.has::<AudioSource>(e) {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new("🔊 AudioSource — см. окно").small().weak().italics());
        });
    }
    if let Some((name, original)) = &assets.selected_material {
        inspector_material(ui, world, e, editor, action, assets, name, original);
    }
    if world.has::<Tint>(e) { inspector_tint(ui, world, e, editor); }
    if world.has::<Health>(e) { inspector_health(ui, world, e); }
    if world.has::<Chase>(e) { inspector_chase(ui, world, e); }
    if world.has::<Spinner>(e) { inspector_spinner(ui, world, e); }
    if world.has::<Velocity>(e) { inspector_velocity(ui, world, e); }
    if world.has::<RigidBody>(e) { inspector_rigidbody(ui, world, e, editor); }
    if world.has::<Collider>(e) { inspector_collider(ui, world, e, editor); }
    if world.has::<PhysicsMaterial>(e) { inspector_physics_material(ui, world, e, editor); }

    ui.separator();
    if ui.button("Deselect").clicked() { editor.selected.clear(); }
}

fn inspector_directional_light(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("☀ Directional Light").default_open(true).show(ui, |ui| {
        ui.label(egui::RichText::new(
            "Направленный свет без затухания. `direction` — вектор ОТ сцены К источнику."
        ).small().weak());
        let Some(light) = world.get_mut::<DirectionalLight>(e) else { return; };
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Sun").clicked() { *light = DirectionalLight::sun(); editor.undo_requested = true; }
            if ui.small_button("Fill").clicked() { *light = DirectionalLight::fill(); editor.undo_requested = true; }
            if ui.small_button("Moon").clicked() {
                *light = DirectionalLight { direction: Vec3::new(-0.3, 0.8, 0.2), color: [0.55, 0.65, 0.95], intensity: 0.4 };
                editor.undo_requested = true;
            }
            if ui.small_button("Sunset").clicked() {
                *light = DirectionalLight { direction: Vec3::new(1.0, 0.15, 0.0), color: [1.0, 0.55, 0.25], intensity: 1.5 };
                editor.undo_requested = true;
            }
        });
        ui.separator();
        let mut dir_arr = light.direction.to_array();
        ui.label("Direction");
        ui.horizontal(|ui| {
            for i in 0..3 {
                let r = ui.add(egui::DragValue::new(&mut dir_arr[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i]));
                if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
            }
        });
        let new_dir = Vec3::from_array(dir_arr);
        if new_dir.length_squared() > 1e-6 { light.direction = new_dir.normalize(); }
        ui.horizontal(|ui| {
            ui.label("Color:");
            if ui.color_edit_button_rgb(&mut light.color).changed() { editor.undo_requested = true; }
        });
        let r = ui.add(egui::Slider::new(&mut light.intensity, 0.0..=5.0).logarithmic(true).text("Intensity"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        ui.label(egui::RichText::new(format!(
            "→ ({:.2}, {:.2}, {:.2}) × {:.2}",
            light.direction.x, light.direction.y, light.direction.z, light.intensity,
        )).small().weak().monospace());
    });
}

fn inspector_point_light(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("💡 Point Light").default_open(true).show(ui, |ui| {
        ui.label(egui::RichText::new("Точечный источник. Позиция берётся из Transform.").small().weak());
        let Some(light) = world.get_mut::<PointLight>(e) else { return; };
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Warm").clicked() { *light = PointLight::new([1.0, 0.6, 0.3], 4.0, 15.0); editor.undo_requested = true; }
            if ui.small_button("Cool").clicked() { *light = PointLight::new([0.4, 0.7, 1.0], 3.0, 12.0); editor.undo_requested = true; }
            if ui.small_button("Red").clicked() { *light = PointLight::new([1.0, 0.15, 0.15], 5.0, 10.0); editor.undo_requested = true; }
            if ui.small_button("Green").clicked() { *light = PointLight::new([0.2, 1.0, 0.3], 5.0, 10.0); editor.undo_requested = true; }
            if ui.small_button("Candle").clicked() { *light = PointLight::new([1.0, 0.55, 0.2], 1.5, 6.0); editor.undo_requested = true; }
        });
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Color:");
            if ui.color_edit_button_rgb(&mut light.color).changed() { editor.undo_requested = true; }
        });
        let r = ui.add(egui::Slider::new(&mut light.intensity, 0.0..=20.0).logarithmic(true).text("Intensity"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut light.range, 0.5..=100.0).logarithmic(true).text("Range (m)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        ui.label(egui::RichText::new("Двигай позицию через Transform (клавиша 1 → Translate).").small().weak());
    });
}

fn inspector_decal(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("🎨 Decal").default_open(true).show(ui, |ui| {
        let Some(dec) = world.get_mut::<crate::game::decals::Decal>(e) else { return; };
        ui.horizontal(|ui| {
            ui.label("Texture:");
            ui.add(egui::TextEdit::singleline(&mut dec.texture).desired_width(140.0).hint_text("blood"));
        });
        ui.label("Tint (RGB + Alpha)");
        if ui.color_edit_button_rgba_unmultiplied(&mut dec.tint).changed() { editor.undo_requested = true; }
        ui.label(egui::RichText::new(
            "Decal — unit cube поверх поверхности. Масштабируй Transform, чтобы растянуть."
        ).small().weak());
        ui.horizontal(|ui| {
            if ui.small_button("Small (0.5m)").clicked() {
                if let Some(t) = world.get_mut::<Transform>(e) { t.scale = Vec3::splat(0.5); }
                editor.undo_requested = true;
            }
            if ui.small_button("Medium (1m)").clicked() {
                if let Some(t) = world.get_mut::<Transform>(e) { t.scale = Vec3::splat(1.0); }
                editor.undo_requested = true;
            }
            if ui.small_button("Large (2m)").clicked() {
                if let Some(t) = world.get_mut::<Transform>(e) { t.scale = Vec3::splat(2.0); }
                editor.undo_requested = true;
            }
        });
    });
}

fn draw_component_chips(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    let mut chips: Vec<(&'static str, ComponentKind)> = Vec::new();
    if world.has::<Transform>(e) { chips.push(("Transform", ComponentKind::Transform)); }
    if world.has::<MeshHandle>(e) { chips.push(("Mesh", ComponentKind::Mesh)); }
    if world.has::<MaterialHandle>(e) { chips.push(("Material", ComponentKind::Material)); }
    if world.has::<SkeletonHandle>(e) { chips.push(("Skeleton", ComponentKind::Skeleton)); }
    if world.has::<AnimationPlayer>(e) { chips.push(("Animation", ComponentKind::Animation)); }
    if world.has::<Spinner>(e) { chips.push(("Spinner", ComponentKind::Spinner)); }
    if world.has::<Velocity>(e) { chips.push(("Velocity", ComponentKind::Velocity)); }
    if world.has::<Parent>(e) { chips.push(("Parent", ComponentKind::Parent)); }
    if world.has::<Health>(e) { chips.push(("Health", ComponentKind::Health)); }
    if world.has::<Chase>(e) { chips.push(("Chase", ComponentKind::Chase)); }
    if world.has::<Interactable>(e) { chips.push(("Interactable", ComponentKind::Interactable)); }
    if world.has::<Trigger>(e) { chips.push(("Trigger", ComponentKind::Trigger)); }
    if world.has::<Tint>(e) { chips.push(("Tint", ComponentKind::Tint)); }
    if world.has::<Visible>(e) { chips.push(("Visible", ComponentKind::Visible)); }
    if world.has::<TextureTiling>(e) { chips.push(("TexTiling", ComponentKind::TextureTiling)); }
    if world.has::<Elevator>(e) { chips.push(("Elevator", ComponentKind::Elevator)); }
    if world.has::<SlidingDoor>(e) { chips.push(("SlidingDoor", ComponentKind::SlidingDoor)); }
    if world.has::<RigidBody>(e) { chips.push(("RigidBody", ComponentKind::RigidBody)); }
    if world.has::<Collider>(e) { chips.push(("Collider", ComponentKind::Collider)); }
    if world.has::<PhysicsMaterial>(e) { chips.push(("PhysicsMaterial", ComponentKind::PhysicsMaterial)); }
    if world.has::<DirectionalLight>(e) { chips.push(("☀ DirLight", ComponentKind::DirectionalLight)); }
    if world.has::<PointLight>(e) { chips.push(("💡 PointLight", ComponentKind::PointLight)); }
    if world.has::<crate::game::decals::Decal>(e) { chips.push(("🎨 Decal", ComponentKind::Decal)); }
    if world.has::<AudioSource>(e) { chips.push(("🔊 Audio", ComponentKind::AudioSource)); }
    if world.has::<crate::game::ai::AiAgent>(e) { chips.push(("🤖 AI", ComponentKind::AiAgent)); }
    if world.has::<crate::game::ai::PatrolPath>(e) { chips.push(("🚶 Patrol", ComponentKind::PatrolPath)); }
    if world.has::<crate::game::ai::Enemy>(e) { chips.push(("👹 Enemy", ComponentKind::Enemy)); }
    if world.has::<crate::game::ai::AiTarget>(e) { chips.push(("🎯 Target", ComponentKind::AiTarget)); }

    ui.horizontal_wrapped(|ui| {
        for (label, kind) in &chips {
            let resp = ui.add(egui::Label::new(
                egui::RichText::new(*label).small()
                    .background_color(ui.visuals().faint_bg_color)
                    .color(ui.visuals().weak_text_color()),
            ).sense(egui::Sense::click())).on_hover_text("Right-click to remove");
            resp.context_menu(|ui| {
                if ui.button(format!("Remove {}", label)).clicked() {
                    remove_component(world, e, *kind);
                    editor.undo_requested = true;
                    ui.close();
                }
            });
        }
    });
    ui.horizontal(|ui| {
        ui.menu_button("+ Add Component", |ui| { add_component_menu(ui, world, e, editor); });
        if !chips.is_empty() {
            ui.label(egui::RichText::new("right-click chip to remove").small().weak().italics());
        }
    });
}

fn inspector_transform(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Transform").default_open(true).show(ui, |ui| {
        let Some(t) = world.get_mut::<Transform>(e) else { return; };
        ui.horizontal(|ui| {
            if ui.button("Copy").clicked() { editor.clipboard_transform = Some(*t); }
            let paste_enabled = editor.clipboard_transform.is_some();
            if ui.add_enabled(paste_enabled, egui::Button::new("Paste")).clicked() {
                if let Some(src) = editor.clipboard_transform { editor.undo_requested = true; *t = src; }
            }
            if ui.button("Reset").clicked() {
                editor.undo_requested = true;
                t.position = Vec3::ZERO;
                t.rotation = glam::Quat::IDENTITY;
                t.scale = Vec3::ONE;
            }
        });
        draw_vec3_row(ui, "Position", &mut t.position, 0.01, editor);
        draw_euler_row(ui, "Rotation (deg)", &mut t.rotation, editor);
        draw_vec3_row(ui, "Scale", &mut t.scale, 0.01, editor);
    });
}

fn draw_vec3_row(ui: &mut egui::Ui, label: &str, v: &mut Vec3, speed: f32, editor: &mut EditorState) {
    let mut arr = v.to_array();
    ui.label(label);
    let mut changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(egui::DragValue::new(&mut arr[i]).speed(speed).prefix(["X ", "Y ", "Z "][i]));
            if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
            if r.changed() { changed = true; }
        }
    });
    if changed { *v = Vec3::from_array(arr); }
}

fn draw_euler_row(ui: &mut egui::Ui, label: &str, q: &mut glam::Quat, editor: &mut EditorState) {
    let (ry, rx, rz) = q.to_euler(glam::EulerRot::YXZ);
    let mut e = [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()];
    ui.label(label);
    let mut changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(egui::DragValue::new(&mut e[i]).speed(0.5).prefix(["X ", "Y ", "Z "][i]));
            if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
            if r.changed() { changed = true; }
        }
    });
    if changed {
        *q = glam::Quat::from_euler(glam::EulerRot::YXZ,
            e[1].to_radians(), e[0].to_radians(), e[2].to_radians());
    }
}

fn inspector_hierarchy(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Hierarchy").default_open(true).show(ui, |ui| {
        match world.get::<Parent>(e).copied() {
            Some(Parent(p)) => {
                let pname = world.get::<Name>(p).map(|n| n.0.clone()).unwrap_or_else(|| format!("#{}", p));
                ui.label(format!("Parent: {}", pname));
                if ui.button("Clear Parent").clicked() {
                    editor.undo_requested = true;
                    world.remove::<Parent>(e);
                }
            }
            None => { ui.label("Parent: (none)"); }
        }
        if editor.selected.len() >= 2 && ui.button("Set parent from last selected").clicked() {
            let primary = *editor.selected.last().unwrap();
            if primary != e && !would_create_cycle(world, e, primary) {
                editor.undo_requested = true;
                world.insert(e, Parent(primary));
            }
        }
        ui.separator();
        let children: Vec<Entity> = world.entities().iter().copied()
            .filter(|&c| world.get::<Parent>(c).map(|p| p.0 == e).unwrap_or(false)).collect();
        if children.is_empty() { ui.label("Children: (none)"); }
        else {
            ui.label(format!("Children: {}", children.len()));
            for c in children {
                let cname = world.get::<Name>(c).map(|n| n.0.clone()).unwrap_or_else(|| format!("#{}", c));
                if ui.selectable_label(false, cname).clicked() { editor.select_single(c); }
            }
        }
    });
}

fn inspector_geometry(ui: &mut egui::Ui, world: &mut World, e: Entity, assets: &UiAssets<'_>) {
    egui::CollapsingHeader::new("Geometry").default_open(true).show(ui, |ui| {
        if let Some(mh) = world.get_mut::<MeshHandle>(e) {
            let mut current = mh.0.clone();
            ui.horizontal(|ui| {
                ui.label("Mesh:");
                egui::ComboBox::from_id_salt("mesh_selector").selected_text(&current).show_ui(ui, |ui| {
                    for name in assets.mesh_names { ui.selectable_value(&mut current, name.clone(), name); }
                });
            });
            if current != mh.0 { mh.0 = current; }
        }
        if let Some(mh) = world.get_mut::<MaterialHandle>(e) {
            let mut current = mh.0.clone();
            ui.horizontal(|ui| {
                ui.label("Material:");
                egui::ComboBox::from_id_salt("mat_selector").selected_text(&current).show_ui(ui, |ui| {
                    for name in assets.material_names { ui.selectable_value(&mut current, name.clone(), name); }
                });
            });
            if current != mh.0 { mh.0 = current; }
        }
    });
}

fn inspector_texture_tiling(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Texture Tiling").default_open(true).show(ui, |ui| {
        ui.label(egui::RichText::new(
            "Размер одного тайла в мировых единицах. При масштабировании объекта текстура тайлится, а не растягивается."
        ).small().weak());
        let Some(t) = world.get_mut::<TextureTiling>(e) else { return; };
        let r = ui.add(egui::Slider::new(&mut t.size, 0.05..=50.0).logarithmic(true).text("Tile size"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        ui.horizontal(|ui| {
            if ui.small_button("Fine (0.25)").on_hover_text("Мелкая текстура").clicked() { t.size = 0.25; editor.undo_requested = true; }
            if ui.small_button("Default (1.0)").on_hover_text("1 unit = 1 tile").clicked() { t.size = 1.0; editor.undo_requested = true; }
            if ui.small_button("Coarse (4.0)").on_hover_text("Крупная текстура").clicked() { t.size = 4.0; editor.undo_requested = true; }
        });
    });
}

fn inspector_elevator(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Elevator").default_open(true).show(ui, |ui| {
        ui.label(egui::RichText::new(
            "FSM с trapezoid-профилем скорости: Idle → DoorsOpening → DoorsOpen → DoorsClosing → Moving. \
             Платформа двигается через RigidBody::Kinematic (velocity.y)."
        ).small().weak());
        let Some(el) = world.get_mut::<Elevator>(e) else { return; };
        let mut floors = el.floors.clone();
        let floor_count = floors.len();
        let mut remove: Option<usize> = None;
        ui.label(format!("Floors: {}", floor_count));
        for (i, y) in floors.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(format!("#{}:", i));
                let r = ui.add(egui::DragValue::new(y).speed(0.1).suffix(" m"));
                if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
                if floor_count > 2 && ui.small_button("✖").clicked() { remove = Some(i); }
            });
        }
        if floors != el.floors { el.floors = floors; editor.undo_requested = true; }
        if let Some(i) = remove {
            el.floors.remove(i);
            el.current_floor = el.current_floor.min(el.floors.len().saturating_sub(1));
            el.target_floor = el.target_floor.min(el.floors.len().saturating_sub(1));
            editor.undo_requested = true;
        }
        if ui.small_button("+ Floor").clicked() {
            let last = el.floors.last().copied().unwrap_or(0.0);
            el.floors.push(last + 3.0);
            editor.undo_requested = true;
        }
        ui.separator();
        let r = ui.add(egui::Slider::new(&mut el.speed, 0.1..=8.0).text("Max speed (m/s)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut el.acceleration, 0.5..=15.0).logarithmic(true).text("Acceleration (m/s²)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut el.door_speed, 0.2..=5.0).text("Door speed (1/s)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut el.dwell, 0.0..=10.0).text("Dwell (s)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut el.sensor_radius, 0.5..=5.0).text("Door sensor radius (m)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(format!("State: {:?}", el.state));
            ui.label(format!("Floor: {}/{}", el.current_floor, el.floors.len().saturating_sub(1)));
        });
        ui.horizontal(|ui| {
            ui.label(format!("Velocity: {:+.2} m/s", el.current_velocity));
            ui.label(format!("Doors: {:.0}%", el.doors_open * 100.0));
        });
        if el.player_inside {
            ui.label(egui::RichText::new("● Player in door sensor").small().color(egui::Color32::from_rgb(120, 220, 120)));
        }
        let r = ui.add(egui::Slider::new(&mut el.doors_open, 0.0..=1.0).text("Doors open"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        ui.separator();
        ui.label("Вызвать лифт:");
        let floors_snapshot: Vec<f32> = el.floors.clone();
        let mut call_floor: Option<usize> = None;
        ui.horizontal_wrapped(|ui| {
            for (i, &y) in floors_snapshot.iter().enumerate() {
                if ui.small_button(format!("F{} ({:.1}m)", i, y)).clicked() { call_floor = Some(i); }
            }
        });
        if let Some(i) = call_floor { el.call(i); editor.undo_requested = true; }
    });
}

fn inspector_sliding_door(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Sliding Door").default_open(true).show(ui, |ui| {
        let Some(sd) = world.get_mut::<SlidingDoor>(e) else { return; };
        let r = ui.add(egui::Slider::new(&mut sd.open_amount, 0.0..=1.0).text("Open amount"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut sd.target, 0.0..=1.0).text("Target"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut sd.speed, 0.1..=5.0).text("Speed (1/s)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut sd.slide_distance, 0.1..=5.0).text("Slide distance (local units)"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        ui.label("Slide axis (local)");
        let mut ax = sd.slide_axis.to_array();
        ui.horizontal(|ui| {
            for i in 0..3 {
                let r = ui.add(egui::DragValue::new(&mut ax[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i]));
                if r.changed() { editor.undo_requested = true; }
            }
        });
        sd.slide_axis = Vec3::from_array(ax).normalize_or_zero();
        ui.label("Closed position (local, rel. Parent)");
        let mut cp = sd.closed_position.to_array();
        ui.horizontal(|ui| {
            for i in 0..3 {
                let r = ui.add(egui::DragValue::new(&mut cp[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i]));
                if r.changed() { editor.undo_requested = true; }
            }
        });
        sd.closed_position = Vec3::from_array(cp);
    });
}

#[allow(clippy::too_many_arguments)]
fn inspector_material(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState,
    action: &mut Option<EditorAction>, assets: &UiAssets<'_>, name: &str, original: &Material) {
    let header_label = format!("Material: {}", name);
    egui::CollapsingHeader::new(header_label).default_open(false).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Make Unique").clicked() { *action = Some(EditorAction::MakeMaterialUnique); }
        });
        ui.separator();
        let mut m = original.clone();
        let mut changed = false;
        ui.label("Base color");
        if ui.color_edit_button_rgba_unmultiplied(&mut m.base_color).changed() { changed = true; editor.undo_requested = true; }
        let r = ui.add(egui::Slider::new(&mut m.metallic, 0.0..=1.0).text("Metallic"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        if r.changed() { changed = true; }
        let r = ui.add(egui::Slider::new(&mut m.roughness, 0.0..=1.0).text("Roughness"));
        if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        if r.changed() { changed = true; }
        ui.label("Emissive");
        if ui.color_edit_button_rgb(&mut m.emissive).changed() { changed = true; editor.undo_requested = true; }
        ui.separator();
        ui.label(egui::RichText::new("Textures").strong());
        ui.label(egui::RichText::new("загрузи через Assets → Load Texture(s)…").small().weak());
        if texture_picker(ui, "mat_base_tex", "Base color:", &mut m.base_color_texture, assets.texture_list) { changed = true; editor.undo_requested = true; }
        if texture_picker(ui, "mat_mr_tex", "Metallic-Rough:", &mut m.metallic_roughness_texture, assets.texture_list) { changed = true; editor.undo_requested = true; }
        if texture_picker(ui, "mat_normal_tex", "Normal map:", &mut m.normal_texture, assets.texture_list) { changed = true; editor.undo_requested = true; }
        if texture_picker(ui, "mat_emissive_tex", "Emissive map:", &mut m.emissive_texture, assets.texture_list) { changed = true; editor.undo_requested = true; }
        ui.separator();
        ui.label("Alpha mode");
        ui.horizontal(|ui| {
            let mut am = m.alpha_mode;
            if ui.selectable_label(am == AlphaMode::Opaque, "Opaque").clicked() { am = AlphaMode::Opaque; }
            if ui.selectable_label(am == AlphaMode::Mask, "Mask").clicked() { am = AlphaMode::Mask; }
            if ui.selectable_label(am == AlphaMode::Blend, "Blend").clicked() { am = AlphaMode::Blend; }
            if am != m.alpha_mode { m.alpha_mode = am; changed = true; }
        });
        if m.alpha_mode == AlphaMode::Mask {
            let r = ui.add(egui::Slider::new(&mut m.alpha_cutoff, 0.0..=1.0).text("Alpha cutoff"));
            if r.changed() { changed = true; }
        }
        if ui.checkbox(&mut m.double_sided, "Double-sided").changed() { changed = true; }
        if changed { editor.dirty_materials.push((e, name.to_string(), m)); }
        let _ = world;
    });
}

fn inspector_tint(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Tint").default_open(true).show(ui, |ui| {
        let Some(t) = world.get_mut::<Tint>(e) else { return; };
        let mut color = t.0;
        ui.horizontal(|ui| {
            ui.label("Color:");
            if ui.color_edit_button_rgba_unmultiplied(&mut color).changed() { t.0 = color; editor.undo_requested = true; }
            if ui.small_button("White").clicked() { t.0 = [1.0, 1.0, 1.0, 1.0]; editor.undo_requested = true; }
        });
        ui.label(egui::RichText::new("Поверх base_color материала (multiply)").small().weak());
    });
}

fn inspector_health(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Health").default_open(true).show(ui, |ui| {
        let Some(h) = world.get_mut::<Health>(e) else { return; };
        ui.add(egui::Slider::new(&mut h.max, 1.0..=1000.0).text("Max"));
        ui.add(egui::Slider::new(&mut h.current, 0.0..=h.max).text("Current"));
    });
}

fn inspector_chase(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Chase").default_open(true).show(ui, |ui| {
        let Some(c) = world.get_mut::<Chase>(e) else { return; };
        ui.add(egui::Slider::new(&mut c.speed, 0.1..=20.0).text("Speed"));
        ui.add(egui::Slider::new(&mut c.stop_distance, 0.1..=10.0).text("Stop dist"));
    });
}

fn inspector_spinner(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Spinner").default_open(true).show(ui, |ui| {
        let Some(sp) = world.get_mut::<Spinner>(e) else { return; };
        let mut axis = sp.axis.to_array();
        ui.label("Axis");
        ui.horizontal(|ui| {
            for i in 0..3 { ui.add(egui::DragValue::new(&mut axis[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i])); }
        });
        sp.axis = Vec3::from_array(axis);
        ui.add(egui::DragValue::new(&mut sp.speed).speed(0.01).prefix("Speed "));
    });
}

fn inspector_velocity(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Velocity").default_open(true).show(ui, |ui| {
        let Some(v) = world.get_mut::<Velocity>(e) else { return; };
        let mut val = v.value.to_array();
        ui.horizontal(|ui| {
            for i in 0..3 { ui.add(egui::DragValue::new(&mut val[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i])); }
        });
        v.value = Vec3::from_array(val);
    });
}

fn inspector_rigidbody(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("RigidBody").default_open(true).show(ui, |ui| {
        let has_parent = world.has::<Parent>(e);
        if has_parent {
            if let Some(rb) = world.get::<RigidBody>(e) {
                if rb.body_type != BodyType::Static {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgba_unmultiplied(120, 30, 30, 60))
                        .inner_margin(egui::Margin::symmetric(8, 6))
                        .corner_radius(egui::CornerRadius::same(4))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new("⚠ Physics disabled: has Parent")
                                    .strong()
                                    .color(egui::Color32::from_rgb(240, 130, 130)),
                            );
                            ui.label(
                                egui::RichText::new(
                                    "Parent-space dynamics is unsupported: \
                                     Transform.position is parent-local, and \
                                     gravity in a rotated parent frame would \
                                     be wrong. Remove Parent to enable simulation.",
                                )
                                .small()
                                .color(egui::Color32::from_rgb(220, 200, 200)),
                            );
                        });
                    ui.separator();
                }
            }
        }

        let Some(rb) = world.get_mut::<RigidBody>(e) else { return; };
        let mut changed = false;
        ui.horizontal(|ui| {
            ui.label("Type:");
            let mut bt = rb.body_type;
            ui.selectable_value(&mut bt, BodyType::Static, "Static");
            ui.selectable_value(&mut bt, BodyType::Dynamic, "Dynamic");
            ui.selectable_value(&mut bt, BodyType::Kinematic, "Kinematic");
            if bt != rb.body_type { rb.body_type = bt; changed = true; }
        });
        match rb.body_type {
            BodyType::Dynamic => {
                ui.add(egui::Slider::new(&mut rb.mass, 0.01..=100.0).logarithmic(true).text("Mass"));
                ui.add(egui::Slider::new(&mut rb.gravity_scale, 0.0..=3.0).text("Gravity scale"));
                ui.add(egui::Slider::new(&mut rb.linear_damping, 0.0..=1.0).text("Linear damping"));
                let mut v = rb.velocity.to_array();
                ui.label("Velocity");
                ui.horizontal(|ui| {
                    for i in 0..3 { ui.add(egui::DragValue::new(&mut v[i]).speed(0.1).prefix(["X ", "Y ", "Z "][i])); }
                });
                rb.velocity = Vec3::from_array(v);
                if ui.small_button("Reset velocity").clicked() { rb.velocity = Vec3::ZERO; changed = true; }
            }
            BodyType::Kinematic => {
                let mut v = rb.velocity.to_array();
                ui.label("Kinematic velocity (записывается системой лифта)");
                ui.horizontal(|ui| {
                    for i in 0..3 { ui.add(egui::DragValue::new(&mut v[i]).speed(0.1).prefix(["X ", "Y ", "Z "][i])); }
                });
                rb.velocity = Vec3::from_array(v);
            }
            BodyType::Static => {
                ui.label(egui::RichText::new(
                    "Static: бесконечная масса, не двигается. Масса и velocity игнорируются."
                ).small().weak());
            }
        }
        ui.horizontal(|ui| {
            ui.label(format!("Sleeping: {} ({:.2}s)", rb.sleeping, rb.sleep_timer));
            if ui.small_button("Wake").clicked() { rb.wake(); }
        });
        if changed { editor.undo_requested = true; }
    });
}

fn inspector_collider(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Collider").default_open(true).show(ui, |ui| {
        let current_kind = match world.get::<Collider>(e) {
            Some(Collider::Sphere { .. }) => 0,
            Some(Collider::Aabb { .. }) => 1,
            Some(Collider::Capsule { .. }) => 2,
            None => return,
        };
        let mut new_kind = current_kind;
        ui.horizontal(|ui| {
            ui.label("Shape:");
            ui.selectable_value(&mut new_kind, 0, "Sphere");
            ui.selectable_value(&mut new_kind, 1, "AABB");
            ui.selectable_value(&mut new_kind, 2, "Capsule");
        });
        if new_kind != current_kind {
            if let Some(col) = world.get_mut::<Collider>(e) {
                *col = match new_kind {
                    0 => Collider::sphere(0.5),
                    1 => Collider::aabb(Vec3::splat(0.5)),
                    2 => Collider::capsule(0.35, 1.8),
                    _ => unreachable!(),
                };
            }
            editor.undo_requested = true;
            return;
        }
        let Some(col) = world.get_mut::<Collider>(e) else { return; };
        match col {
            Collider::Sphere { radius } => { ui.add(egui::Slider::new(radius, 0.05..=20.0).text("Radius")); }
            Collider::Aabb { half_extents } => {
                let mut h = half_extents.to_array();
                ui.label("Half extents");
                ui.horizontal(|ui| {
                    for i in 0..3 { ui.add(egui::DragValue::new(&mut h[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i])); }
                });
                *half_extents = Vec3::from_array(h);
            }
            Collider::Capsule { radius, height } => {
                ui.add(egui::Slider::new(radius, 0.05..=5.0).text("Radius"));
                ui.add(egui::Slider::new(height, 0.1..=10.0).text("Height"));
            }
        }
    });
}

fn inspector_physics_material(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    egui::CollapsingHeader::new("PhysicsMaterial").default_open(false).show(ui, |ui| {
        ui.label("Presets:");
        ui.horizontal_wrapped(|ui| {
            let presets: [(&str, PhysicsMaterial); 5] = [
                ("Wood", PhysicsMaterial::wood()), ("Metal", PhysicsMaterial::metal()),
                ("Rubber", PhysicsMaterial::rubber()), ("Ice", PhysicsMaterial::ice()),
                ("Concrete", PhysicsMaterial::concrete()),
            ];
            for (label, preset) in presets {
                if ui.small_button(label).clicked() {
                    if let Some(m) = world.get_mut::<PhysicsMaterial>(e) { *m = preset; editor.undo_requested = true; }
                }
            }
        });
        ui.separator();
        let Some(m) = world.get_mut::<PhysicsMaterial>(e) else { return; };
        ui.add(egui::Slider::new(&mut m.restitution, 0.0..=1.0).text("Restitution (bounce)"));
        ui.add(egui::Slider::new(&mut m.friction, 0.0..=2.0).text("Friction"));
    });
}

#[derive(Copy, Clone, PartialEq, Eq)]
enum ComponentKind {
    Transform, Mesh, Material, Skeleton, Animation, Spinner, Velocity, Health, Chase,
    Interactable, Trigger, Parent, Tint, Visible, TextureTiling, Elevator, SlidingDoor,
    RigidBody, Collider, PhysicsMaterial, DirectionalLight, PointLight, Decal,
    AudioSource,
    AiAgent, PatrolPath, Enemy, AiTarget,
}

fn remove_component(world: &mut World, e: Entity, kind: ComponentKind) {
    use crate::game::components::*;
    match kind {
        ComponentKind::Transform => { world.remove::<Transform>(e); }
        ComponentKind::Mesh => { world.remove::<MeshHandle>(e); }
        ComponentKind::Material => { world.remove::<MaterialHandle>(e); }
        ComponentKind::Skeleton => { world.remove::<SkeletonHandle>(e); }
        ComponentKind::Animation => { world.remove::<AnimationPlayer>(e); }
        ComponentKind::Spinner => { world.remove::<Spinner>(e); }
        ComponentKind::Velocity => { world.remove::<Velocity>(e); }
        ComponentKind::Health => { world.remove::<Health>(e); }
        ComponentKind::Chase => { world.remove::<Chase>(e); }
        ComponentKind::Interactable => { world.remove::<Interactable>(e); }
        ComponentKind::Trigger => { world.remove::<Trigger>(e); }
        ComponentKind::Parent => { world.remove::<Parent>(e); }
        ComponentKind::Tint => { world.remove::<Tint>(e); }
        ComponentKind::Visible => { world.remove::<Visible>(e); }
        ComponentKind::TextureTiling => { world.remove::<TextureTiling>(e); }
        ComponentKind::Elevator => { world.remove::<Elevator>(e); }
        ComponentKind::SlidingDoor => { world.remove::<SlidingDoor>(e); }
        ComponentKind::RigidBody => { world.remove::<RigidBody>(e); }
        ComponentKind::Collider => { world.remove::<Collider>(e); }
        ComponentKind::PhysicsMaterial => { world.remove::<PhysicsMaterial>(e); }
        ComponentKind::DirectionalLight => { world.remove::<DirectionalLight>(e); }
        ComponentKind::PointLight => { world.remove::<PointLight>(e); }
        ComponentKind::Decal => { world.remove::<crate::game::decals::Decal>(e); }
        ComponentKind::AudioSource => { world.remove::<AudioSource>(e); }
        ComponentKind::AiAgent => { world.remove::<crate::game::ai::AiAgent>(e); }
        ComponentKind::PatrolPath => { world.remove::<crate::game::ai::PatrolPath>(e); }
        ComponentKind::Enemy => { world.remove::<crate::game::ai::Enemy>(e); }
        ComponentKind::AiTarget => { world.remove::<crate::game::ai::AiTarget>(e); }
    }
}

fn add_component_menu(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    use crate::game::components::*;
    let mut any = false;

    if !world.has::<Transform>(e) {
        any = true;
        if ui.button("Transform").clicked() { world.insert(e, Transform::at(Vec3::ZERO)); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<MeshHandle>(e) {
        any = true;
        if ui.button("Mesh").clicked() { world.insert(e, MeshHandle("cube".into())); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<MaterialHandle>(e) {
        any = true;
        if ui.button("Material").clicked() { world.insert(e, MaterialHandle("flat_blue".into())); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<Tint>(e) {
        any = true;
        if ui.button("Tint").clicked() { world.insert(e, Tint([1.0, 1.0, 1.0, 1.0])); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<Visible>(e) {
        any = true;
        if ui.button("Visible (false)").clicked() { world.insert(e, Visible(false)); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<TextureTiling>(e) {
        any = true;
        if ui.button("Texture Tiling").on_hover_text("Size of one tile in world units (1.0 = 1 unit per tile)").clicked() {
            world.insert(e, TextureTiling::default()); editor.undo_requested = true; ui.close();
        }
    }
    if !world.has::<AudioSource>(e) {
        any = true;
        ui.menu_button("🔊 Audio Source", |ui| {
            let presets = [
                ("pickup (SFX)", AudioSource::new("pickup").with_bus(AudioBus::Sfx).with_range(1.0, 20.0)),
                ("shot (SFX)",   AudioSource::new("shot").with_bus(AudioBus::Sfx).with_range(1.0, 30.0)),
                ("ding (Music)", AudioSource::new("ding").with_bus(AudioBus::Music).with_range(0.5, 15.0)),
                ("pickup (UI, non-spatial)",
                    AudioSource::non_spatial("pickup").with_bus(AudioBus::Ui)),
                ("ambient loop (Music)",
                    AudioSource::looping("ding").with_bus(AudioBus::Music).with_range(2.0, 25.0)),
            ];
            for (label, src) in presets {
                if ui.button(label).clicked() {
                    world.insert(e, src);
                    editor.undo_requested = true;
                    ui.close();
                }
            }
        });
    }
    if !world.has::<crate::game::ai::AiAgent>(e) {
        any = true;
        ui.menu_button("🤖 AI Agent", |ui| {
            let presets: [(&str, crate::game::ai::AiAgent); 4] = [
                ("Melee (fast)", crate::game::ai::AiAgent::new()
                    .with_speed(4.0)
                    .with_vision(18.0, 60_f32.to_radians())
                    .with_attack(1.5, 10.0, 0.8)),
                ("Brute (slow)", crate::game::ai::AiAgent::new()
                    .with_speed(2.0)
                    .with_vision(12.0, 45_f32.to_radians())
                    .with_attack(2.0, 25.0, 1.5)),
                ("Sniper (range)", crate::game::ai::AiAgent::new()
                    .with_speed(3.0)
                    .with_vision(30.0, 30_f32.to_radians())
                    .with_attack(8.0, 15.0, 2.0)),
                ("Default", crate::game::ai::AiAgent::new()),
            ];
            for (label, agent) in presets {
                if ui.button(label).clicked() {
                    world.insert(e, agent);
                    editor.undo_requested = true;
                    ui.close();
                }
            }
        });
    }
    if !world.has::<crate::game::ai::Enemy>(e) {
        any = true;
        if ui.button("👹 Enemy marker").on_hover_text("Помечает entity как 'врага' (для систем и UI)").clicked() {
            world.insert(e, crate::game::ai::Enemy);
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<crate::game::ai::AiTarget>(e) {
        any = true;
        if ui.button("🎯 AI Target").on_hover_text("Маркер цели — AI-агенты будут её преследовать").clicked() {
            world.insert(e, crate::game::ai::AiTarget);
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<crate::game::ai::PatrolPath>(e) {
        any = true;
        if ui.button("🚶 Patrol Path (circle)").on_hover_text("Простой круг радиусом 5м вокруг позиции").clicked() {
            let origin = world.get::<Transform>(e).map(|t| t.position).unwrap_or(Vec3::ZERO);
            let mut pts = Vec::new();
            for i in 0..4 {
                let a = i as f32 / 4.0 * std::f32::consts::TAU;
                pts.push(origin + Vec3::new(a.cos() * 5.0, 0.0, a.sin() * 5.0));
            }
            world.insert(e, crate::game::ai::PatrolPath::new(pts));
            editor.undo_requested = true;
            ui.close();
        }
    }

    if !world.has::<Elevator>(e) {
        any = true;
        if ui.button("Elevator (2 floors)").on_hover_text("FSM-лифт. Требует RigidBody::Kinematic + Collider::Aabb.").clicked() {
            world.insert(e, Elevator::new(vec![0.0, 3.0], 2.0)); editor.undo_requested = true; ui.close();
        }
    }
    if !world.has::<SlidingDoor>(e) {
        any = true;
        if ui.button("Sliding Door").on_hover_text("Створка. Обычно Parent к платформе лифта.").clicked() {
            world.insert(e, SlidingDoor::new(Vec3::new(0.7, 1.0, 1.5), Vec3::X, 1.4)); editor.undo_requested = true; ui.close();
        }
    }
    if !world.has::<Spinner>(e) || !world.has::<Velocity>(e) || !world.has::<Chase>(e) || !world.has::<Parent>(e) {
        ui.separator();
    }
    if !world.has::<Spinner>(e) {
        any = true;
        if ui.button("Spinner").clicked() { world.insert(e, Spinner::new(Vec3::Y, 1.0)); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<Velocity>(e) {
        any = true;
        if ui.button("Velocity").clicked() { world.insert(e, Velocity::new(0.0, 0.0, 0.0)); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<Chase>(e) {
        any = true;
        if ui.button("Chase (legacy)").on_hover_text("Простой lerp к игроку. Используй AI Agent для полноценного поведения.").clicked() {
            world.insert(e, Chase::new(3.0, 1.2)); editor.undo_requested = true; ui.close();
        }
    }
    if !world.has::<Parent>(e) {
        any = true;
        if ui.button("Parent (self-id)").clicked() { world.insert(e, Parent(e)); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<Health>(e) || !world.has::<Interactable>(e) || !world.has::<Trigger>(e) {
        ui.separator();
    }
    if !world.has::<Health>(e) {
        any = true;
        if ui.button("Health").clicked() { world.insert(e, Health::new(100.0)); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<Interactable>(e) {
        any = true;
        ui.menu_button("Interactable", |ui| {
            if ui.button("Pickup").clicked() { world.insert(e, Interactable::Pickup); editor.undo_requested = true; ui.close(); }
            if ui.button("Paint (red)").clicked() { world.insert(e, Interactable::Paint([1.0, 0.0, 0.0, 1.0])); editor.undo_requested = true; ui.close(); }
            if ui.button("Toggle").clicked() { world.insert(e, Interactable::Toggle); editor.undo_requested = true; ui.close(); }
        });
    }
    if !world.has::<Trigger>(e) {
        any = true;
        ui.menu_button("Trigger", |ui| {
            if ui.button("Teleport to (0, 2, 0)").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Teleport([0.0, 2.0, 0.0]))); editor.undo_requested = true; ui.close();
            }
            if ui.button("Tint (green)").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Tint([0.2, 1.0, 0.2, 1.0]))); editor.undo_requested = true; ui.close();
            }
            if ui.button("Despawn").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Despawn)); editor.undo_requested = true; ui.close();
            }
            if ui.button("Play Sound (pickup)").clicked() {
                world.insert(e, Trigger::repeatable(2.5, TriggerAction::PlaySound("pickup".to_string()))); editor.undo_requested = true; ui.close();
            }
            if ui.button("Call Elevator (self, floor 0)").clicked() {
                world.insert(e, Trigger::repeatable(1.5, TriggerAction::CallElevator { elevator: e, floor_idx: 0 })); editor.undo_requested = true; ui.close();
            }
        });
    }
    if !world.has::<AnimationPlayer>(e) || !world.has::<SkeletonHandle>(e) {
        ui.separator();
    }
    if !world.has::<AnimationPlayer>(e) {
        any = true;
        if ui.button("Animation Player").clicked() { world.insert(e, AnimationPlayer::new("")); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<SkeletonHandle>(e) {
        any = true;
        if ui.button("Skeleton Handle").clicked() { world.insert(e, SkeletonHandle(String::new())); editor.undo_requested = true; ui.close(); }
    }
    ui.separator();
    if !world.has::<RigidBody>(e) {
        any = true;
        let has_parent = world.has::<Parent>(e);
        ui.menu_button("RigidBody", |ui| {
            if has_parent {
                ui.label(
                    egui::RichText::new("⚠ Entity has Parent — physics disabled")
                        .small()
                        .color(egui::Color32::from_rgb(240, 130, 130)),
                );
                ui.separator();
            }
            if ui.button("Static").clicked() {
                world.insert(e, RigidBody::static_body());
                editor.undo_requested = true;
                ui.close();
            }
            if ui
                .add_enabled(!has_parent, egui::Button::new("Dynamic (mass 1)"))
                .on_disabled_hover_text("Parent-space dynamics is unsupported. Remove Parent first.")
                .clicked()
            {
                world.insert(e, RigidBody::dynamic(1.0));
                editor.undo_requested = true;
                ui.close();
            }
            if ui
                .add_enabled(!has_parent, egui::Button::new("Kinematic"))
                .on_disabled_hover_text("Parent-space dynamics is unsupported. Remove Parent first.")
                .clicked()
            {
                world.insert(e, RigidBody::kinematic());
                editor.undo_requested = true;
                ui.close();
            }
        });
    }
    if !world.has::<Collider>(e) {
        any = true;
        ui.menu_button("Collider", |ui| {
            if ui.button("Sphere (r = 0.5)").clicked() { world.insert(e, Collider::sphere(0.5)); editor.undo_requested = true; ui.close(); }
            if ui.button("AABB (1×1×1)").clicked() { world.insert(e, Collider::aabb(Vec3::splat(0.5))); editor.undo_requested = true; ui.close(); }
            if ui.button("Capsule (r = 0.35, h = 1.8)").clicked() { world.insert(e, Collider::capsule(0.35, 1.8)); editor.undo_requested = true; ui.close(); }
        });
    }
    if !world.has::<PhysicsMaterial>(e) {
        any = true;
        ui.menu_button("PhysicsMaterial", |ui| {
            for (label, mat) in [
                ("Wood", PhysicsMaterial::wood()), ("Metal", PhysicsMaterial::metal()),
                ("Rubber", PhysicsMaterial::rubber()), ("Ice", PhysicsMaterial::ice()),
                ("Concrete", PhysicsMaterial::concrete()),
            ] {
                if ui.button(label).clicked() { world.insert(e, mat); editor.undo_requested = true; ui.close(); }
            }
        });
    }
    ui.separator();
    ui.label(egui::RichText::new("Lights").strong());
    if !world.has::<DirectionalLight>(e) {
        any = true;
        if ui.button("☀ Directional Light").clicked() { world.insert(e, DirectionalLight::sun()); editor.undo_requested = true; ui.close(); }
    }
    if !world.has::<PointLight>(e) {
        any = true;
        ui.menu_button("💡 Point Light", |ui| {
            for (label, l) in [
                ("Warm", PointLight::new([1.0, 0.6, 0.3], 4.0, 15.0)),
                ("Cool", PointLight::new([0.4, 0.7, 1.0], 3.0, 12.0)),
                ("Red", PointLight::new([1.0, 0.15, 0.15], 5.0, 10.0)),
                ("Green", PointLight::new([0.2, 1.0, 0.3], 5.0, 10.0)),
                ("Candle", PointLight::new([1.0, 0.55, 0.2], 1.5, 6.0)),
            ] {
                if ui.button(label).clicked() { world.insert(e, l); editor.undo_requested = true; ui.close(); }
            }
        });
    }
    if !world.has::<crate::game::decals::Decal>(e) {
        any = true;
        if ui.button("🎨 Decal").clicked() {
            world.insert(e, crate::game::decals::Decal::default());
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !any {
        ui.label(egui::RichText::new("All components already present").weak().italics());
    }
}

// ============================================================
// Renderer panel
// ============================================================

fn apply_preset(postfx: &mut PostFx, mut preset: PostFx) {
    preset.debug_view = postfx.debug_view;
    *postfx = preset;
}

fn preset_low() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.0; p.ssao_strength = 0.0; p.ibl_strength = 0.15;
    p.taa_strength = 0.0; p.taa_sharpening = 0.0; p.fxaa_strength = 1.0;
    p.fog_density = 0.0; p.volumetric_density = 0.0;
    p
}

fn preset_medium() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.4; p.ssao_strength = 0.5; p.ibl_strength = 0.3;
    p.taa_strength = 1.0; p.taa_sharpening = 0.05; p.fxaa_strength = 0.7;
    p.volumetric_density = 0.015; p.volumetric_scattering = 0.35; p.volumetric_phase_g = 0.5;
    p
}

fn preset_high() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.6; p.ssao_strength = 0.8; p.ibl_strength = 0.35;
    p.taa_strength = 1.0; p.taa_sharpening = 0.1; p.fxaa_strength = 0.5;
    p.volumetric_density = 0.025; p.volumetric_scattering = 0.4; p.volumetric_phase_g = 0.6;
    p
}

fn preset_ultra() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.8; p.bloom_knee = 0.4;
    p.ssao_strength = 1.0; p.ssao_radius = 0.8; p.ibl_strength = 0.45;
    p.taa_strength = 1.0; p.taa_sharpening = 0.15; p.fxaa_strength = 0.3;
    p.vignette_strength = 0.15;
    p.volumetric_density = 0.04; p.volumetric_scattering = 0.5; p.volumetric_phase_g = 0.7;
    p
}

fn draw_renderer_panel(
    ui: &mut egui::Ui,
    postfx: &mut PostFx,
    editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    egui::CollapsingHeader::new("AI / Navmesh").default_open(true).show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui
                .button("🔨 Bake Navmesh")
                .on_hover_text(
                    "Строит grid-navmesh из текущих физических коллайдеров сцены.\n\
                     Результат используется AI-агентами (AiAgent + PatrolPath).",
                )
                .clicked()
            {
                *action = Some(EditorAction::BakeNavmesh);
            }
        });
        ui.checkbox(&mut editor.settings.show_navmesh, "Show navmesh")
            .on_hover_text(
                "Отображает границы walkable-ячеек зелёными линиями.\n\
                 Видно только после того, как navmesh запечён.",
            );
        ui.label(
            egui::RichText::new(
                "Bake после любых изменений геометрии сцены: добавления стен, полов, препятствий."
            )
            .small()
            .weak()
            .italics(),
        );
    });

    ui.separator();
    ui.label(egui::RichText::new("Quality preset").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Low").on_hover_text("Без TAA, SSAO, bloom, volumetric. Максимум FPS.").clicked() { apply_preset(postfx, preset_low()); }
        if ui.button("Medium").on_hover_text("Мягкое SSAO, лёгкий bloom, TAA, лёгкий туман.").clicked() { apply_preset(postfx, preset_medium()); }
        if ui.button("High").on_hover_text("Полное SSAO, заметный bloom, TAA + sharpen, god rays.").clicked() { apply_preset(postfx, preset_high()); }
        if ui.button("Ultra").on_hover_text("Максимум постобработки.").clicked() { apply_preset(postfx, preset_ultra()); }
        if ui.button("Reset").on_hover_text("PostFx::default()").clicked() { apply_preset(postfx, PostFx::default()); }
    });
    ui.separator();
    egui::CollapsingHeader::new("Anti-aliasing").default_open(true).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.taa_strength, 0.0..=1.0).text("TAA strength"));
        ui.label(egui::RichText::new("1.0 = полное накопление истории. 0.0 = passthrough.").small().weak());
        ui.add(egui::Slider::new(&mut postfx.taa_sharpening, 0.0..=1.0).text("TAA sharpening"));
        ui.label(egui::RichText::new("Unsharp-mask поверх TAA.").small().weak());
        ui.add(egui::Slider::new(&mut postfx.fxaa_strength, 0.0..=1.0).text("FXAA strength"));
        ui.label(egui::RichText::new("При активной TAA ослабляется автоматически (в 2×).").small().weak());
    });
    ui.separator();
    egui::CollapsingHeader::new("Post-processing").default_open(true).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.bloom_threshold, 0.1..=5.0).text("Bloom threshold"));
        ui.add(egui::Slider::new(&mut postfx.bloom_strength, 0.0..=3.0).text("Bloom strength"));
        ui.add(egui::Slider::new(&mut postfx.bloom_knee, 0.01..=2.0).text("Bloom knee"));
        ui.add(egui::Slider::new(&mut postfx.bloom_radius, 0.5..=3.0).text("Bloom radius"));
        ui.add(egui::Slider::new(&mut postfx.exposure, 0.1..=3.0).text("Exposure"));
    });

    // Спринт 1.1 + 1.2: Color grading + Tonemapper
    ui.separator();
    egui::CollapsingHeader::new("🎨 Color grading").default_open(true).show(ui, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("Tonemapper:");
            let tm = &mut postfx.tonemapper;
            if ui.selectable_label(*tm == Tonemapper::AgX, "AgX")
                .on_hover_text("Реалистичный (Sobotka 2022). Дефолт.").clicked() { *tm = Tonemapper::AgX; }
            if ui.selectable_label(*tm == Tonemapper::ACESFilmic, "ACES")
                .on_hover_text("Кинематографичный (Narkowicz).").clicked() { *tm = Tonemapper::ACESFilmic; }
            if ui.selectable_label(*tm == Tonemapper::Reinhard, "Reinhard")
                .on_hover_text("Простейший x/(1+x).").clicked() { *tm = Tonemapper::Reinhard; }
            if ui.selectable_label(*tm == Tonemapper::Uncharted2, "Uncharted2")
                .on_hover_text("Naughty Dog, яркая кривая.").clicked() { *tm = Tonemapper::Uncharted2; }
            if ui.selectable_label(*tm == Tonemapper::None, "None")
                .on_hover_text("Без тонального маппинга (clamp 0..1).").clicked() { *tm = Tonemapper::None; }
        });
        ui.separator();
        ui.add(egui::Slider::new(&mut postfx.exposure_bias, 0.25..=4.0)
            .logarithmic(true).text("Exposure bias"));
        ui.label(egui::RichText::new("Множитель к Exposure выше. 1.0 = нейтрально.")
            .small().weak().italics());
        ui.separator();
        ui.add(egui::Slider::new(&mut postfx.color_temperature, -1.0..=1.0)
            .text("Temperature (cool ↔ warm)"));
        ui.add(egui::Slider::new(&mut postfx.color_tint, -1.0..=1.0)
            .text("Tint (green ↔ magenta)"));
        ui.add(egui::Slider::new(&mut postfx.color_contrast, 0.5..=2.0)
            .text("Contrast"));
        ui.add(egui::Slider::new(&mut postfx.color_saturation, 0.0..=2.0)
            .text("Saturation"));
        ui.separator();
        ui.label(egui::RichText::new("ASC CDL").strong());
        ui.horizontal(|ui| {
            ui.label("Lift:");
            for i in 0..3 {
                ui.add(egui::DragValue::new(&mut postfx.color_lift[i])
                    .speed(0.005).range(-0.2..=0.2)
                    .prefix(["R ", "G ", "B "][i]));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Gain:");
            for i in 0..3 {
                ui.add(egui::DragValue::new(&mut postfx.color_gain[i])
                    .speed(0.005).range(0.0..=2.0)
                    .prefix(["R ", "G ", "B "][i]));
            }
        });
        ui.horizontal(|ui| {
            ui.label("Gamma:");
            for i in 0..3 {
                ui.add(egui::DragValue::new(&mut postfx.color_gamma[i])
                    .speed(0.005).range(0.1..=3.0)
                    .prefix(["R ", "G ", "B "][i]));
            }
        });
        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Reset grading").clicked() {
                postfx.exposure_bias = 1.0;
                postfx.color_temperature = 0.0;
                postfx.color_tint = 0.0;
                postfx.color_contrast = 1.0;
                postfx.color_saturation = 1.0;
                postfx.color_lift = [0.0, 0.0, 0.0];
                postfx.color_gain = [1.0, 1.0, 1.0];
                postfx.color_gamma = [1.0, 1.0, 1.0];
            }
            if ui.button("Cinematic").on_hover_text("Тёплый контрастный пресет").clicked() {
                postfx.tonemapper = Tonemapper::ACESFilmic;
                postfx.exposure_bias = 1.0;
                postfx.color_temperature = 0.15;
                postfx.color_tint = 0.0;
                postfx.color_contrast = 1.15;
                postfx.color_saturation = 1.05;
                postfx.color_lift = [0.0, -0.005, 0.01];
                postfx.color_gain = [1.03, 1.0, 0.97];
                postfx.color_gamma = [1.0, 1.0, 1.02];
            }
            if ui.button("Cold horror").on_hover_text("Холодный, десатурированный").clicked() {
                postfx.tonemapper = Tonemapper::AgX;
                postfx.exposure_bias = 0.9;
                postfx.color_temperature = -0.25;
                postfx.color_tint = -0.05;
                postfx.color_contrast = 1.1;
                postfx.color_saturation = 0.75;
                postfx.color_lift = [-0.01, -0.005, 0.01];
                postfx.color_gain = [0.97, 1.0, 1.05];
                postfx.color_gamma = [1.02, 1.0, 0.98];
            }
            if ui.button("Sunny warm").clicked() {
                postfx.tonemapper = Tonemapper::Uncharted2;
                postfx.exposure_bias = 1.1;
                postfx.color_temperature = 0.25;
                postfx.color_tint = 0.0;
                postfx.color_contrast = 1.05;
                postfx.color_saturation = 1.2;
                postfx.color_lift = [0.01, 0.005, 0.0];
                postfx.color_gain = [1.05, 1.0, 0.95];
                postfx.color_gamma = [1.0, 1.0, 1.0];
            }
        });
    });

    // Спринт 1.4: Lens flare
    ui.separator();
    egui::CollapsingHeader::new("✨ Lens flare").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.lens_flare_intensity, 0.0..=1.5)
            .text("Intensity"));
        ui.label(egui::RichText::new("0 = выключено. 0.3..0.8 — заметный flares.")
            .small().weak().italics());
        ui.add(egui::Slider::new(&mut postfx.lens_flare_threshold, 0.5..=8.0)
            .logarithmic(true).text("Brightness threshold"));
        ui.label(egui::RichText::new("Реагирует только на HDR-пиксели ярче порога.")
            .small().weak().italics());

        let mut ghosts = postfx.lens_flare_ghosts as i32;
        if ui.add(egui::Slider::new(&mut ghosts, 0..=12)
            .text("Ghost count")).changed() {
            postfx.lens_flare_ghosts = ghosts.max(0) as u32;
        }
        ui.add(egui::Slider::new(&mut postfx.lens_flare_streak, 0.0..=0.5)
            .text("Streak length"));

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Reset").clicked() {
                postfx.lens_flare_intensity = 0.0;
                postfx.lens_flare_threshold = 1.5;
                postfx.lens_flare_ghosts = 6;
                postfx.lens_flare_streak = 0.15;
            }
            if ui.button("Cinematic").on_hover_text("Мягкий flare").clicked() {
                postfx.lens_flare_intensity = 0.4;
                postfx.lens_flare_threshold = 2.0;
                postfx.lens_flare_ghosts = 4;
                postfx.lens_flare_streak = 0.12;
            }
            if ui.button("J.J. Abrams").on_hover_text("Много flare").clicked() {
                postfx.lens_flare_intensity = 1.0;
                postfx.lens_flare_threshold = 1.0;
                postfx.lens_flare_ghosts = 10;
                postfx.lens_flare_streak = 0.35;
            }
            if ui.button("Subtle").clicked() {
                postfx.lens_flare_intensity = 0.15;
                postfx.lens_flare_threshold = 3.0;
                postfx.lens_flare_ghosts = 3;
                postfx.lens_flare_streak = 0.08;
            }
        });
    });

    // Спринт 2.1: Depth of Field
    ui.separator();
    egui::CollapsingHeader::new("🎥 Depth of Field").default_open(false).show(ui, |ui| {
        let mut enabled = postfx.dof_enabled > 0.5;
        if ui.checkbox(&mut enabled, "Enable DOF").changed() {
            postfx.dof_enabled = if enabled { 1.0 } else { 0.0 };
        }

        ui.add_enabled_ui(enabled, |ui| {
            ui.add(egui::Slider::new(&mut postfx.dof_focus_distance, 0.5..=100.0)
                .logarithmic(true).text("Focus distance (m)"));
            ui.label(egui::RichText::new("Дистанция фокуса от камеры.")
                .small().weak().italics());

            ui.add(egui::Slider::new(&mut postfx.dof_focus_range, 0.0..=30.0)
                .text("Focus range (m)"));
            ui.label(egui::RichText::new("Зона резкости вокруг фокуса.")
                .small().weak().italics());

            ui.add(egui::Slider::new(&mut postfx.dof_max_blur, 0.0..=32.0)
                .text("Max blur (px)"));
            ui.label(egui::RichText::new("Максимальный радиус CoC.")
                .small().weak().italics());

            ui.add(egui::Slider::new(&mut postfx.dof_blur_falloff, 0.5..=20.0)
                .logarithmic(true).text("Blur falloff (m/px)"));
            ui.label(egui::RichText::new("Меньше = размытие растёт быстрее.")
                .small().weak().italics());
        });

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Auto (from camera)").on_hover_text("Фокус на цель камеры").clicked() {
                postfx.dof_focus_distance = postfx.dof_focus_distance.clamp(0.5, 100.0);
                postfx.dof_enabled = 1.0;
            }
            if ui.button("Cinematic").on_hover_text("Портрет: резкий центр, мягкий фон").clicked() {
                postfx.dof_enabled = 1.0;
                postfx.dof_focus_distance = 6.0;
                postfx.dof_focus_range = 1.5;
                postfx.dof_max_blur = 12.0;
                postfx.dof_blur_falloff = 3.0;
            }
            if ui.button("Macro").on_hover_text("Очень близкий фокус").clicked() {
                postfx.dof_enabled = 1.0;
                postfx.dof_focus_distance = 1.5;
                postfx.dof_focus_range = 0.3;
                postfx.dof_max_blur = 16.0;
                postfx.dof_blur_falloff = 1.5;
            }
            if ui.button("Landscape").on_hover_text("Всё резкое, размыт только дальний план").clicked() {
                postfx.dof_enabled = 1.0;
                postfx.dof_focus_distance = 40.0;
                postfx.dof_focus_range = 30.0;
                postfx.dof_max_blur = 6.0;
                postfx.dof_blur_falloff = 8.0;
            }
            if ui.button("Disable").clicked() {
                postfx.dof_enabled = 0.0;
            }
        });
    });

    // Спринт 2.2: Motion Blur
    ui.separator();
    egui::CollapsingHeader::new("💨 Motion Blur").default_open(false).show(ui, |ui| {
        let mut enabled = postfx.motion_blur_enabled > 0.5;
        if ui.checkbox(&mut enabled, "Enable motion blur").changed() {
            postfx.motion_blur_enabled = if enabled { 1.0 } else { 0.0 };
        }

        ui.add_enabled_ui(enabled, |ui| {
            ui.add(egui::Slider::new(&mut postfx.motion_blur_intensity, 0.0..=2.0)
                .text("Intensity"));
            ui.label(egui::RichText::new("Множитель скорости в пиксели. 1.0 = как в motion buffer.")
                .small().weak().italics());

            ui.add(egui::Slider::new(&mut postfx.motion_blur_max_px, 4.0..=120.0)
                .logarithmic(true).text("Max blur length (px)"));

            let mut samples = postfx.motion_blur_samples as i32;
            if ui.add(egui::Slider::new(&mut samples, 2..=24)
                .text("Samples")).changed() {
                postfx.motion_blur_samples = samples.max(2) as u32;
            }
        });

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            if ui.button("Subtle").clicked() {
                postfx.motion_blur_enabled = 1.0;
                postfx.motion_blur_intensity = 0.3;
                postfx.motion_blur_max_px = 16.0;
                postfx.motion_blur_samples = 8;
            }
            if ui.button("Cinematic").on_hover_text("Заметный blur при быстром движении").clicked() {
                postfx.motion_blur_enabled = 1.0;
                postfx.motion_blur_intensity = 0.8;
                postfx.motion_blur_max_px = 48.0;
                postfx.motion_blur_samples = 14;
            }
            if ui.button("Speed").on_hover_text("Драматичный blur (racing, dash)").clicked() {
                postfx.motion_blur_enabled = 1.0;
                postfx.motion_blur_intensity = 1.5;
                postfx.motion_blur_max_px = 100.0;
                postfx.motion_blur_samples = 20;
            }
            if ui.button("Disable").clicked() {
                postfx.motion_blur_enabled = 0.0;
            }
        });
    });

    ui.separator();
    egui::CollapsingHeader::new("SSAO").default_open(true).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.ssao_strength, 0.0..=2.0).text("Strength"));
        ui.add(egui::Slider::new(&mut postfx.ssao_radius, 0.05..=2.0).text("Radius"));
    });
    ui.separator();
    egui::CollapsingHeader::new("IBL").default_open(true).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.ibl_strength, 0.0..=3.0).text("IBL strength"));
    });
    ui.separator();
    egui::CollapsingHeader::new("Fog (height)").default_open(false).show(ui, |ui| {
        let mut c = postfx.fog_color;
        if ui.color_edit_button_rgb(&mut c).changed() { postfx.fog_color = c; }
        ui.add(egui::Slider::new(&mut postfx.fog_density, 0.0..=0.2).text("Density"));
        ui.add(egui::Slider::new(&mut postfx.fog_height_base, -10.0..=20.0).text("Height base"));
        ui.add(egui::Slider::new(&mut postfx.fog_height_falloff, 0.0..=0.5).text("Height falloff"));
    });
    ui.separator();
    egui::CollapsingHeader::new("Volumetric fog").default_open(true).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.volumetric_density, 0.0..=0.02).logarithmic(true).text("Density"));
        ui.label(egui::RichText::new("0 = выключено. 0.02..0.05 — плотный туман с god rays.").small().weak());
        ui.add(egui::Slider::new(&mut postfx.volumetric_scattering, 0.0..=1.0).text("Scattering (albedo)"));
        ui.add(egui::Slider::new(&mut postfx.volumetric_phase_g, 0.0..=0.9).text("Phase g (god rays)"));
        ui.label(egui::RichText::new("Больше g → сильнее forward scattering.").small().weak());
    });
    ui.separator();
    egui::CollapsingHeader::new("Screen effects").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.vignette_strength, 0.0..=1.0).text("Vignette"));
        ui.add(egui::Slider::new(&mut postfx.film_grain, 0.0..=1.0).text("Film grain"));
        ui.add(egui::Slider::new(&mut postfx.chromatic_aberration, 0.0..=1.0).text("Chromatic ab."));
    });
    ui.separator();
    egui::CollapsingHeader::new("Shadows").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.shadow_bias, 0.0..=0.01).text("Depth bias"));
        ui.add(egui::Slider::new(&mut postfx.shadow_normal_bias, 0.0..=10.0).text("Slope bias"));
        ui.add(egui::Slider::new(&mut postfx.shadow_fade_start, 50.0..=250.0).text("Fade start"));
        ui.add(egui::Slider::new(&mut postfx.shadow_fade_end, 60.0..=300.0).text("Fade end"));
    });
    ui.separator();
    egui::CollapsingHeader::new("LOD").default_open(false).show(ui, |ui| {
        ui.add(egui::Slider::new(&mut postfx.lod_bias, 0.2..=4.0).logarithmic(true).text("LOD bias"));
        ui.add(egui::Slider::new(&mut postfx.lod_distances[0], 5.0..=200.0).text("Distance LOD0→1"));
        ui.add(egui::Slider::new(&mut postfx.lod_distances[1], 20.0..=500.0).text("Distance LOD1→2"));
        ui.add(egui::Slider::new(&mut postfx.lod_distances[2], 50.0..=1000.0).text("Distance LOD2→3"));

        let d = &mut postfx.lod_distances;
        if d[1] <= d[0] { d[1] = d[0] + 1.0; }
        if d[2] <= d[1] { d[2] = d[1] + 1.0; }
    });
    ui.separator();
    ui.label("Debug view");
    ui.horizontal_wrapped(|ui| {
        debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::Final, "Final");
        debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::Ssao, "SSAO");
        debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::GbufferNormal, "Normal");
        debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::GbufferDepth, "Depth");
        debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::HdrPreBloom, "HDR");
        debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::CsmCascade0, "CSM");
    });
    ui.separator();
    draw_play_settings(ui, editor);
    ui.separator();
    draw_fly_settings(ui, editor);
}

fn draw_play_settings(ui: &mut egui::Ui, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Play").default_open(false).show(ui, |ui| {
        let p = &mut editor.play;

        ui.label(egui::RichText::new("Movement").strong());
        ui.add(egui::Slider::new(&mut p.walk_speed, 0.5..=20.0).text("Walk speed"));
        ui.add(egui::Slider::new(&mut p.run_speed, 1.0..=40.0).text("Run speed"));
        ui.add(egui::Slider::new(&mut p.jump_speed, 1.0..=20.0).text("Jump"));
        ui.add(egui::Slider::new(&mut p.gravity, 1.0..=60.0).text("Gravity"));
        ui.add(egui::Slider::new(&mut p.eye_height, 0.5..=3.0).text("Eye height"));
        ui.add(egui::Slider::new(&mut p.look_sensitivity, 0.0005..=0.01).text("Look sensitivity"));

        ui.separator();
        ui.label(egui::RichText::new("Feel").strong());
        ui.add(egui::Slider::new(&mut p.coyote_time, 0.0..=0.5)
            .text("Coyote time (s)")
            .fixed_decimals(2))
            .on_hover_text(
                "Время после схода с платформы, когда прыжок ещё срабатывает. \
                 0.1-0.2 — стандарт для платформеров."
            );
        ui.add(egui::Slider::new(&mut p.jump_buffer_time, 0.0..=0.5)
            .text("Jump buffer (s)")
            .fixed_decimals(2))
            .on_hover_text(
                "Если игрок нажал прыжок за N секунд до касания земли — \
                 прыжок сработает автоматически при контакте."
            );
        ui.add(egui::Slider::new(&mut p.ground_accel_tau, 0.01..=0.3)
            .text("Ground accel τ (s)")
            .fixed_decimals(3))
            .on_hover_text("Меньше = резче разгон/торможение на земле.");
        ui.add(egui::Slider::new(&mut p.air_accel_tau, 0.05..=1.0)
            .text("Air accel τ (s)")
            .fixed_decimals(2))
            .on_hover_text("Больше = меньше контроля в воздухе.");

        ui.separator();
        ui.label(egui::RichText::new("Slope / step").strong());
        ui.add(egui::Slider::new(&mut p.step_down_max, 0.0..=1.5)
            .text("Step-down max (m)")
            .fixed_decimals(2))
            .on_hover_text(
                "Максимальная высота уступа, к которому персонаж \
                 «прилипает» при спуске. 0.0 — выключает step-down."
            );
        let mut walk_deg = p.slope_walk_limit_cos.acos().to_degrees();
        let r = ui.add(egui::Slider::new(&mut walk_deg, 10.0..=80.0)
            .text("Walkable slope (°)")
            .fixed_decimals(0));
        if r.changed() {
            p.slope_walk_limit_cos = walk_deg.to_radians().cos();
        }
        ui.add(egui::Slider::new(&mut p.slope_slide_speed, 0.0..=20.0)
            .text("Slope slide speed (m/s)")
            .fixed_decimals(1))
            .on_hover_text("Скорость скольжения по вертикальной стене.");
        if p.ground_normal.y < 0.999 {
            ui.label(egui::RichText::new(format!(
                "ground normal: ({:.2}, {:.2}, {:.2})",
                p.ground_normal.x, p.ground_normal.y, p.ground_normal.z,
            )).small().weak().monospace());
        }

        ui.separator();
        ui.label("Player capsule");
        ui.add(egui::Slider::new(&mut p.player_radius, 0.1..=1.0).text("Radius"));
        ui.add(egui::Slider::new(&mut p.player_height, 0.5..=3.0).text("Height"));

        ui.separator();
        ui.label("Crouch (Ctrl)");
        ui.add(egui::Slider::new(&mut p.crouch_height, 0.5..=1.7).text("Eye height"));
        ui.add(egui::Slider::new(&mut p.crouch_speed_mult, 0.1..=1.0).text("Speed mult"));

        ui.separator();
        ui.label("Combat");
        ui.add(egui::Slider::new(&mut p.max_health, 10.0..=500.0).text("Max HP"));
        ui.add(egui::Slider::new(&mut p.max_ammo, 0..=500).text("Max ammo"));
        ui.add(egui::Slider::new(&mut p.damage_per_shot, 1.0..=200.0).text("Damage"));
        ui.add(egui::Slider::new(&mut p.gun_range, 5.0..=500.0).text("Range"));
        ui.add(egui::Slider::new(&mut p.bullet_speed, 0.0..=300.0).text("Bullet speed"));
        ui.add(egui::Slider::new(&mut p.fire_cooldown_max, 0.02..=1.0).text("Fire cd"));
        ui.add(egui::Slider::new(&mut p.interact_distance, 1.0..=20.0).text("Interact dist"));

        ui.separator();
        ui.checkbox(&mut p.bob_enabled, "Head bob");
        ui.add(egui::Slider::new(&mut p.bob_amplitude, 0.0..=0.15).text("Bob amplitude"));

        ui.separator();
        ui.checkbox(&mut p.show_crosshair, "Show crosshair");
        ui.checkbox(&mut p.show_hud, "Show HUD");
        ui.checkbox(&mut p.show_health, "Show health bar");
        ui.checkbox(&mut p.show_ammo, "Show ammo bar");
    });
}

fn draw_fly_settings(ui: &mut egui::Ui, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Fly (RMB)").default_open(false).show(ui, |ui| {
        ui.label("WASD — move, E/Q or Space — up/down");
        ui.label("Shift — faster, Ctrl — slower");
        ui.label("Scroll during fly — fly speed");
        ui.separator();
        ui.add(egui::Slider::new(&mut editor.fly_speed, 1.0..=100.0).text("Fly speed"));
        ui.add(egui::Slider::new(&mut editor.fly_sensitivity, 0.0005..=0.01).text("Fly sensitivity"));
    });
}

fn draw_multi_edit(ui: &mut egui::Ui, world: &mut World, editor: &mut EditorState) {
    let selected: Vec<Entity> = editor.selected.clone();
    let with_tf: Vec<(Entity, Transform)> = selected.iter()
        .filter_map(|&e| world.get::<Transform>(e).map(|t| (e, *t))).collect();
    if with_tf.is_empty() {
        ui.label(egui::RichText::new("No Transform components in selection").weak().italics());
        return;
    }
    ui.label(egui::RichText::new(format!("Group Transform ({} objects)", with_tf.len())).strong());

    let positions: Vec<Vec3> = with_tf.iter().map(|(_, t)| t.position).collect();
    let pos_common = common_vec3(positions.iter().copied());
    let mixed_pos = pos_common.is_none();
    let mut pos = pos_common.unwrap_or(Vec3::ZERO).to_array();
    ui.label(if mixed_pos { egui::RichText::new("Position (mixed)").weak() } else { egui::RichText::new("Position") });
    let mut pos_changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(egui::DragValue::new(&mut pos[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i]));
            if r.changed() { pos_changed = true; }
            if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        }
    });
    if pos_changed {
        let new_pos = Vec3::from_array(pos);
        for (e, _) in &with_tf { if let Some(t) = world.get_mut::<Transform>(*e) { t.position = new_pos; } }
    }

    let eulers: Vec<Vec3> = with_tf.iter().map(|(_, t)| {
        let (y, x, z) = t.rotation.to_euler(glam::EulerRot::YXZ);
        Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
    }).collect();
    let rot_common = common_vec3(eulers.iter().copied());
    let mixed_rot = rot_common.is_none();
    let mut rot = rot_common.unwrap_or(Vec3::ZERO).to_array();
    ui.label(if mixed_rot { egui::RichText::new("Rotation (deg, mixed)").weak() } else { egui::RichText::new("Rotation (deg)") });
    let mut rot_changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(egui::DragValue::new(&mut rot[i]).speed(0.5).prefix(["X ", "Y ", "Z "][i]));
            if r.changed() { rot_changed = true; }
            if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        }
    });
    if rot_changed {
        let q = glam::Quat::from_euler(glam::EulerRot::YXZ,
            rot[1].to_radians(), rot[0].to_radians(), rot[2].to_radians());
        for (e, _) in &with_tf { if let Some(t) = world.get_mut::<Transform>(*e) { t.rotation = q; } }
    }

    let scales: Vec<Vec3> = with_tf.iter().map(|(_, t)| t.scale).collect();
    let scale_common = common_vec3(scales.iter().copied());
    let mixed_scale = scale_common.is_none();
    let mut scale = scale_common.unwrap_or(Vec3::ONE).to_array();
    ui.label(if mixed_scale { egui::RichText::new("Scale (mixed)").weak() } else { egui::RichText::new("Scale") });
    let mut scale_changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(egui::DragValue::new(&mut scale[i]).speed(0.01).prefix(["X ", "Y ", "Z "][i]));
            if r.changed() { scale_changed = true; }
            if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
        }
    });
    if scale_changed {
        let new_scale = Vec3::from_array(scale);
        for (e, _) in &with_tf { if let Some(t) = world.get_mut::<Transform>(*e) { t.scale = new_scale; } }
    }

    let tiled: Vec<(Entity, f32)> = selected.iter()
        .filter_map(|&e| world.get::<TextureTiling>(e).map(|t| (e, t.size))).collect();
    if tiled.is_empty() { return; }
    ui.separator();
    ui.label(egui::RichText::new(format!("Texture Tiling ({} objects)", tiled.len())).strong());
    let common_size = common_vec3(tiled.iter().map(|(_, s)| Vec3::splat(*s)));
    let mixed = common_size.is_none();
    let mut size = common_size.unwrap_or(Vec3::ONE).x;
    ui.label(if mixed { egui::RichText::new("Tile size (mixed)").weak() } else { egui::RichText::new("Tile size (world units)") });
    let r = ui.add(egui::Slider::new(&mut size, 0.05..=50.0).logarithmic(true));
    if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
    if r.changed() {
        for &(e, _) in &tiled { if let Some(t) = world.get_mut::<TextureTiling>(e) { t.size = size; } }
    }
}

fn common_vec3<I: Iterator<Item = Vec3>>(mut it: I) -> Option<Vec3> {
    let first = it.next()?;
    for v in it { if (v - first).length() > 1e-4 { return None; } }
    Some(first)
}

#[allow(dead_code)]
fn draw_play_hud(_ctx: &egui::Context, _play: &PlayState, _stats: &Stats) {
    // Отключено: runtime UI из Game::collect_ui рисует crosshair/health/ammo.
}

fn component_badges(world: &World, e: Entity) -> String {
    let mut s = String::with_capacity(24);
    if world.has::<Transform>(e) { s.push_str("T "); }
    if world.has::<MeshHandle>(e) { s.push_str("M "); }
    if world.has::<MaterialHandle>(e) { s.push_str("Mat "); }
    if world.has::<Parent>(e) { s.push_str("P "); }
    if world.has::<SkeletonHandle>(e) { s.push_str("Sk "); }
    if world.has::<AnimationPlayer>(e) { s.push_str("A "); }
    if world.has::<Spinner>(e) { s.push_str("Sp "); }
    if world.has::<Velocity>(e) { s.push_str("V "); }
    if world.has::<Tint>(e) { s.push_str("Ti "); }
    if world.has::<Visible>(e) { s.push_str("Vi "); }
    if world.has::<TextureTiling>(e) { s.push_str("Tt "); }
    if world.has::<Elevator>(e) { s.push_str("El "); }
    if world.has::<SlidingDoor>(e) { s.push_str("SD "); }
    if world.has::<Health>(e) { s.push_str("H "); }
    if world.has::<Chase>(e) { s.push_str("Ch "); }
    if world.has::<Interactable>(e) { s.push_str("In "); }
    if world.has::<Trigger>(e) { s.push_str("Tr "); }
    if world.has::<RigidBody>(e) { s.push_str("Ph "); }
    if world.has::<Collider>(e) { s.push_str("C "); }
    if world.has::<PhysicsMaterial>(e) { s.push_str("PM "); }
    if world.has::<DirectionalLight>(e) { s.push_str("☀ "); }
    if world.has::<PointLight>(e) { s.push_str("💡 "); }
    if world.has::<crate::game::decals::Decal>(e) { s.push_str("🎨 "); }
    if world.has::<AudioSource>(e) { s.push_str("🔊 "); }
    if world.has::<crate::game::ai::AiAgent>(e) { s.push_str("🤖 "); }
    if world.has::<crate::game::ai::Enemy>(e) { s.push_str("👹 "); }
    if world.has::<crate::game::ai::AiTarget>(e) { s.push_str("🎯 "); }

    if world.has::<Parent>(e) {
        if let Some(rb) = world.get::<RigidBody>(e) {
            if rb.body_type != BodyType::Static {
                s.push_str("⚠ ");
            }
        }
    }

    s.trim_end().to_string()
}

fn entity_display_name(world: &World, e: Entity) -> String {
    if let Some(n) = world.get::<Name>(e) { return n.0.clone(); }
    let mesh = world.get::<MeshHandle>(e).map(|m| m.0.as_str()).unwrap_or("?");
    format!("#{} {}", e, mesh)
}

fn would_create_cycle(world: &World, child: Entity, new_parent: Entity) -> bool {
    let mut cur = new_parent;
    for _ in 0..64 {
        if cur == child { return true; }
        match world.get::<Parent>(cur) {
            Some(&Parent(p)) => cur = p,
            None => return false,
        }
    }
    true
}

fn texture_picker(
    ui: &mut egui::Ui,
    id: &str,
    label: &str,
    current: &mut Option<String>,
    textures: &[(String, u32, u32, bool)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        let display = current.as_deref().unwrap_or("(none)");
        egui::ComboBox::from_id_salt(id).selected_text(display).width(170.0).show_ui(ui, |ui| {
            if ui.selectable_label(current.is_none(), "(none)").clicked() {
                *current = None;
                changed = true;
            }
            if textures.is_empty() {
                ui.label(egui::RichText::new("(no textures loaded)").weak().italics());
            }
            for (name, w, h, is_srgb) in textures {
                let selected = current.as_deref() == Some(name.as_str());

                let tag = if *is_srgb { "[sRGB]" } else { "[Linear]" };
                let tag_color = if *is_srgb {
                    egui::Color32::from_rgb(230, 200, 130)
                } else {
                    egui::Color32::from_rgb(150, 200, 230)
                };

                ui.horizontal(|ui| {
                    ui.label(
                        egui::RichText::new(tag)
                            .small()
                            .monospace()
                            .color(tag_color),
                    );
                    let resp = ui.selectable_label(selected, name);
                    if resp
                        .on_hover_text(format!(
                            "{}×{} pixels\nLoaded as: {}",
                            w, h,
                            if *is_srgb { "sRGB (color)" } else { "Linear (data)" },
                        ))
                        .clicked()
                    {
                        *current = Some(name.clone());
                        changed = true;
                    }
                });
            }
        });
    });
    changed
}

fn brush_button(ui: &mut egui::Ui, palette: &mut PaletteState, item: PaletteItem) {
    let selected = palette.active == Some(item);
    if ui.selectable_label(selected, item.label()).on_hover_text(format!("Click to place {}", item.label())).clicked() {
        palette.active = if selected { None } else { Some(item) };
    }
}

fn debug_button(ui: &mut egui::Ui, current: &mut crate::render::DebugView,
    target: crate::render::DebugView, label: &str) {
    if ui.selectable_label(*current == target, label).clicked() { *current = target; }
}