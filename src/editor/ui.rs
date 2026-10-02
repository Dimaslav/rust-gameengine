//! Панели редактора.

use crate::ecs::{Entity, World};
use crate::editor::gizmo::GizmoMode;
use crate::editor::palette::{PaletteItem, PaletteState};
use crate::editor::play::PlayState;
use crate::editor::{EditorAction, EditorState};
use crate::game::components::{
    AnimationPlayer, Chase, Health, Interactable, MaterialHandle, MeshHandle, Name, Parent,
    SkeletonHandle, Spinner, Tint, Transform, Trigger, TriggerAction, Velocity, Visible,
};
use crate::physics::{BodyType, Collider, PhysicsMaterial, RigidBody};
use crate::render::{AlphaMode, Material, PostFx};
use glam::Vec3;

#[derive(Default)]
pub struct UiState {
    pub show_renderer_panel: bool,
    pub show_stats_panel: bool,
    pub show_hierarchy_panel: bool,
    pub show_inspector_panel: bool,
}

impl UiState {
    pub fn new() -> Self {
        Self {
            show_renderer_panel: true,
            show_stats_panel: true,
            show_hierarchy_panel: true,
            show_inspector_panel: true,
        }
    }
}

pub struct Stats {
    pub fps: f32,
    pub frame_time_max_ms: f32,
    pub hitches: u64,
    pub entities: usize,
    pub draws: usize,
    pub instances: usize,
    pub dir_lights: usize,
    pub point_lights: usize,
    // === LOD ===
    pub lod_counts: [usize; 4],
    pub lod_triangles: [usize; 4],
}

pub struct UiAssets<'a> {
    pub mesh_names: &'a [String],
    pub material_names: &'a [String],
    /// Список текстур: (имя, ширина, высота). Уже отсортирован.
    pub texture_list: &'a [(String, u32, u32)],
    pub selected_material: Option<(String, Material)>,
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut UiState,
    editor: &mut EditorState,
    world: &mut World,
    postfx: &mut PostFx,
    stats: &Stats,
    assets: &UiAssets<'_>,
) -> Option<EditorAction> {
    let mut action: Option<EditorAction> = None;

    if editor.play.active {
        draw_play_hud(ctx, &editor.play, stats.fps);
    }

    // ===== Box-select overlay =====
    if let Some(bs) = editor.box_select {
        let ppp = ctx.pixels_per_point();
        let rect = egui::Rect::from_two_pos(
            egui::pos2(bs.start.0 / ppp, bs.start.1 / ppp),
            egui::pos2(bs.current.0 / ppp, bs.current.1 / ppp),
        );
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("box_select_layer"),
        ));
        painter.rect_filled(
            rect,
            egui::Rounding::ZERO,
            egui::Color32::from_rgba_unmultiplied(90, 180, 255, 32),
        );
        painter.rect_stroke(
            rect,
            egui::Rounding::ZERO,
            egui::Stroke::new(
                1.5_f32,
                egui::Color32::from_rgba_unmultiplied(120, 200, 255, 230),
            ),
        );
    }

    // ===== Контекстное меню viewport =====
    if let Some(pos) = editor.context_menu_pos {
        let ppp = ctx.pixels_per_point();
        let p = egui::pos2(pos.0 / ppp, pos.1 / ppp);

        let area = egui::Area::new(egui::Id::new("viewport_ctx_menu"))
            .fixed_pos(p)
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(180.0);
                    let has_sel = !editor.selected.is_empty();

                    if ui.add_enabled(has_sel, egui::Button::new("Focus (F)")).clicked() {
                        action = Some(EditorAction::FocusSelected);
                        editor.context_menu_pos = None;
                    }
                    if ui
                        .add_enabled(has_sel, egui::Button::new("Duplicate (Ctrl+D)"))
                        .clicked()
                    {
                        action = Some(EditorAction::Duplicate);
                        editor.context_menu_pos = None;
                    }
                    if ui.add_enabled(has_sel, egui::Button::new("Delete")).clicked() {
                        action = Some(EditorAction::DeleteSelected);
                        editor.context_menu_pos = None;
                    }
                    ui.separator();
                    if ui.add_enabled(has_sel, egui::Button::new("Copy (Ctrl+C)")).clicked() {
                        action = Some(EditorAction::CopyEntity);
                        editor.context_menu_pos = None;
                    }
                    if ui
                        .add_enabled(
                            !editor.clipboard_entities.is_empty(),
                            egui::Button::new("Paste (Ctrl+V)"),
                        )
                        .clicked()
                    {
                        action = Some(EditorAction::PasteEntity);
                        editor.context_menu_pos = None;
                    }
                    ui.separator();
                    if ui
                        .add_enabled(has_sel, egui::Button::new("Make Material Unique"))
                        .clicked()
                    {
                        action = Some(EditorAction::MakeMaterialUnique);
                        editor.context_menu_pos = None;
                    }
                    ui.separator();
                    if ui
                        .add_enabled(has_sel, egui::Button::new("Export FBX (selection)"))
                        .clicked()
                    {
                        action = Some(EditorAction::ExportFbxSelected);
                        editor.context_menu_pos = None;
                    }
                    if ui.button("Import FBX…").clicked() {
                        action = Some(EditorAction::ImportFbx);
                        editor.context_menu_pos = None;
                    }
                });
            });

        let menu_rect = area.response.rect;
        let clicked_outside = ctx.input(|i| {
            let primary = i.pointer.primary_clicked();
            let pos = i.pointer.interact_pos();
            match (primary, pos) {
                (true, Some(p)) => !menu_rect.contains(p),
                _ => false,
            }
        });
        let esc = ctx.input(|i| i.key_pressed(egui::Key::Escape));
        if clicked_outside || esc {
            editor.context_menu_pos = None;
        }
    }

    // ===== Верхняя панель =====
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Rust Engine 3D");
            ui.separator();

            if ui
                .add_enabled(editor.undo.can_undo(), egui::Button::new("↶ Undo"))
                .on_hover_text("Ctrl+Z")
                .clicked()
            {
                action = Some(EditorAction::Undo);
            }
            if ui
                .add_enabled(editor.undo.can_redo(), egui::Button::new("↷ Redo"))
                .on_hover_text("Ctrl+Y")
                .clicked()
            {
                action = Some(EditorAction::Redo);
            }

            ui.separator();

            if editor.play.active {
                if ui.button("■ Stop").on_hover_text("Exit play mode (Esc)").clicked() {
                    action = Some(EditorAction::TogglePlay);
                }
            } else if ui.button("▶ Play").on_hover_text("Run game (F9)").clicked() {
                action = Some(EditorAction::TogglePlay);
            }

            ui.separator();

            if !editor.play.active {
                let m = &mut editor.gizmo.mode;
                if ui.selectable_label(*m == GizmoMode::Translate, "T").on_hover_text("Translate (1)").clicked() {
                    *m = GizmoMode::Translate;
                }
                if ui.selectable_label(*m == GizmoMode::Rotate, "R").on_hover_text("Rotate (2)").clicked() {
                    *m = GizmoMode::Rotate;
                }
                if ui.selectable_label(*m == GizmoMode::Scale, "S").on_hover_text("Scale (3)").clicked() {
                    *m = GizmoMode::Scale;
                }
                let snap_resp = ui.toggle_value(&mut editor.gizmo.snap_enabled, "Snap");
                snap_resp.on_hover_text("Snap без Ctrl (0.5 м / 15°)");
                ui.separator();
            }

            if ui
                .add_enabled(
                    !editor.selected.is_empty(),
                    egui::Button::new("Copy (Ctrl+C)"),
                )
                .clicked()
            {
                action = Some(EditorAction::CopyEntity);
            }
            if ui
                .add_enabled(
                    !editor.clipboard_entities.is_empty(),
                    egui::Button::new("Paste (Ctrl+V)"),
                )
                .clicked()
            {
                action = Some(EditorAction::PasteEntity);
            }

            ui.separator();
            if ui.button("New").on_hover_text("New empty scene").clicked() {
                action = Some(EditorAction::NewScene);
            }
            if ui.button("Save").clicked() {
                action = Some(EditorAction::Save);
            }
            if ui.button("Load").clicked() {
                action = Some(EditorAction::Load);
            }
            ui.add(
                egui::TextEdit::singleline(&mut editor.save_path)
                    .desired_width(140.0)
                    .hint_text("scene.ron"),
            );

            ui.separator();
            if ui.button("FBX All").on_hover_text("Экспорт всей сцены в .fbx").clicked() {
                action = Some(EditorAction::ExportFbxAll);
            }
            if ui
                .add_enabled(!editor.selected.is_empty(), egui::Button::new("FBX Sel"))
                .on_hover_text("Экспорт выделения в .fbx")
                .clicked()
            {
                action = Some(EditorAction::ExportFbxSelected);
            }
            if ui.button("FBX Import").on_hover_text("Импорт ASCII или binary FBX").clicked() {
                action = Some(EditorAction::ImportFbx);
            }

            ui.separator();
            ui.label(format!("FPS: {:.1}", stats.fps));
            ui.separator();
            ui.label(format!("Entities: {}", stats.entities));

            if editor.play.active {
                ui.separator();
                ui.label(
                    egui::RichText::new("● PLAY")
                        .strong()
                        .color(egui::Color32::from_rgb(230, 90, 90)),
                );
            } else if editor.flying {
                ui.separator();
                ui.label(
                    egui::RichText::new("✦ FLY")
                        .strong()
                        .color(egui::Color32::from_rgb(90, 180, 230)),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut state.show_renderer_panel, "Renderer");
                ui.toggle_value(&mut state.show_inspector_panel, "Inspector");
                ui.toggle_value(&mut state.show_hierarchy_panel, "Hierarchy");
                ui.toggle_value(&mut state.show_stats_panel, "Stats");
            });
        });
    });

    // ===== Палитра =====
    if !editor.play.active {
        egui::TopBottomPanel::top("palette_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("🖌 Brush:").strong());
                for &item in PaletteItem::all_primitives() {
                    brush_button(ui, &mut editor.palette, item);
                }
                ui.separator();
                ui.label("Presets:");
                for &item in PaletteItem::all_presets() {
                    brush_button(ui, &mut editor.palette, item);
                }
                ui.separator();

                let active = editor.palette.active;
                if active.is_some() {
                    if ui.button("✖ Clear (Esc)").on_hover_text("Снять кисть").clicked() {
                        editor.palette.active = None;
                    }
                    ui.checkbox(&mut editor.palette.keep_active, "Keep");
                    let snap_resp = ui.checkbox(&mut editor.palette.snap_to_grid, "Snap");
                    if snap_resp.hovered() {
                        snap_resp.on_hover_text("Ctrl+клик форсирует snap");
                    }
                    ui.add(
                        egui::DragValue::new(&mut editor.palette.grid_step)
                            .speed(0.01)
                            .range(0.05..=10.0)
                            .prefix("step: "),
                    );
                } else {
                    ui.label(egui::RichText::new("no brush").weak().italics());
                    ui.label(
                        egui::RichText::new("— выбери примитив и кликай по земле")
                            .weak()
                            .small(),
                    );
                }
            });
        });
    }

    if !editor.play.active {
        // ===== Левая панель =====
        if state.show_stats_panel || state.show_hierarchy_panel {
            egui::SidePanel::left("left_panel").default_width(260.0).show(ctx, |ui| {
                if state.show_stats_panel {
                    ui.heading("Stats");
                    ui.separator();
                    ui.label(format!("FPS: {:.1}", stats.fps));
                    ui.label(format!(
                        "Frame: {:.2} ms",
                        if stats.fps > 0.1 { 1000.0 / stats.fps } else { 0.0 }
                    ));
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
                    ui.label("LOD");
                    ui.label(format!(
                        "LOD0: {} · LOD1: {} · LOD2: {} · LOD3: {}",
                        stats.lod_counts[0],
                        stats.lod_counts[1],
                        stats.lod_counts[2],
                        stats.lod_counts[3],
                    ));
                }

                if state.show_hierarchy_panel {
                    ui.separator();
                    ui.heading("Hierarchy");
                    ui.separator();

                    ui.horizontal(|ui| {
                        if ui.button("+ Cube").clicked() {
                            action = Some(EditorAction::AddCube);
                        }
                        if ui.button("+ Sphere").clicked() {
                            action = Some(EditorAction::AddSphere);
                        }
                    });
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !editor.selected.is_empty(),
                                egui::Button::new("Duplicate (Ctrl+D)"),
                            )
                            .clicked()
                        {
                            action = Some(EditorAction::Duplicate);
                        }
                        if ui
                            .add_enabled(!editor.selected.is_empty(), egui::Button::new("Delete"))
                            .clicked()
                        {
                            action = Some(EditorAction::DeleteSelected);
                        }
                    });
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("🔍");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.search_filter)
                                .desired_width(f32::INFINITY)
                                .hint_text("filter by name…"),
                        );
                    });
                    ui.separator();

                    let filter_lower = editor.search_filter.to_lowercase();
                    let all_entities: Vec<Entity> = world.entities().to_vec();
                    let filtered: Vec<Entity> = all_entities
                        .into_iter()
                        .filter(|&e| {
                            if filter_lower.is_empty() {
                                return true;
                            }
                            entity_display_name(world, e)
                                .to_lowercase()
                                .contains(&filter_lower)
                        })
                        .collect();

                    let total = filtered.len();
                    let sel_count = editor.selected.len();
                    if sel_count > 1 {
                        ui.label(format!("{} shown · {} selected", total, sel_count));
                    } else {
                        ui.label(format!("{} shown", total));
                    }

                    let anchor = editor.primary();

                    let row_height = 22.0;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_height(300.0)
                        .show_rows(ui, row_height, total, |ui, row_range| {
                            for i in row_range {
                                let e = filtered[i];
                                let is_selected = editor.is_selected(e);

                                ui.horizontal(|ui| {
                                    // Eye icon
                                    let visible = world
                                        .get::<Visible>(e)
                                        .map(|v| v.0)
                                        .unwrap_or(true);
                                    let eye_label = if visible { "👁" } else { "✖" };
                                    let eye_resp = ui
                                        .add(egui::Button::new(eye_label).frame(false))
                                        .on_hover_text("Toggle visibility");
                                    if eye_resp.clicked() {
                                        editor.undo_requested = true;
                                        if visible {
                                            world.insert(e, Visible(false));
                                        } else {
                                            world.remove::<Visible>(e);
                                        }
                                    }

                                    // Rename mode
                                    if editor.renaming == Some(e) {
                                        let response = ui.add(
                                            egui::TextEdit::singleline(&mut editor.rename_buffer)
                                                .desired_width(160.0),
                                        );
                                        response.request_focus();

                                        let enter =
                                            ui.input(|i| i.key_pressed(egui::Key::Enter));
                                        let esc =
                                            ui.input(|i| i.key_pressed(egui::Key::Escape));

                                        if enter || response.lost_focus() {
                                            let new_name =
                                                editor.rename_buffer.trim().to_string();
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
                                    } else {
                                        let label = entity_display_name(world, e);
                                        let drag_id = egui::Id::new(("hier_drag", e));
                                        let inner = ui.dnd_drag_source(drag_id, e, |ui| {
                                            ui.selectable_label(is_selected, &label)
                                        });
                                        let label_resp = inner.inner;
                                        let drag_resp = inner.response;

                                        if label_resp.clicked() {
                                            let modifiers = ui.input(|i| i.modifiers);
                                            if modifiers.ctrl || modifiers.command {
                                                editor.toggle_select(e);
                                            } else if modifiers.shift {
                                                let anchor_e = anchor.unwrap_or(e);
                                                editor.select_range(&filtered, anchor_e, e);
                                            } else {
                                                editor.select_single(e);
                                            }
                                        }
                                        if label_resp.double_clicked() {
                                            editor.select_single(e);
                                            editor.renaming = Some(e);
                                            editor.rename_buffer = world
                                                .get::<Name>(e)
                                                .map(|n| n.0.clone())
                                                .unwrap_or_else(|| {
                                                    entity_display_name(world, e)
                                                });
                                        }

                                        if drag_resp.dnd_hover_payload::<Entity>().is_some() {
                                            ui.painter().rect_stroke(
                                                drag_resp.rect,
                                                egui::Rounding::same(2.0),
                                                egui::Stroke::new(
                                                    2.0_f32,
                                                    egui::Color32::from_rgb(255, 210, 90),
                                                ),
                                            );
                                        }

                                        if let Some(child_arc) =
                                            drag_resp.dnd_release_payload::<Entity>()
                                        {
                                            let child = *child_arc;
                                            if child != e && !would_create_cycle(world, child, e) {
                                                editor.undo_requested = true;
                                                world.insert(child, Parent(e));
                                            }
                                        }

                                        label_resp.context_menu(|ui| {
                                            if ui.button("Rename").clicked() {
                                                editor.renaming = Some(e);
                                                editor.rename_buffer = world
                                                    .get::<Name>(e)
                                                    .map(|n| n.0.clone())
                                                    .unwrap_or_else(|| {
                                                        entity_display_name(world, e)
                                                    });
                                                ui.close_menu();
                                            }
                                            if ui.button("Focus (F)").clicked() {
                                                editor.select_single(e);
                                                action = Some(EditorAction::FocusSelected);
                                                ui.close_menu();
                                            }
                                            if ui.button("Duplicate").clicked() {
                                                editor.select_single(e);
                                                action = Some(EditorAction::Duplicate);
                                                ui.close_menu();
                                            }
                                            if ui.button("Delete").clicked() {
                                                editor.select_single(e);
                                                action = Some(EditorAction::DeleteSelected);
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                            if ui.button("Export FBX…").clicked() {
                                                editor.select_single(e);
                                                action = Some(EditorAction::ExportFbxSelected);
                                                ui.close_menu();
                                            }
                                            ui.separator();
                                            if world.has::<Parent>(e) {
                                                if ui.button("Clear Parent").clicked() {
                                                    editor.undo_requested = true;
                                                    world.remove::<Parent>(e);
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                    }
                                });
                            }
                        });

                    // Prefabs
                    ui.separator();
                    ui.heading("Prefabs");
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("📁");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.prefabs_dir)
                                .desired_width(140.0)
                                .hint_text("prefabs"),
                        );
                        if ui.small_button("⟳").on_hover_text("Refresh list").clicked() {
                            action = Some(EditorAction::RefreshPrefabs);
                        }
                    });

                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.add(
                            egui::TextEdit::singleline(&mut editor.prefab_save_name)
                                .desired_width(120.0)
                                .hint_text("my_prefab"),
                        );
                        let can_save = !editor.selected.is_empty()
                            && !editor.prefab_save_name.trim().is_empty();
                        if ui
                            .add_enabled(can_save, egui::Button::new("💾 Save Sel"))
                            .on_hover_text("Сохранить выделение как .prefab.ron")
                            .clicked()
                        {
                            action = Some(EditorAction::SavePrefab);
                        }
                    });

                    ui.separator();

                    let prefab_count = editor.prefab_list.len();
                    if prefab_count == 0 {
                        ui.label(egui::RichText::new("no prefabs found").weak().italics());
                        ui.label(
                            egui::RichText::new("Выдели объекты → введи имя → Save Sel")
                                .small()
                                .weak(),
                        );
                    } else {
                        ui.label(format!("{} files", prefab_count));
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for (idx, path) in editor.prefab_list.iter().enumerate() {
                                    let display = crate::scene::prefab::prefab_display_name(path);
                                    let resp = ui
                                        .button(format!("📦 {}", display))
                                        .on_hover_text(format!("Spawn {}", path.display()));
                                    if resp.clicked() {
                                        action = Some(EditorAction::InstantiatePrefab(idx as u32));
                                    }
                                    resp.context_menu(|ui| {
                                        if ui.button("Spawn here").clicked() {
                                            action = Some(EditorAction::InstantiatePrefab(
                                                idx as u32,
                                            ));
                                            ui.close_menu();
                                        }
                                        if ui.button("Show path").clicked() {
                                            log::info!("{}", path.display());
                                            ui.close_menu();
                                        }
                                    });
                                }
                            });
                    }

                    // Assets
                    ui.separator();
                    ui.heading("Assets");
                    ui.separator();

                    if ui
                        .button("➕ Load Texture(s)…")
                        .on_hover_text(
                            "PNG, JPEG, GIF, WebP, BMP, TIFF, TGA, DDS, \
                             HDR, EXR, ICO, PNM, QOI, Farbfeld",
                        )
                        .clicked()
                    {
                        action = Some(EditorAction::LoadTextures);
                    }

                    ui.separator();

                    let tex_count = assets.texture_list.len();
                    if tex_count == 0 {
                        ui.label(egui::RichText::new("no textures loaded").weak().italics());
                    } else {
                        ui.label(format!("{} textures", tex_count));
                        egui::ScrollArea::vertical()
                            .auto_shrink([false; 2])
                            .max_height(180.0)
                            .show(ui, |ui| {
                                for (name, w, h) in assets.texture_list {
                                    let resp = ui
                                        .button(format!("🖼 {}", name))
                                        .on_hover_text(format!(
                                            "{} × {} pixels\nRight-click to remove",
                                            w, h
                                        ));
                                    resp.context_menu(|ui| {
                                        if ui.button("Remove texture").clicked() {
                                            action = Some(EditorAction::RemoveTexture(
                                                name.clone(),
                                            ));
                                            ui.close_menu();
                                        }
                                        if ui.button("Copy name").clicked() {
                                            ui.output_mut(|o| {
                                                o.copied_text = name.clone();
                                            });
                                            ui.close_menu();
                                        }
                                    });
                                }
                            });
                    }
                }
            });
        }

        // ===== Правая панель =====
        if state.show_inspector_panel || state.show_renderer_panel {
            egui::SidePanel::right("right_panel").default_width(340.0).show(ctx, |ui| {
                if state.show_inspector_panel {
                    ui.heading("Inspector");
                    ui.separator();

                    ui.horizontal(|ui| {
                        ui.label("Gizmo:");
                        let m = &mut editor.gizmo.mode;
                        if ui.selectable_label(*m == GizmoMode::Translate, "T (1)").clicked() {
                            *m = GizmoMode::Translate;
                        }
                        if ui.selectable_label(*m == GizmoMode::Rotate, "R (2)").clicked() {
                            *m = GizmoMode::Rotate;
                        }
                        if ui.selectable_label(*m == GizmoMode::Scale, "S (3)").clicked() {
                            *m = GizmoMode::Scale;
                        }
                        ui.toggle_value(&mut editor.gizmo.snap_enabled, "Snap");
                    });
                    ui.label(
                        egui::RichText::new("Alt+drag: duplicate · Ctrl: snap")
                            .small()
                            .weak(),
                    );
                    ui.separator();

                    let n = editor.selected.len();
                    if n == 0 {
                        ui.label("Nothing selected");
                    } else if n > 1 {
                        ui.label(format!("{} objects selected", n));
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Focus (F)").clicked() {
                                action = Some(EditorAction::FocusSelected);
                            }
                            if ui.button("Duplicate").clicked() {
                                action = Some(EditorAction::Duplicate);
                            }
                            if ui.button("Delete").clicked() {
                                action = Some(EditorAction::DeleteSelected);
                            }
                            if ui.button("FBX Sel").clicked() {
                                action = Some(EditorAction::ExportFbxSelected);
                            }
                        });
                        ui.separator();
                        draw_multi_edit(ui, world, editor);
                        ui.separator();
                        if ui.button("Deselect all").clicked() {
                            editor.selected.clear();
                        }
                    } else {
                        let e = editor.selected[0];
                        if !world.entities().contains(&e) {
                            editor.selected.clear();
                            ui.label("Selection removed");
                        } else {
                            draw_inspector(ui, world, e, editor, assets, &mut action);
                        }
                    }
                }

                if state.show_renderer_panel {
                    ui.separator();
                    egui::CollapsingHeader::new("Renderer")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.label("Post-processing");
                            ui.add(
                                egui::Slider::new(&mut postfx.bloom_threshold, 0.1..=5.0)
                                    .text("Bloom threshold"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.bloom_strength, 0.0..=3.0)
                                    .text("Bloom strength"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.bloom_knee, 0.01..=2.0)
                                    .text("Bloom knee"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.bloom_radius, 0.5..=3.0)
                                    .text("Bloom radius"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.exposure, 0.1..=3.0)
                                    .text("Exposure"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.fxaa_strength, 0.0..=1.0)
                                    .text("FXAA strength"),
                            );

                            ui.separator();
                            ui.label("SSAO");
                            ui.add(
                                egui::Slider::new(&mut postfx.ssao_strength, 0.0..=2.0)
                                    .text("Strength"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.ssao_radius, 0.05..=2.0)
                                    .text("Radius"),
                            );

                            ui.separator();
                            ui.label("IBL");
                            ui.add(
                                egui::Slider::new(&mut postfx.ibl_strength, 0.0..=3.0)
                                    .text("IBL strength"),
                            );

                            ui.separator();
                            ui.label("Fog");
                            let mut c = postfx.fog_color;
                            if ui.color_edit_button_rgb(&mut c).changed() {
                                postfx.fog_color = c;
                            }
                            ui.add(
                                egui::Slider::new(&mut postfx.fog_density, 0.0..=0.2)
                                    .text("Density"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.fog_height_base, -10.0..=20.0)
                                    .text("Height base"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.fog_height_falloff, 0.0..=0.5)
                                    .text("Height falloff"),
                            );

                            ui.separator();
                            ui.label("Screen effects");
                            ui.add(
                                egui::Slider::new(&mut postfx.vignette_strength, 0.0..=1.0)
                                    .text("Vignette"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.film_grain, 0.0..=1.0)
                                    .text("Film grain"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.chromatic_aberration, 0.0..=1.0)
                                    .text("Chromatic ab."),
                            );

                            ui.separator();
                            ui.label("Shadows");
                            ui.add(
                                egui::Slider::new(&mut postfx.shadow_bias, 0.0..=0.01)
                                    .text("Depth bias"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.shadow_normal_bias, 0.0..=10.0)
                                    .text("Slope bias"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.shadow_fade_start, 50.0..=250.0)
                                    .text("Fade start"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.shadow_fade_end, 60.0..=300.0)
                                    .text("Fade end"),
                            );

                            ui.separator();
                            ui.label("LOD");
                            ui.add(
                                egui::Slider::new(&mut postfx.lod_bias, 0.2..=4.0)
                                    .logarithmic(true)
                                    .text("LOD bias"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.lod_distances[0], 5.0..=200.0)
                                    .text("Distance LOD0→1"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.lod_distances[1], 20.0..=500.0)
                                    .text("Distance LOD1→2"),
                            );
                            ui.add(
                                egui::Slider::new(&mut postfx.lod_distances[2], 50.0..=1000.0)
                                    .text("Distance LOD2→3"),
                            );

                            ui.separator();
                            ui.label("Debug view");
                            ui.horizontal_wrapped(|ui| {
                                debug_button(
                                    ui, &mut postfx.debug_view,
                                    crate::render::DebugView::Final, "Final",
                                );
                                debug_button(
                                    ui, &mut postfx.debug_view,
                                    crate::render::DebugView::Ssao, "SSAO",
                                );
                                debug_button(
                                    ui, &mut postfx.debug_view,
                                    crate::render::DebugView::GbufferNormal, "Normal",
                                );
                                debug_button(
                                    ui, &mut postfx.debug_view,
                                    crate::render::DebugView::GbufferDepth, "Depth",
                                );
                                debug_button(
                                    ui, &mut postfx.debug_view,
                                    crate::render::DebugView::HdrPreBloom, "HDR",
                                );
                                debug_button(
                                    ui, &mut postfx.debug_view,
                                    crate::render::DebugView::CsmCascade0, "CSM",
                                );
                            });

                            ui.separator();
                            egui::CollapsingHeader::new("Play")
                                .default_open(false)
                                .show(ui, |ui| {
                                    let p = &mut editor.play;
                                    ui.add(egui::Slider::new(&mut p.walk_speed, 0.5..=20.0).text("Walk speed"));
                                    ui.add(egui::Slider::new(&mut p.run_speed, 1.0..=40.0).text("Run speed"));
                                    ui.add(egui::Slider::new(&mut p.jump_speed, 1.0..=20.0).text("Jump"));
                                    ui.add(egui::Slider::new(&mut p.gravity, 1.0..=60.0).text("Gravity"));
                                    ui.add(egui::Slider::new(&mut p.eye_height, 0.5..=3.0).text("Eye height"));
                                    ui.add(
                                        egui::Slider::new(&mut p.look_sensitivity, 0.0005..=0.01)
                                            .text("Look sensitivity"),
                                    );
                                    ui.separator();
                                    ui.label("Player capsule");
                                    ui.add(egui::Slider::new(&mut p.player_radius, 0.1..=1.0).text("Radius"));
                                    ui.add(egui::Slider::new(&mut p.player_height, 0.5..=3.0).text("Height"));
                                    ui.separator();
                                    ui.label("Crouch (Ctrl)");
                                    ui.add(
                                        egui::Slider::new(&mut p.crouch_height, 0.5..=1.7)
                                            .text("Eye height"),
                                    );
                                    ui.add(
                                        egui::Slider::new(&mut p.crouch_speed_mult, 0.1..=1.0)
                                            .text("Speed mult"),
                                    );
                                    ui.separator();
                                    ui.label("Combat");
                                    ui.add(egui::Slider::new(&mut p.max_health, 10.0..=500.0).text("Max HP"));
                                    ui.add(egui::Slider::new(&mut p.max_ammo, 0..=500).text("Max ammo"));
                                    ui.add(egui::Slider::new(&mut p.damage_per_shot, 1.0..=200.0).text("Damage"));
                                    ui.add(egui::Slider::new(&mut p.gun_range, 5.0..=500.0).text("Range"));
                                    ui.add(
                                        egui::Slider::new(&mut p.bullet_speed, 0.0..=300.0)
                                            .text("Bullet speed"),
                                    );
                                    ui.add(egui::Slider::new(&mut p.fire_cooldown_max, 0.02..=1.0).text("Fire cd"));
                                    ui.add(egui::Slider::new(&mut p.interact_distance, 1.0..=20.0).text("Interact dist"));
                                    ui.separator();
                                    ui.checkbox(&mut p.bob_enabled, "Head bob");
                                    ui.add(
                                        egui::Slider::new(&mut p.bob_amplitude, 0.0..=0.15)
                                            .text("Bob amplitude"),
                                    );
                                    ui.separator();
                                    ui.checkbox(&mut p.show_crosshair, "Show crosshair");
                                    ui.checkbox(&mut p.show_hud, "Show HUD");
                                    ui.checkbox(&mut p.show_health, "Show health bar");
                                    ui.checkbox(&mut p.show_ammo, "Show ammo bar");
                                });

                            ui.separator();
                            egui::CollapsingHeader::new("Fly (RMB)")
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.label("WASD — move, E/Q or Space — up/down");
                                    ui.label("Shift — faster, Ctrl — slower");
                                    ui.label("Scroll during fly — fly speed");
                                    ui.separator();
                                    ui.add(
                                        egui::Slider::new(&mut editor.fly_speed, 1.0..=100.0)
                                            .text("Fly speed"),
                                    );
                                    ui.add(
                                        egui::Slider::new(&mut editor.fly_sensitivity, 0.0005..=0.01)
                                            .text("Fly sensitivity"),
                                    );
                                });
                        });
                }
            });
        }
    }

    action
}

// ============================================================
// Component kind
// ============================================================

#[derive(Copy, Clone, PartialEq, Eq)]
enum ComponentKind {
    Transform,
    Mesh,
    Material,
    Skeleton,
    Animation,
    Spinner,
    Velocity,
    Health,
    Chase,
    Interactable,
    Trigger,
    Parent,
    Tint,
    Visible,
    RigidBody,
    Collider,
    PhysicsMaterial,
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
        ComponentKind::RigidBody => { world.remove::<RigidBody>(e); }
        ComponentKind::Collider => { world.remove::<Collider>(e); }
        ComponentKind::PhysicsMaterial => { world.remove::<PhysicsMaterial>(e); }
    }
}

fn add_component_menu(ui: &mut egui::Ui, world: &mut World, e: Entity, editor: &mut EditorState) {
    use crate::game::components::*;

    let mut any = false;

    if !world.has::<Transform>(e) {
        any = true;
        if ui.button("Transform").clicked() {
            world.insert(e, Transform::at(Vec3::ZERO));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<MeshHandle>(e) {
        any = true;
        if ui.button("Mesh").clicked() {
            world.insert(e, MeshHandle("cube".into()));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<MaterialHandle>(e) {
        any = true;
        if ui.button("Material").clicked() {
            world.insert(e, MaterialHandle("flat_blue".into()));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<Tint>(e) {
        any = true;
        if ui.button("Tint").clicked() {
            world.insert(e, Tint([1.0, 1.0, 1.0, 1.0]));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<Visible>(e) {
        any = true;
        if ui.button("Visible (false)").clicked() {
            world.insert(e, Visible(false));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }

    if !world.has::<Spinner>(e)
        || !world.has::<Velocity>(e)
        || !world.has::<Chase>(e)
        || !world.has::<Parent>(e)
    {
        ui.separator();
    }

    if !world.has::<Spinner>(e) {
        any = true;
        if ui.button("Spinner").clicked() {
            world.insert(e, Spinner::new(Vec3::Y, 1.0));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<Velocity>(e) {
        any = true;
        if ui.button("Velocity").clicked() {
            world.insert(e, Velocity::new(0.0, 0.0, 0.0));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<Chase>(e) {
        any = true;
        if ui.button("Chase").clicked() {
            world.insert(e, Chase::new(3.0, 1.2));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<Parent>(e) {
        any = true;
        if ui.button("Parent (self-id)").clicked() {
            world.insert(e, Parent(e));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }

    if !world.has::<Health>(e)
        || !world.has::<Interactable>(e)
        || !world.has::<Trigger>(e)
    {
        ui.separator();
    }

    if !world.has::<Health>(e) {
        any = true;
        if ui.button("Health").clicked() {
            world.insert(e, Health::new(100.0));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }

    if !world.has::<Interactable>(e) {
        any = true;
        ui.menu_button("Interactable", |ui| {
            if ui.button("Pickup").clicked() {
                world.insert(e, Interactable::Pickup);
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Paint (red)").clicked() {
                world.insert(e, Interactable::Paint([1.0, 0.0, 0.0, 1.0]));
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Toggle").clicked() {
                world.insert(e, Interactable::Toggle);
                editor.undo_requested = true;
                ui.close_menu();
            }
        });
    }

    if !world.has::<Trigger>(e) {
        any = true;
        ui.menu_button("Trigger", |ui| {
            if ui.button("Teleport to (0, 2, 0)").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Teleport([0.0, 2.0, 0.0])));
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Tint (green)").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Tint([0.2, 1.0, 0.2, 1.0])));
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Despawn").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Despawn));
                editor.undo_requested = true;
                ui.close_menu();
            }
        });
    }

    if !world.has::<AnimationPlayer>(e) || !world.has::<SkeletonHandle>(e) {
        ui.separator();
    }

    if !world.has::<AnimationPlayer>(e) {
        any = true;
        if ui.button("Animation Player").clicked() {
            world.insert(e, AnimationPlayer::new(""));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }
    if !world.has::<SkeletonHandle>(e) {
        any = true;
        if ui.button("Skeleton Handle").clicked() {
            world.insert(e, SkeletonHandle(String::new()));
            editor.undo_requested = true;
            ui.close_menu();
        }
    }

    ui.separator();
    if !world.has::<RigidBody>(e) {
        any = true;
        ui.menu_button("RigidBody", |ui| {
            if ui.button("Static").clicked() {
                world.insert(e, RigidBody::static_body());
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Dynamic (mass 1)").clicked() {
                world.insert(e, RigidBody::dynamic(1.0));
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Kinematic").clicked() {
                world.insert(e, RigidBody::kinematic());
                editor.undo_requested = true;
                ui.close_menu();
            }
        });
    }
    if !world.has::<Collider>(e) {
        any = true;
        ui.menu_button("Collider", |ui| {
            if ui.button("Sphere (r = 0.5)").clicked() {
                world.insert(e, Collider::sphere(0.5));
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("AABB (1×1×1)").clicked() {
                world.insert(e, Collider::aabb(Vec3::splat(0.5)));
                editor.undo_requested = true;
                ui.close_menu();
            }
            if ui.button("Capsule (r = 0.35, h = 1.8)").clicked() {
                world.insert(e, Collider::capsule(0.35, 1.8));
                editor.undo_requested = true;
                ui.close_menu();
            }
        });
    }
    if !world.has::<PhysicsMaterial>(e) {
        any = true;
        ui.menu_button("PhysicsMaterial", |ui| {
            for (label, mat) in [
                ("Wood", PhysicsMaterial::wood()),
                ("Metal", PhysicsMaterial::metal()),
                ("Rubber", PhysicsMaterial::rubber()),
                ("Ice", PhysicsMaterial::ice()),
                ("Concrete", PhysicsMaterial::concrete()),
            ] {
                if ui.button(label).clicked() {
                    world.insert(e, mat);
                    editor.undo_requested = true;
                    ui.close_menu();
                }
            }
        });
    }

    if !any {
        ui.label(
            egui::RichText::new("All components already present")
                .weak()
                .italics(),
        );
    }
}

// ============================================================
// Вспомогательное
// ============================================================

fn would_create_cycle(world: &World, child: Entity, new_parent: Entity) -> bool {
    let mut cur = new_parent;
    for _ in 0..64 {
        if cur == child {
            return true;
        }
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
    textures: &[(String, u32, u32)],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(label);
        let display = current.as_deref().unwrap_or("(none)");
        egui::ComboBox::from_id_source(id)
            .selected_text(display)
            .width(170.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "(none)").clicked() {
                    *current = None;
                    changed = true;
                }
                if textures.is_empty() {
                    ui.label(egui::RichText::new("(no textures loaded)").weak().italics());
                }
                for (name, w, h) in textures {
                    let selected = current.as_deref() == Some(name.as_str());
                    if ui
                        .selectable_label(selected, name)
                        .on_hover_text(format!("{}×{} pixels", w, h))
                        .clicked()
                    {
                        *current = Some(name.clone());
                        changed = true;
                    }
                }
            });
    });
    changed
}

fn brush_button(ui: &mut egui::Ui, palette: &mut PaletteState, item: PaletteItem) {
    let selected = palette.active == Some(item);
    if ui
        .selectable_label(selected, item.label())
        .on_hover_text(format!("Click to place {}", item.label()))
        .clicked()
    {
        palette.active = if selected { None } else { Some(item) };
    }
}

fn draw_multi_edit(ui: &mut egui::Ui, world: &mut World, editor: &mut EditorState) {
    let selected: Vec<Entity> = editor.selected.clone();

    let with_tf: Vec<(Entity, Transform)> = selected
        .iter()
        .filter_map(|&e| world.get::<Transform>(e).map(|t| (e, *t)))
        .collect();

    if with_tf.is_empty() {
        ui.label(
            egui::RichText::new("No Transform components in selection")
                .weak()
                .italics(),
        );
        return;
    }

    ui.label(
        egui::RichText::new(format!("Group Transform ({} objects)", with_tf.len())).strong(),
    );

    fn common_vec3<I: Iterator<Item = Vec3>>(mut it: I) -> Option<Vec3> {
        let first = it.next()?;
        for v in it {
            if (v - first).length() > 1e-4 {
                return None;
            }
        }
        Some(first)
    }

    // Position
    let pos_common = common_vec3(with_tf.iter().map(|(_, t)| t.position));
    let mixed_pos = pos_common.is_none();
    let mut pos = pos_common.unwrap_or(Vec3::ZERO).to_array();

    ui.label(if mixed_pos {
        egui::RichText::new("Position (mixed)").weak()
    } else {
        egui::RichText::new("Position")
    });

    let mut pos_changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(
                egui::DragValue::new(&mut pos[i])
                    .speed(0.01)
                    .prefix(["X ", "Y ", "Z "][i]),
            );
            if r.changed() {
                pos_changed = true;
            }
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
        }
    });

    if pos_changed {
        let new_pos = Vec3::from_array(pos);
        for (e, _) in &with_tf {
            if let Some(t) = world.get_mut::<Transform>(*e) {
                t.position = new_pos;
            }
        }
    }

    // Rotation
    let eulers: Vec<Vec3> = with_tf
        .iter()
        .map(|(_, t)| {
            let (y, x, z) = t.rotation.to_euler(glam::EulerRot::YXZ);
            Vec3::new(x.to_degrees(), y.to_degrees(), z.to_degrees())
        })
        .collect();
    let rot_common = common_vec3(eulers.iter().copied());
    let mixed_rot = rot_common.is_none();
    let mut rot = rot_common.unwrap_or(Vec3::ZERO).to_array();

    ui.label(if mixed_rot {
        egui::RichText::new("Rotation (deg, mixed)").weak()
    } else {
        egui::RichText::new("Rotation (deg)")
    });

    let mut rot_changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(
                egui::DragValue::new(&mut rot[i])
                    .speed(0.5)
                    .prefix(["X ", "Y ", "Z "][i]),
            );
            if r.changed() {
                rot_changed = true;
            }
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
        }
    });

    if rot_changed {
        let q = glam::Quat::from_euler(
            glam::EulerRot::YXZ,
            rot[1].to_radians(),
            rot[0].to_radians(),
            rot[2].to_radians(),
        );
        for (e, _) in &with_tf {
            if let Some(t) = world.get_mut::<Transform>(*e) {
                t.rotation = q;
            }
        }
    }

    // Scale
    let scale_common = common_vec3(with_tf.iter().map(|(_, t)| t.scale));
    let mixed_scale = scale_common.is_none();
    let mut scale = scale_common.unwrap_or(Vec3::ONE).to_array();

    ui.label(if mixed_scale {
        egui::RichText::new("Scale (mixed)").weak()
    } else {
        egui::RichText::new("Scale")
    });

    let mut scale_changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(
                egui::DragValue::new(&mut scale[i])
                    .speed(0.01)
                    .prefix(["X ", "Y ", "Z "][i]),
            );
            if r.changed() {
                scale_changed = true;
            }
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
        }
    });

    if scale_changed {
        let new_scale = Vec3::from_array(scale);
        for (e, _) in &with_tf {
            if let Some(t) = world.get_mut::<Transform>(*e) {
                t.scale = new_scale;
            }
        }
    }
}

// ============================================================
// Play HUD
// ============================================================

fn draw_play_hud(ctx: &egui::Context, play: &PlayState, fps: f32) {
    if play.show_crosshair {
        let screen = ctx.screen_rect();
        let center = screen.center();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("crosshair_layer"),
        ));

        let (color, len, thick) = if play.highlight.is_some() {
            (
                egui::Color32::from_rgba_unmultiplied(255, 220, 90, 240),
                10.0_f32,
                1.5_f32,
            )
        } else {
            (
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 200),
                8.0_f32,
                1.0_f32,
            )
        };

        painter.line_segment(
            [
                egui::pos2(center.x - len, center.y),
                egui::pos2(center.x + len, center.y),
            ],
            egui::Stroke::new(thick, color),
        );
        painter.line_segment(
            [
                egui::pos2(center.x, center.y - len),
                egui::pos2(center.x, center.y + len),
            ],
            egui::Stroke::new(thick, color),
        );
    }

    if play.highlight.is_some() {
        let screen = ctx.screen_rect();
        let center = screen.center();
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("hint_layer"),
        ));
        painter.text(
            egui::pos2(center.x + 14.0, center.y + 14.0),
            egui::Align2::LEFT_TOP,
            "[E] interact",
            egui::FontId::proportional(13.0),
            egui::Color32::from_rgba_unmultiplied(255, 220, 90, 230),
        );
    }

    if play.show_hud {
        egui::Area::new(egui::Id::new("play_hud_area"))
            .anchor(egui::Align2::LEFT_BOTTOM, egui::vec2(16.0, -16.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140))
                    .inner_margin(egui::Margin::symmetric(10.0, 6.0))
                    .show(ui, |ui| {
                        ui.label(
                            egui::RichText::new(format!("FPS: {:.0}", fps))
                                .monospace()
                                .color(egui::Color32::from_rgb(230, 230, 230)),
                        );
                        let p = play.saved_position;
                        ui.label(
                            egui::RichText::new(format!(
                                "Pos: {:.1}, {:.1}, {:.1}",
                                p.x, p.y, p.z
                            ))
                            .monospace()
                            .color(egui::Color32::from_rgb(200, 200, 200)),
                        );
                        let state_str = if play.crouching {
                            "crouching"
                        } else if play.on_ground {
                            "on ground"
                        } else {
                            "airborne"
                        };
                        ui.label(
                            egui::RichText::new(state_str)
                                .monospace()
                                .color(egui::Color32::from_rgb(180, 200, 180)),
                        );
                    });
            });
    }

    let show_bars = play.show_health || play.show_ammo;
    if show_bars {
        egui::Area::new(egui::Id::new("play_bars_area"))
            .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-16.0, -16.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140))
                    .inner_margin(egui::Margin::symmetric(10.0, 8.0))
                    .show(ui, |ui| {
                        ui.set_min_width(180.0);

                        if play.show_health {
                            let frac = if play.max_health > 0.0 {
                                (play.health / play.max_health).clamp(0.0, 1.0)
                            } else {
                                0.0
                            };
                            ui.label(
                                egui::RichText::new(format!(
                                    "HP   {:>3.0} / {:<3.0}",
                                    play.health, play.max_health
                                ))
                                .monospace()
                                .color(egui::Color32::from_rgb(230, 230, 230)),
                            );
                            let desired = egui::vec2(160.0, 10.0);
                            let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                            let painter = ui.painter();
                            painter.rect_filled(
                                rect,
                                egui::Rounding::same(2.0),
                                egui::Color32::from_rgb(40, 40, 40),
                            );
                            let fill_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(rect.width() * frac, rect.height()),
                            );
                            let col = if frac > 0.5 {
                                egui::Color32::from_rgb(80, 200, 80)
                            } else if frac > 0.25 {
                                egui::Color32::from_rgb(220, 180, 60)
                            } else {
                                egui::Color32::from_rgb(220, 70, 70)
                            };
                            painter.rect_filled(fill_rect, egui::Rounding::same(2.0), col);
                        }

                        if play.show_ammo {
                            if play.show_health {
                                ui.add_space(4.0);
                            }
                            ui.label(
                                egui::RichText::new(format!(
                                    "AMMO {:>3} / {:<3}",
                                    play.ammo, play.max_ammo
                                ))
                                .monospace()
                                .color(egui::Color32::from_rgb(230, 230, 230)),
                            );
                            let frac = if play.max_ammo > 0 {
                                play.ammo as f32 / play.max_ammo as f32
                            } else {
                                0.0
                            };
                            let desired = egui::vec2(160.0, 10.0);
                            let (rect, _) = ui.allocate_exact_size(desired, egui::Sense::hover());
                            let painter = ui.painter();
                            painter.rect_filled(
                                rect,
                                egui::Rounding::same(2.0),
                                egui::Color32::from_rgb(40, 40, 40),
                            );
                            let fill_rect = egui::Rect::from_min_size(
                                rect.min,
                                egui::vec2(rect.width() * frac, rect.height()),
                            );
                            painter.rect_filled(
                                fill_rect,
                                egui::Rounding::same(2.0),
                                egui::Color32::from_rgb(220, 190, 90),
                            );
                        }
                    });
            });
    }
}

fn entity_display_name(world: &World, e: Entity) -> String {
    if let Some(n) = world.get::<Name>(e) {
        return n.0.clone();
    }
    let mesh = world.get::<MeshHandle>(e).map(|m| m.0.as_str()).unwrap_or("?");
    format!("#{} {}", e, mesh)
}

fn draw_inspector(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
    assets: &UiAssets<'_>,
    action: &mut Option<EditorAction>,
) {
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(format!("#{}", e)).weak().monospace());
        if let Some(n) = world.get_mut::<Name>(e) {
            let r = ui.add(egui::TextEdit::singleline(&mut n.0).desired_width(160.0));
            if r.gained_focus() {
                editor.undo_requested = true;
            }
        } else if ui.button("+ Name").clicked() {
            editor.undo_requested = true;
            world.insert(e, Name(format!("Entity_{}", e)));
        }
        if ui.button("Focus (F)").clicked() {
            *action = Some(EditorAction::FocusSelected);
        }
        if ui.button("Spawn Player Here").clicked() {
            *action = Some(EditorAction::SpawnPlayerHere);
        }
        if ui.button("Export FBX").clicked() {
            *action = Some(EditorAction::ExportFbxSelected);
        }
    });

    ui.horizontal(|ui| {
        let mut visible = world.get::<Visible>(e).map(|v| v.0).unwrap_or(true);
        if ui.checkbox(&mut visible, "Visible").changed() {
            editor.undo_requested = true;
            if visible {
                world.remove::<Visible>(e);
            } else {
                world.insert(e, Visible(false));
            }
        }
    });

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
    if world.has::<RigidBody>(e) { chips.push(("RigidBody", ComponentKind::RigidBody)); }
    if world.has::<Collider>(e) { chips.push(("Collider", ComponentKind::Collider)); }
    if world.has::<PhysicsMaterial>(e) { chips.push(("PhysicsMaterial", ComponentKind::PhysicsMaterial)); }

    ui.horizontal_wrapped(|ui| {
        for (label, kind) in &chips {
            let resp = ui
                .add(
                    egui::Label::new(
                        egui::RichText::new(*label)
                            .small()
                            .background_color(ui.visuals().faint_bg_color)
                            .color(ui.visuals().weak_text_color()),
                    )
                    .sense(egui::Sense::click()),
                )
                .on_hover_text("Right-click to remove");

            resp.context_menu(|ui| {
                if ui.button(format!("Remove {}", label)).clicked() {
                    remove_component(world, e, *kind);
                    editor.undo_requested = true;
                    ui.close_menu();
                }
            });
        }
    });

    ui.horizontal(|ui| {
        ui.menu_button("+ Add Component", |ui| {
            add_component_menu(ui, world, e, editor);
        });
        if !chips.is_empty() {
            ui.label(
                egui::RichText::new("right-click chip to remove")
                    .small()
                    .weak()
                    .italics(),
            );
        }
    });

    ui.separator();

    if world.has::<Transform>(e) {
        egui::CollapsingHeader::new("Transform")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(t) = world.get_mut::<Transform>(e) {
                    ui.horizontal(|ui| {
                        if ui.button("Copy").clicked() {
                            editor.clipboard_transform = Some(*t);
                        }
                        let paste_enabled = editor.clipboard_transform.is_some();
                        if ui.add_enabled(paste_enabled, egui::Button::new("Paste")).clicked() {
                            if let Some(src) = editor.clipboard_transform {
                                editor.undo_requested = true;
                                *t = src;
                            }
                        }
                        if ui.button("Reset").clicked() {
                            editor.undo_requested = true;
                            t.position = Vec3::ZERO;
                            t.rotation = glam::Quat::IDENTITY;
                            t.scale = Vec3::ONE;
                        }
                    });

                    let mut pos = t.position.to_array();
                    ui.label("Position");
                    let mut changed_pos = false;
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            let r = ui.add(
                                egui::DragValue::new(&mut pos[i])
                                    .speed(0.01)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                            if r.drag_started() || r.gained_focus() {
                                editor.undo_requested = true;
                            }
                            if r.changed() { changed_pos = true; }
                        }
                    });
                    if changed_pos {
                        t.position = Vec3::from_array(pos);
                    }

                    let (mut ry, mut rx, mut rz) = t.rotation.to_euler(glam::EulerRot::YXZ);
                    ui.label("Rotation (deg)");
                    let mut changed_rot = false;
                    ui.horizontal(|ui| {
                        let mut rxd = rx.to_degrees();
                        let mut ryd = ry.to_degrees();
                        let mut rzd = rz.to_degrees();
                        let vals: [&mut f32; 3] = [&mut rxd, &mut ryd, &mut rzd];
                        for i in 0..3 {
                            let r = ui.add(
                                egui::DragValue::new(vals[i])
                                    .speed(0.5)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                            if r.drag_started() || r.gained_focus() {
                                editor.undo_requested = true;
                            }
                            if r.changed() { changed_rot = true; }
                        }
                        if changed_rot {
                            rx = rxd.to_radians();
                            ry = ryd.to_radians();
                            rz = rzd.to_radians();
                        }
                    });
                    if changed_rot {
                        t.rotation = glam::Quat::from_euler(glam::EulerRot::YXZ, ry, rx, rz);
                    }

                    let mut scale = t.scale.to_array();
                    ui.label("Scale");
                    let mut changed_scale = false;
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            let r = ui.add(
                                egui::DragValue::new(&mut scale[i])
                                    .speed(0.01)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                            if r.drag_started() || r.gained_focus() {
                                editor.undo_requested = true;
                            }
                            if r.changed() { changed_scale = true; }
                        }
                    });
                    if changed_scale {
                        t.scale = Vec3::from_array(scale);
                    }
                }
            });
    }

    if world.has::<Parent>(e) || editor.selected.len() >= 2 {
        egui::CollapsingHeader::new("Hierarchy")
            .default_open(true)
            .show(ui, |ui| {
                let parent_opt = world.get::<Parent>(e).copied();
                match parent_opt {
                    Some(Parent(p)) => {
                        let pname = world
                            .get::<Name>(p)
                            .map(|n| n.0.clone())
                            .unwrap_or_else(|| format!("#{}", p));
                        ui.label(format!("Parent: {}", pname));
                        if ui.button("Clear Parent").clicked() {
                            editor.undo_requested = true;
                            world.remove::<Parent>(e);
                        }
                    }
                    None => {
                        ui.label("Parent: (none)");
                    }
                }

                if editor.selected.len() >= 2 {
                    if ui.button("Set parent from last selected").clicked() {
                        let primary = *editor.selected.last().unwrap();
                        if primary != e && !would_create_cycle(world, e, primary) {
                            editor.undo_requested = true;
                            world.insert(e, Parent(primary));
                        }
                    }
                }

                ui.separator();
                let children: Vec<Entity> = world
                    .entities()
                    .iter()
                    .copied()
                    .filter(|&c| world.get::<Parent>(c).map(|p| p.0 == e).unwrap_or(false))
                    .collect();
                if children.is_empty() {
                    ui.label("Children: (none)");
                } else {
                    ui.label(format!("Children: {}", children.len()));
                    for c in children {
                        let cname = world
                            .get::<Name>(c)
                            .map(|n| n.0.clone())
                            .unwrap_or_else(|| format!("#{}", c));
                        if ui.selectable_label(false, cname).clicked() {
                            editor.select_single(c);
                        }
                    }
                }
            });
    }

    if world.has::<MeshHandle>(e) || world.has::<MaterialHandle>(e) {
        egui::CollapsingHeader::new("Geometry")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(mh) = world.get_mut::<MeshHandle>(e) {
                    let mut current = mh.0.clone();
                    ui.horizontal(|ui| {
                        ui.label("Mesh:");
                        egui::ComboBox::from_id_source("mesh_selector")
                            .selected_text(&current)
                            .show_ui(ui, |ui| {
                                for name in assets.mesh_names {
                                    ui.selectable_value(&mut current, name.clone(), name);
                                }
                            });
                    });
                    if current != mh.0 { mh.0 = current; }
                }

                if let Some(mh) = world.get_mut::<MaterialHandle>(e) {
                    let mut current = mh.0.clone();
                    ui.horizontal(|ui| {
                        ui.label("Material:");
                        egui::ComboBox::from_id_source("mat_selector")
                            .selected_text(&current)
                            .show_ui(ui, |ui| {
                                for name in assets.material_names {
                                    ui.selectable_value(&mut current, name.clone(), name);
                                }
                            });
                    });
                    if current != mh.0 { mh.0 = current; }
                }
            });
    }

    if let Some((name, original)) = &assets.selected_material {
        let header_label = format!("Material: {}", name);
        egui::CollapsingHeader::new(header_label)
            .default_open(false)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    if ui.button("Make Unique").clicked() {
                        *action = Some(EditorAction::MakeMaterialUnique);
                    }
                });
                ui.separator();

                let mut m = original.clone();
                let mut changed = false;

                ui.label("Base color");
                let r = ui.color_edit_button_rgba_unmultiplied(&mut m.base_color);
                if r.changed() { changed = true; editor.undo_requested = true; }

                let r = ui.add(egui::Slider::new(&mut m.metallic, 0.0..=1.0).text("Metallic"));
                if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
                if r.changed() { changed = true; }

                let r = ui.add(egui::Slider::new(&mut m.roughness, 0.0..=1.0).text("Roughness"));
                if r.drag_started() || r.gained_focus() { editor.undo_requested = true; }
                if r.changed() { changed = true; }

                ui.label("Emissive");
                let r = ui.color_edit_button_rgb(&mut m.emissive);
                if r.changed() { changed = true; editor.undo_requested = true; }

                ui.separator();
                ui.label(egui::RichText::new("Textures").strong());
                ui.label(
                    egui::RichText::new("загрузи через Assets → Load Texture(s)…")
                        .small()
                        .weak(),
                );

                if texture_picker(ui, "mat_base_tex", "Base color:",
                    &mut m.base_color_texture, assets.texture_list) {
                    changed = true; editor.undo_requested = true;
                }
                if texture_picker(ui, "mat_mr_tex", "Metallic-Rough:",
                    &mut m.metallic_roughness_texture, assets.texture_list) {
                    changed = true; editor.undo_requested = true;
                }
                if texture_picker(ui, "mat_normal_tex", "Normal map:",
                    &mut m.normal_texture, assets.texture_list) {
                    changed = true; editor.undo_requested = true;
                }
                if texture_picker(ui, "mat_emissive_tex", "Emissive map:",
                    &mut m.emissive_texture, assets.texture_list) {
                    changed = true; editor.undo_requested = true;
                }

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

                if ui.checkbox(&mut m.double_sided, "Double-sided").changed() {
                    changed = true;
                }

                if changed {
                    editor.dirty_materials.push((e, name.clone(), m));
                }
            });
    }

    if world.has::<Tint>(e) {
        egui::CollapsingHeader::new("Tint")
            .default_open(true)
            .show(ui, |ui| {
                if let Some(t) = world.get_mut::<Tint>(e) {
                    let mut color = t.0;
                    ui.horizontal(|ui| {
                        ui.label("Color:");
                        if ui.color_edit_button_rgba_unmultiplied(&mut color).changed() {
                            t.0 = color;
                            editor.undo_requested = true;
                        }
                        if ui.small_button("White").clicked() {
                            t.0 = [1.0, 1.0, 1.0, 1.0];
                            editor.undo_requested = true;
                        }
                    });
                    ui.label(
                        egui::RichText::new("Поверх base_color материала (multiply)")
                            .small()
                            .weak(),
                    );
                }
            });
    }

    if world.has::<Health>(e) {
        egui::CollapsingHeader::new("Health").default_open(true).show(ui, |ui| {
            if let Some(h) = world.get_mut::<Health>(e) {
                ui.add(egui::Slider::new(&mut h.max, 1.0..=1000.0).text("Max"));
                ui.add(egui::Slider::new(&mut h.current, 0.0..=h.max).text("Current"));
            }
        });
    }

    if world.has::<Chase>(e) {
        egui::CollapsingHeader::new("Chase").default_open(true).show(ui, |ui| {
            if let Some(c) = world.get_mut::<Chase>(e) {
                ui.add(egui::Slider::new(&mut c.speed, 0.1..=20.0).text("Speed"));
                ui.add(egui::Slider::new(&mut c.stop_distance, 0.1..=10.0).text("Stop dist"));
            }
        });
    }

    if world.has::<Spinner>(e) {
        egui::CollapsingHeader::new("Spinner").default_open(true).show(ui, |ui| {
            if let Some(sp) = world.get_mut::<Spinner>(e) {
                let mut axis = sp.axis.to_array();
                ui.label("Axis");
                ui.horizontal(|ui| {
                    for i in 0..3 {
                        ui.add(egui::DragValue::new(&mut axis[i]).speed(0.01)
                            .prefix(["X ", "Y ", "Z "][i]));
                    }
                });
                sp.axis = Vec3::from_array(axis);
                ui.add(egui::DragValue::new(&mut sp.speed).speed(0.01).prefix("Speed "));
            }
        });
    }

    if world.has::<Velocity>(e) {
        egui::CollapsingHeader::new("Velocity").default_open(true).show(ui, |ui| {
            if let Some(v) = world.get_mut::<Velocity>(e) {
                let mut val = v.value.to_array();
                ui.horizontal(|ui| {
                    for i in 0..3 {
                        ui.add(egui::DragValue::new(&mut val[i]).speed(0.01)
                            .prefix(["X ", "Y ", "Z "][i]));
                    }
                });
                v.value = Vec3::from_array(val);
            }
        });
    }

    if world.has::<RigidBody>(e) {
        egui::CollapsingHeader::new("RigidBody").default_open(true).show(ui, |ui| {
            if let Some(rb) = world.get_mut::<RigidBody>(e) {
                let mut changed = false;
                ui.horizontal(|ui| {
                    ui.label("Type:");
                    let mut bt = rb.body_type;
                    ui.selectable_value(&mut bt, BodyType::Static, "Static");
                    ui.selectable_value(&mut bt, BodyType::Dynamic, "Dynamic");
                    ui.selectable_value(&mut bt, BodyType::Kinematic, "Kinematic");
                    if bt != rb.body_type {
                        rb.body_type = bt;
                        changed = true;
                    }
                });
                if rb.body_type == BodyType::Dynamic {
                    ui.add(egui::Slider::new(&mut rb.mass, 0.01..=100.0)
                        .logarithmic(true).text("Mass"));
                    ui.add(egui::Slider::new(&mut rb.gravity_scale, 0.0..=3.0)
                        .text("Gravity scale"));
                    ui.add(egui::Slider::new(&mut rb.linear_damping, 0.0..=1.0)
                        .text("Linear damping"));
                    let mut v = rb.velocity.to_array();
                    ui.label("Velocity");
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            ui.add(egui::DragValue::new(&mut v[i])
                                .speed(0.1).prefix(["X ", "Y ", "Z "][i]));
                        }
                    });
                    rb.velocity = Vec3::from_array(v);
                }
                ui.horizontal(|ui| {
                    ui.label(format!("Sleeping: {} ({:.2}s)", rb.sleeping, rb.sleep_timer));
                    if ui.small_button("Wake").clicked() { rb.wake(); }
                });
                if changed { editor.undo_requested = true; }
            }
        });
    }

    if world.has::<Collider>(e) {
        egui::CollapsingHeader::new("Collider").default_open(true).show(ui, |ui| {
            if let Some(col) = world.get_mut::<Collider>(e) {
                let kind = match col {
                    Collider::Sphere { .. } => 0,
                    Collider::Aabb { .. } => 1,
                    Collider::Capsule { .. } => 2,
                };
                ui.horizontal(|ui| {
                    ui.label("Shape:");
                    let mut k = kind;
                    ui.selectable_value(&mut k, 0, "Sphere");
                    ui.selectable_value(&mut k, 1, "AABB");
                    ui.selectable_value(&mut k, 2, "Capsule");
                });
                match col {
                    Collider::Sphere { radius } => {
                        ui.add(egui::Slider::new(radius, 0.05..=20.0).text("Radius"));
                    }
                    Collider::Aabb { half_extents } => {
                        let mut h = half_extents.to_array();
                        ui.label("Half extents");
                        ui.horizontal(|ui| {
                            for i in 0..3 {
                                ui.add(egui::DragValue::new(&mut h[i]).speed(0.01)
                                    .prefix(["X ", "Y ", "Z "][i]));
                            }
                        });
                        *half_extents = Vec3::from_array(h);
                    }
                    Collider::Capsule { radius, height } => {
                        ui.add(egui::Slider::new(radius, 0.05..=5.0).text("Radius"));
                        ui.add(egui::Slider::new(height, 0.1..=10.0).text("Height"));
                    }
                }
            }
        });
    }

    if world.has::<PhysicsMaterial>(e) {
        egui::CollapsingHeader::new("PhysicsMaterial").default_open(false).show(ui, |ui| {
            if let Some(m) = world.get_mut::<PhysicsMaterial>(e) {
                ui.add(egui::Slider::new(&mut m.restitution, 0.0..=1.0)
                    .text("Restitution (bounce)"));
                ui.add(egui::Slider::new(&mut m.friction, 0.0..=2.0).text("Friction"));
            }
        });
    }

    ui.separator();
    if ui.button("Deselect").clicked() {
        editor.selected.clear();
    }
}

fn debug_button(
    ui: &mut egui::Ui,
    current: &mut crate::render::DebugView,
    target: crate::render::DebugView,
    label: &str,
) {
    if ui.selectable_label(*current == target, label).clicked() {
        *current = target;
    }
}