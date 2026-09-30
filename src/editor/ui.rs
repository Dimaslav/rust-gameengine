//! Панели редактора.

use crate::ecs::{Entity, World};
use crate::editor::gizmo::GizmoMode;
use crate::editor::{EditorAction, EditorState};
use crate::game::components::{
    AnimationPlayer, MaterialHandle, MeshHandle, Name, SkeletonHandle, Spinner, Transform,
    Velocity,
};
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
    pub entities: usize,
    pub draws: usize,
    pub instances: usize,
    pub dir_lights: usize,
    pub point_lights: usize,
}

pub struct UiAssets<'a> {
    pub mesh_names: &'a [String],
    pub material_names: &'a [String],
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
            if ui.button("Save").clicked() {
                action = Some(EditorAction::Save);
            }
            if ui.button("Load").clicked() {
                action = Some(EditorAction::Load);
            }
            ui.add(
                egui::TextEdit::singleline(&mut editor.save_path)
                    .desired_width(180.0)
                    .hint_text("scene.ron"),
            );

            ui.separator();
            ui.label(format!("FPS: {:.1}", stats.fps));
            ui.separator();
            ui.label(format!("Entities: {}", stats.entities));
            ui.separator();
            ui.label(format!("Lights: {}d {}p", stats.dir_lights, stats.point_lights));

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut state.show_renderer_panel, "Renderer");
                ui.toggle_value(&mut state.show_inspector_panel, "Inspector");
                ui.toggle_value(&mut state.show_hierarchy_panel, "Hierarchy");
                ui.toggle_value(&mut state.show_stats_panel, "Stats");
            });
        });
    });

    // ===== Левая панель =====
    if state.show_stats_panel || state.show_hierarchy_panel {
        egui::SidePanel::left("left_panel")
            .default_width(260.0)
            .show(ctx, |ui| {
                if state.show_stats_panel {
                    ui.heading("Stats");
                    ui.separator();
                    ui.label(format!("FPS: {:.1}", stats.fps));
                    ui.label(format!(
                        "Frame: {:.2} ms",
                        if stats.fps > 0.1 { 1000.0 / stats.fps } else { 0.0 }
                    ));
                    ui.separator();
                    ui.label(format!("Entities: {}", stats.entities));
                    ui.label(format!("Draws: {}", stats.draws));
                    ui.label(format!("Instances: {}", stats.instances));
                    ui.separator();
                    ui.label(format!("Dir lights: {}", stats.dir_lights));
                    ui.label(format!("Point lights: {}", stats.point_lights));
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
                            .add_enabled(
                                !editor.selected.is_empty(),
                                egui::Button::new("Delete"),
                            )
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

                    let row_height = 18.0;
                    egui::ScrollArea::vertical()
                        .auto_shrink([false; 2])
                        .max_height(400.0)
                        .show_rows(ui, row_height, total, |ui, row_range| {
                            for i in row_range {
                                let e = filtered[i];
                                let label = entity_display_name(world, e);
                                let is_selected = editor.is_selected(e);

                                let response = ui.selectable_label(is_selected, &label);

                                if response.clicked() {
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
                                if response.double_clicked() {
                                    editor.select_single(e);
                                    action = Some(EditorAction::FocusSelected);
                                }

                                response.context_menu(|ui| {
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
                                });
                            }
                        });
                }
            });
    }

    // ===== Правая панель =====
    if state.show_inspector_panel || state.show_renderer_panel {
        egui::SidePanel::right("right_panel")
            .default_width(340.0)
            .show(ctx, |ui| {
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
                    });
                    ui.separator();

                    let n = editor.selected.len();
                    if n == 0 {
                        ui.label("Nothing selected");
                    } else if n > 1 {
                        ui.label(format!("{} objects selected", n));
                        ui.label("(Ctrl+click to add/remove, Shift+click for range)");
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
                        });
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
                                egui::Slider::new(&mut postfx.exposure, 0.1..=3.0)
                                    .text("Exposure"),
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
                            ui.label("Debug view");
                            ui.horizontal_wrapped(|ui| {
                                debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::Final, "Final");
                                debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::Ssao, "SSAO");
                                debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::GbufferNormal, "Normal");
                                debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::GbufferDepth, "Depth");
                                debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::HdrPreBloom, "HDR");
                                debug_button(ui, &mut postfx.debug_view, crate::render::DebugView::CsmCascade0, "CSM");
                            });
                        });
                }
            });
    }

    action
}

// ============================================================
// Helpers
// ============================================================

fn entity_display_name(world: &World, e: Entity) -> String {
    if let Some(n) = world.get::<Name>(e) {
        return n.0.clone();
    }
    let mesh = world
        .get::<MeshHandle>(e)
        .map(|m| m.0.as_str())
        .unwrap_or("?");
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
    // === Header: Entity ID + Name + Focus ===
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!("#{}", e))
                .weak()
                .monospace(),
        );
        if let Some(n) = world.get_mut::<Name>(e) {
            let r = ui.add(
                egui::TextEdit::singleline(&mut n.0)
                    .desired_width(160.0),
            );
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
    });

    // === Components chips ===
    let mut chips: Vec<&str> = Vec::new();
    if world.has::<Transform>(e) { chips.push("Transform"); }
    if world.has::<MeshHandle>(e) { chips.push("Mesh"); }
    if world.has::<MaterialHandle>(e) { chips.push("Material"); }
    if world.has::<SkeletonHandle>(e) { chips.push("Skeleton"); }
    if world.has::<AnimationPlayer>(e) { chips.push("Animation"); }
    if world.has::<Spinner>(e) { chips.push("Spinner"); }
    if world.has::<Velocity>(e) { chips.push("Velocity"); }
    if !chips.is_empty() {
        ui.horizontal_wrapped(|ui| {
            for c in chips {
                ui.label(
                    egui::RichText::new(c)
                        .small()
                        .background_color(ui.visuals().faint_bg_color)
                        .color(ui.visuals().weak_text_color()),
                );
            }
        });
    }

    ui.separator();

    // === Transform ===
    egui::CollapsingHeader::new("Transform")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(t) = world.get_mut::<Transform>(e) {
                // Кнопки Copy/Paste/Reset
                ui.horizontal(|ui| {
                    if ui.button("Copy").on_hover_text("Скопировать transform").clicked() {
                        editor.clipboard_transform = Some(*t);
                    }
                    let paste_enabled = editor.clipboard_transform.is_some();
                    if ui
                        .add_enabled(paste_enabled, egui::Button::new("Paste"))
                        .clicked()
                    {
                        if let Some(src) = editor.clipboard_transform {
                            editor.undo_requested = true;
                            *t = src;
                        }
                    }
                    if ui
                        .button("Reset")
                        .on_hover_text("position=0, rotation=I, scale=1")
                        .clicked()
                    {
                        editor.undo_requested = true;
                        t.position = Vec3::ZERO;
                        t.rotation = glam::Quat::IDENTITY;
                        t.scale = Vec3::ONE;
                    }
                });

                // Position
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
                        if r.changed() {
                            changed_pos = true;
                        }
                    }
                });
                if changed_pos {
                    t.position = Vec3::from_array(pos);
                }

                // Rotation
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
                        if r.changed() {
                            changed_rot = true;
                        }
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

                // Scale
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
                        if r.changed() {
                            changed_scale = true;
                        }
                    }
                });
                if changed_scale {
                    t.scale = Vec3::from_array(scale);
                }
            } else {
                ui.label("(no Transform)");
            }
        });

    // === Geometry (Mesh + Material) ===
    egui::CollapsingHeader::new("Geometry")
        .default_open(true)
        .show(ui, |ui| {
            // Mesh selector
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
                if current != mh.0 {
                    mh.0 = current;
                }
            } else {
                ui.label("(no Mesh)");
            }

            // Material selector
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
                if current != mh.0 {
                    mh.0 = current;
                }
            } else {
                ui.label("(no Material)");
            }
        });

    // === Material editor ===
    if let Some((name, original)) = &assets.selected_material {
        egui::CollapsingHeader::new(format!("Material: {}", name))
            .default_open(false)
            .show(ui, |ui| {
                let mut m = original.clone();
                let mut changed = false;

                ui.label("Base color");
                let r = ui.color_edit_button_rgba_unmultiplied(&mut m.base_color);
                if r.changed() {
                    changed = true;
                    editor.undo_requested = true;
                }

                let r = ui.add(egui::Slider::new(&mut m.metallic, 0.0..=1.0).text("Metallic"));
                if r.drag_started() || r.gained_focus() {
                    editor.undo_requested = true;
                }
                if r.changed() {
                    changed = true;
                }

                let r = ui.add(egui::Slider::new(&mut m.roughness, 0.0..=1.0).text("Roughness"));
                if r.drag_started() || r.gained_focus() {
                    editor.undo_requested = true;
                }
                if r.changed() {
                    changed = true;
                }

                ui.label("Emissive");
                let r = ui.color_edit_button_rgb(&mut m.emissive);
                if r.changed() {
                    changed = true;
                    editor.undo_requested = true;
                }

                ui.label("Alpha mode");
                ui.horizontal(|ui| {
                    let mut am = m.alpha_mode;
                    if ui.selectable_label(am == AlphaMode::Opaque, "Opaque").clicked() {
                        am = AlphaMode::Opaque;
                    }
                    if ui.selectable_label(am == AlphaMode::Mask, "Mask").clicked() {
                        am = AlphaMode::Mask;
                    }
                    if ui.selectable_label(am == AlphaMode::Blend, "Blend").clicked() {
                        am = AlphaMode::Blend;
                    }
                    if am != m.alpha_mode {
                        m.alpha_mode = am;
                        changed = true;
                    }
                });
                if m.alpha_mode == AlphaMode::Mask {
                    let r = ui.add(
                        egui::Slider::new(&mut m.alpha_cutoff, 0.0..=1.0).text("Alpha cutoff"),
                    );
                    if r.changed() {
                        changed = true;
                    }
                }

                if ui.checkbox(&mut m.double_sided, "Double-sided").changed() {
                    changed = true;
                }

                if changed {
                    editor.dirty_materials.push((name.clone(), m));
                }
            });
    }

    // === Spinner ===
    if world.has::<Spinner>(e) {
        egui::CollapsingHeader::new("Spinner")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(sp) = world.get_mut::<Spinner>(e) {
                    let mut axis = sp.axis.to_array();
                    ui.label("Axis");
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            ui.add(
                                egui::DragValue::new(&mut axis[i])
                                    .speed(0.01)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                        }
                    });
                    sp.axis = Vec3::from_array(axis);
                    ui.add(
                        egui::DragValue::new(&mut sp.speed)
                            .speed(0.01)
                            .prefix("Speed "),
                    );
                }
            });
    }

    // === Velocity ===
    if world.has::<Velocity>(e) {
        egui::CollapsingHeader::new("Velocity")
            .default_open(false)
            .show(ui, |ui| {
                if let Some(v) = world.get_mut::<Velocity>(e) {
                    let mut val = v.value.to_array();
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            ui.add(
                                egui::DragValue::new(&mut val[i])
                                    .speed(0.01)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                        }
                    });
                    v.value = Vec3::from_array(val);
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