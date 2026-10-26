//! Панели редактора (переходное состояние, Неделя 2, часть 1).
//!
//! Что здесь осталось:
//! * Inspector (`draw_inspector_panel` + все `inspector_*`).
//! * Renderer panel (`draw_renderer_panel` + `preset_*` + Play/Fly settings).
//! * Prefabs + Assets секции (для `left_panel`).
//! * Command palette.
//! * Component chips / Add-Component menu.
//!
//! Что переехало в `panels/`:
//! * `Stats`, `UiAssets`, `AudioSnapshot` → `panels::*`.
//! * `draw_box_select_overlay`, `draw_pause_overlay`, `draw_context_menu`
//!   → `panels::overlays`.
//! * `draw_top_bar` → `panels::top_bar`.
//! * `draw_left_panel` → `panels::left_panel`.
//! * `draw_right_panel` → `panels::right_panel`.
//! * `HierarchySnapshot`, `draw_hierarchy_*`, `component_badges`,
//!   `entity_display_name`, `would_create_cycle` → `panels::outliner`.
//!
//! На **Неделе 2, часть 2** отсюда уедут `details`, `renderer`,
//! `content`, `scripts`, `command_palette`, `inspector_audio` — и файл
//! будет удалён полностью.

use crate::ecs::{Entity, World};
use crate::editor::gizmo::GizmoMode;
use crate::editor::panels::UiAssets;
use crate::editor::{EditorAction, EditorState};
use crate::game::audio::AudioSource;
use crate::game::components::{
    AnimationPlayer, Chase, Elevator, Health, Interactable, MaterialHandle, MeshHandle, Name,
    Parent, SkeletonHandle, SlidingDoor, Spinner, TextureTiling, Tint, Transform, Trigger,
    TriggerAction, Velocity, Visible,
};
use crate::game::lights::{DirectionalLight, PointLight};
use crate::physics::{BodyType, Collider, PhysicsMaterial, RigidBody};
use crate::render::{AlphaMode, Material, PostFx, Tonemapper};
use glam::Vec3;

// `AudioSnapshot`, `Stats`, `UiAssets` — импортированы для сигнатур
// функций (`draw_assets_section`, `draw_inspector_panel`), но реально
// их значения приходят из `App::redraw`.
//
// Silence unused-import warning, если все три используются только в
// сигнатурах:
#[allow(unused_imports)]
use crate::editor::panels as _panels;

// ============================================================
// Inspector panel
// ============================================================

pub fn draw_inspector_panel(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    world: &mut World,
    assets: &UiAssets<'_>,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Inspector");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Gizmo:");
        let m = &mut editor.gizmo.mode;
        if ui
            .selectable_label(*m == GizmoMode::Translate, "T (1)")
            .clicked()
        {
            *m = GizmoMode::Translate;
        }
        if ui
            .selectable_label(*m == GizmoMode::Rotate, "R (2)")
            .clicked()
        {
            *m = GizmoMode::Rotate;
        }
        if ui
            .selectable_label(*m == GizmoMode::Scale, "S (3)")
            .clicked()
        {
            *m = GizmoMode::Scale;
        }
        ui.toggle_value(&mut editor.gizmo.snap_enabled, "Snap");
    });
    ui.separator();

    let n = editor.selected.len();
    if n == 0 {
        ui.label("Nothing selected");
        return;
    }
    if n > 1 {
        ui.label(format!("{} objects selected", n));
        ui.separator();
        ui.horizontal(|ui| {
            if ui.button("Focus (F)").clicked() {
                *action = Some(EditorAction::FocusSelected);
            }
            if ui.button("Duplicate").clicked() {
                *action = Some(EditorAction::Duplicate);
            }
            if ui.button("Delete").clicked() {
                *action = Some(EditorAction::DeleteSelected);
            }
        });
        ui.separator();
        draw_multi_edit(ui, world, editor);
        ui.separator();
        if ui.button("Deselect all").clicked() {
            editor.selected.clear();
        }
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
    draw_component_chips(ui, world, e, editor);
    ui.separator();

    if world.has::<Transform>(e) {
        inspector_transform(ui, world, e, editor);
    }
    if world.has::<DirectionalLight>(e) {
        inspector_directional_light(ui, world, e, editor);
    }
    if world.has::<PointLight>(e) {
        inspector_point_light(ui, world, e, editor);
    }
    if world.has::<crate::game::decals::Decal>(e) {
        inspector_decal(ui, world, e, editor);
    }
    if world.has::<Parent>(e) || editor.selected.len() >= 2 {
        inspector_hierarchy(ui, world, e, editor);
    }
    if world.has::<MeshHandle>(e) || world.has::<MaterialHandle>(e) {
        inspector_geometry(ui, world, e, assets);
    }
    if world.has::<TextureTiling>(e) {
        inspector_texture_tiling(ui, world, e, editor);
    }
    if world.has::<Elevator>(e) {
        inspector_elevator(ui, world, e, editor);
    }
    if world.has::<SlidingDoor>(e) {
        inspector_sliding_door(ui, world, e, editor);
    }
    if world.has::<AudioSource>(e) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("🔊 AudioSource — см. окно")
                    .small()
                    .weak()
                    .italics(),
            );
        });
    }
    if let Some((name, original)) = &assets.selected_material {
        inspector_material(ui, world, e, editor, action, assets, name, original);
    }
    if world.has::<Tint>(e) {
        inspector_tint(ui, world, e, editor);
    }
    if world.has::<Health>(e) {
        inspector_health(ui, world, e);
    }
    if world.has::<Chase>(e) {
        inspector_chase(ui, world, e);
    }
    if world.has::<Spinner>(e) {
        inspector_spinner(ui, world, e);
    }
    if world.has::<Velocity>(e) {
        inspector_velocity(ui, world, e);
    }
    if world.has::<RigidBody>(e) {
        inspector_rigidbody(ui, world, e, editor);
    }
    if world.has::<Collider>(e) {
        inspector_collider(ui, world, e, editor);
    }
    if world.has::<PhysicsMaterial>(e) {
        inspector_physics_material(ui, world, e, editor);
    }

    ui.separator();
    if ui.button("Deselect").clicked() {
        editor.selected.clear();
    }
}

// ============================================================
// Directional / Point / Decal
// ============================================================

fn inspector_directional_light(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("☀ Directional Light")
        .default_open(true)
        .show(ui, |ui| {
            let Some(light) = world.get_mut::<DirectionalLight>(e) else {
                return;
            };
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Sun").clicked() {
                    *light = DirectionalLight::sun();
                    editor.undo_requested = true;
                }
                if ui.small_button("Fill").clicked() {
                    *light = DirectionalLight::fill();
                    editor.undo_requested = true;
                }
                if ui.small_button("Moon").clicked() {
                    *light = DirectionalLight {
                        direction: Vec3::new(-0.3, 0.8, 0.2),
                        color: [0.55, 0.65, 0.95],
                        intensity: 0.4,
                    };
                    editor.undo_requested = true;
                }
                if ui.small_button("Sunset").clicked() {
                    *light = DirectionalLight {
                        direction: Vec3::new(1.0, 0.15, 0.0),
                        color: [1.0, 0.55, 0.25],
                        intensity: 1.5,
                    };
                    editor.undo_requested = true;
                }
            });
            ui.separator();
            let mut dir_arr = light.direction.to_array();
            ui.label("Direction");
            ui.horizontal(|ui| {
                for i in 0..3 {
                    let r = ui.add(
                        egui::DragValue::new(&mut dir_arr[i])
                            .speed(0.01)
                            .prefix(["X ", "Y ", "Z "][i]),
                    );
                    if r.drag_started() || r.gained_focus() {
                        editor.undo_requested = true;
                    }
                }
            });
            let new_dir = Vec3::from_array(dir_arr);
            if new_dir.length_squared() > 1e-6 {
                light.direction = new_dir.normalize();
            }
            ui.horizontal(|ui| {
                ui.label("Color:");
                if ui.color_edit_button_rgb(&mut light.color).changed() {
                    editor.undo_requested = true;
                }
            });
            let r = ui.add(
                egui::Slider::new(&mut light.intensity, 0.0..=5.0)
                    .logarithmic(true)
                    .text("Intensity"),
            );
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
        });
}

fn inspector_point_light(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("💡 Point Light")
        .default_open(true)
        .show(ui, |ui| {
            let Some(light) = world.get_mut::<PointLight>(e) else {
                return;
            };
            ui.horizontal_wrapped(|ui| {
                if ui.small_button("Warm").clicked() {
                    *light = PointLight::new([1.0, 0.6, 0.3], 4.0, 15.0);
                    editor.undo_requested = true;
                }
                if ui.small_button("Cool").clicked() {
                    *light = PointLight::new([0.4, 0.7, 1.0], 3.0, 12.0);
                    editor.undo_requested = true;
                }
                if ui.small_button("Red").clicked() {
                    *light = PointLight::new([1.0, 0.15, 0.15], 5.0, 10.0);
                    editor.undo_requested = true;
                }
                if ui.small_button("Green").clicked() {
                    *light = PointLight::new([0.2, 1.0, 0.3], 5.0, 10.0);
                    editor.undo_requested = true;
                }
                if ui.small_button("Candle").clicked() {
                    *light = PointLight::new([1.0, 0.55, 0.2], 1.5, 6.0);
                    editor.undo_requested = true;
                }
            });
            ui.separator();
            ui.horizontal(|ui| {
                ui.label("Color:");
                if ui.color_edit_button_rgb(&mut light.color).changed() {
                    editor.undo_requested = true;
                }
            });
            let r = ui.add(
                egui::Slider::new(&mut light.intensity, 0.0..=20.0)
                    .logarithmic(true)
                    .text("Intensity"),
            );
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
            let r = ui.add(
                egui::Slider::new(&mut light.range, 0.5..=100.0)
                    .logarithmic(true)
                    .text("Range (m)"),
            );
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
        });
}

fn inspector_decal(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("🎨 Decal")
        .default_open(true)
        .show(ui, |ui| {
            let Some(dec) = world.get_mut::<crate::game::decals::Decal>(e) else {
                return;
            };
            ui.horizontal(|ui| {
                ui.label("Texture:");
                ui.add(
                    egui::TextEdit::singleline(&mut dec.texture)
                        .desired_width(140.0)
                        .hint_text("blood"),
                );
            });
            ui.label("Tint (RGB + Alpha)");
            if ui
                .color_edit_button_rgba_unmultiplied(&mut dec.tint)
                .changed()
            {
                editor.undo_requested = true;
            }
            ui.horizontal(|ui| {
                if ui.small_button("Small (0.5m)").clicked() {
                    if let Some(t) = world.get_mut::<Transform>(e) {
                        t.scale = Vec3::splat(0.5);
                    }
                    editor.undo_requested = true;
                }
                if ui.small_button("Medium (1m)").clicked() {
                    if let Some(t) = world.get_mut::<Transform>(e) {
                        t.scale = Vec3::splat(1.0);
                    }
                    editor.undo_requested = true;
                }
                if ui.small_button("Large (2m)").clicked() {
                    if let Some(t) = world.get_mut::<Transform>(e) {
                        t.scale = Vec3::splat(2.0);
                    }
                    editor.undo_requested = true;
                }
            });
        });
}

// ============================================================
// Component chips
// ============================================================

fn draw_component_chips(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
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
                    ui.close();
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
}

// ============================================================
// Transform
// ============================================================

fn inspector_transform(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Transform")
        .default_open(true)
        .show(ui, |ui| {
            let Some(t) = world.get_mut::<Transform>(e) else {
                return;
            };
            ui.horizontal(|ui| {
                if ui.button("Copy").clicked() {
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

fn draw_vec3_row(
    ui: &mut egui::Ui,
    label: &str,
    v: &mut Vec3,
    speed: f32,
    editor: &mut EditorState,
) {
    let mut arr = v.to_array();
    ui.label(label);
    let mut changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(
                egui::DragValue::new(&mut arr[i])
                    .speed(speed)
                    .prefix(["X ", "Y ", "Z "][i]),
            );
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
            if r.changed() {
                changed = true;
            }
        }
    });
    if changed {
        *v = Vec3::from_array(arr);
    }
}

fn draw_euler_row(
    ui: &mut egui::Ui,
    label: &str,
    q: &mut glam::Quat,
    editor: &mut EditorState,
) {
    let (ry, rx, rz) = q.to_euler(glam::EulerRot::YXZ);
    let mut e = [rx.to_degrees(), ry.to_degrees(), rz.to_degrees()];
    ui.label(label);
    let mut changed = false;
    ui.horizontal(|ui| {
        for i in 0..3 {
            let r = ui.add(
                egui::DragValue::new(&mut e[i])
                    .speed(0.5)
                    .prefix(["X ", "Y ", "Z "][i]),
            );
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
            if r.changed() {
                changed = true;
            }
        }
    });
    if changed {
        *q = glam::Quat::from_euler(
            glam::EulerRot::YXZ,
            e[1].to_radians(),
            e[0].to_radians(),
            e[2].to_radians(),
        );
    }
}

// ============================================================
// Hierarchy
// ============================================================

fn inspector_hierarchy(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Hierarchy")
        .default_open(true)
        .show(ui, |ui| {
            match world.get::<Parent>(e).copied() {
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
            if editor.selected.len() >= 2
                && ui.button("Set parent from last selected").clicked()
            {
                let primary = *editor.selected.last().unwrap();
                if primary != e && !would_create_cycle(world, e, primary) {
                    editor.undo_requested = true;
                    world.insert(e, Parent(primary));
                }
            }
            ui.separator();
            let children: Vec<Entity> = world
                .entities()
                .iter()
                .copied()
                .filter(|&c| {
                    world
                        .get::<Parent>(c)
                        .map(|p| p.0 == e)
                        .unwrap_or(false)
                })
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

// ============================================================
// Geometry / materials / texture tiling
// ============================================================

fn inspector_geometry(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    assets: &UiAssets<'_>,
) {
    egui::CollapsingHeader::new("Geometry")
        .default_open(true)
        .show(ui, |ui| {
            if let Some(mh) = world.get_mut::<MeshHandle>(e) {
                let mut current = mh.0.clone();
                ui.horizontal(|ui| {
                    ui.label("Mesh:");
                    egui::ComboBox::from_id_salt("mesh_selector")
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
            }
            if let Some(mh) = world.get_mut::<MaterialHandle>(e) {
                let mut current = mh.0.clone();
                ui.horizontal(|ui| {
                    ui.label("Material:");
                    egui::ComboBox::from_id_salt("mat_selector")
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
            }
        });
}

fn inspector_texture_tiling(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Texture Tiling")
        .default_open(true)
        .show(ui, |ui| {
            let Some(t) = world.get_mut::<TextureTiling>(e) else {
                return;
            };
            let r = ui.add(
                egui::Slider::new(&mut t.size, 0.05..=50.0)
                    .logarithmic(true)
                    .text("Tile size"),
            );
            if r.drag_started() || r.gained_focus() {
                editor.undo_requested = true;
            }
            ui.horizontal(|ui| {
                if ui.small_button("Fine (0.25)").clicked() {
                    t.size = 0.25;
                    editor.undo_requested = true;
                }
                if ui.small_button("Default (1.0)").clicked() {
                    t.size = 1.0;
                    editor.undo_requested = true;
                }
                if ui.small_button("Coarse (4.0)").clicked() {
                    t.size = 4.0;
                    editor.undo_requested = true;
                }
            });
        });
}

// ============================================================
// Elevator / Sliding Door
// ============================================================

fn inspector_elevator(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Elevator")
        .default_open(true)
        .show(ui, |ui| {
            let Some(el) = world.get_mut::<Elevator>(e) else {
                return;
            };
            let mut floors = el.floors.clone();
            let floor_count = floors.len();
            let mut remove: Option<usize> = None;
            ui.label(format!("Floors: {}", floor_count));
            for (i, y) in floors.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    ui.label(format!("#{}:", i));
                    let r = ui.add(egui::DragValue::new(y).speed(0.1).suffix(" m"));
                    if r.drag_started() || r.gained_focus() {
                        editor.undo_requested = true;
                    }
                    if floor_count > 2 && ui.small_button("✖").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if floors != el.floors {
                el.floors = floors;
                editor.undo_requested = true;
            }
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
            ui.add(egui::Slider::new(&mut el.speed, 0.1..=8.0).text("Max speed (m/s)"));
            ui.add(
                egui::Slider::new(&mut el.acceleration, 0.5..=15.0)
                    .logarithmic(true)
                    .text("Acceleration"),
            );
            ui.add(egui::Slider::new(&mut el.door_speed, 0.2..=5.0).text("Door speed"));
            ui.add(egui::Slider::new(&mut el.dwell, 0.0..=10.0).text("Dwell (s)"));
            ui.add(egui::Slider::new(&mut el.sensor_radius, 0.5..=5.0).text("Sensor radius"));
            ui.separator();
            ui.horizontal(|ui| {
                ui.label(format!("State: {:?}", el.state));
                ui.label(format!(
                    "Floor: {}/{}",
                    el.current_floor,
                    el.floors.len().saturating_sub(1)
                ));
            });
            ui.horizontal(|ui| {
                ui.label(format!("Velocity: {:+.2} m/s", el.current_velocity));
                ui.label(format!("Doors: {:.0}%", el.doors_open * 100.0));
            });
            ui.separator();
            ui.label("Call elevator:");
            let floors_snapshot: Vec<f32> = el.floors.clone();
            let mut call_floor: Option<usize> = None;
            ui.horizontal_wrapped(|ui| {
                for (i, &y) in floors_snapshot.iter().enumerate() {
                    if ui
                        .small_button(format!("F{} ({:.1}m)", i, y))
                        .clicked()
                    {
                        call_floor = Some(i);
                    }
                }
            });
            if let Some(i) = call_floor {
                el.call(i);
                editor.undo_requested = true;
            }
        });
}

fn inspector_sliding_door(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Sliding Door")
        .default_open(true)
        .show(ui, |ui| {
            let Some(sd) = world.get_mut::<SlidingDoor>(e) else {
                return;
            };
            ui.add(egui::Slider::new(&mut sd.open_amount, 0.0..=1.0).text("Open amount"));
            ui.add(egui::Slider::new(&mut sd.target, 0.0..=1.0).text("Target"));
            ui.add(egui::Slider::new(&mut sd.speed, 0.1..=5.0).text("Speed"));
            ui.add(egui::Slider::new(&mut sd.slide_distance, 0.1..=5.0).text("Slide distance"));
            ui.label("Slide axis (local)");
            let mut ax = sd.slide_axis.to_array();
            ui.horizontal(|ui| {
                for i in 0..3 {
                    let r = ui.add(
                        egui::DragValue::new(&mut ax[i])
                            .speed(0.01)
                            .prefix(["X ", "Y ", "Z "][i]),
                    );
                    if r.changed() {
                        editor.undo_requested = true;
                    }
                }
            });
            sd.slide_axis = Vec3::from_array(ax).normalize_or_zero();
            ui.label("Closed position (local, rel. Parent)");
            let mut cp = sd.closed_position.to_array();
            ui.horizontal(|ui| {
                for i in 0..3 {
                    let r = ui.add(
                        egui::DragValue::new(&mut cp[i])
                            .speed(0.01)
                            .prefix(["X ", "Y ", "Z "][i]),
                    );
                    if r.changed() {
                        editor.undo_requested = true;
                    }
                }
            });
            sd.closed_position = Vec3::from_array(cp);
        });
}

// ============================================================
// Material
// ============================================================

#[allow(clippy::too_many_arguments)]
fn inspector_material(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
    action: &mut Option<EditorAction>,
    assets: &UiAssets<'_>,
    name: &str,
    original: &Material,
) {
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
            if ui
                .color_edit_button_rgba_unmultiplied(&mut m.base_color)
                .changed()
            {
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
            if ui.color_edit_button_rgb(&mut m.emissive).changed() {
                changed = true;
                editor.undo_requested = true;
            }
            ui.separator();
            ui.label(egui::RichText::new("Textures").strong());
            if texture_picker(
                ui,
                "mat_base_tex",
                "Base color:",
                &mut m.base_color_texture,
                assets.texture_list,
            ) {
                changed = true;
                editor.undo_requested = true;
            }
            if texture_picker(
                ui,
                "mat_mr_tex",
                "Metallic-Rough:",
                &mut m.metallic_roughness_texture,
                assets.texture_list,
            ) {
                changed = true;
                editor.undo_requested = true;
            }
            if texture_picker(
                ui,
                "mat_normal_tex",
                "Normal map:",
                &mut m.normal_texture,
                assets.texture_list,
            ) {
                changed = true;
                editor.undo_requested = true;
            }
            if texture_picker(
                ui,
                "mat_emissive_tex",
                "Emissive map:",
                &mut m.emissive_texture,
                assets.texture_list,
            ) {
                changed = true;
                editor.undo_requested = true;
            }
            ui.separator();
            ui.label("Alpha mode");
            ui.horizontal(|ui| {
                let mut am = m.alpha_mode;
                if ui
                    .selectable_label(am == AlphaMode::Opaque, "Opaque")
                    .clicked()
                {
                    am = AlphaMode::Opaque;
                }
                if ui
                    .selectable_label(am == AlphaMode::Mask, "Mask")
                    .clicked()
                {
                    am = AlphaMode::Mask;
                }
                if ui
                    .selectable_label(am == AlphaMode::Blend, "Blend")
                    .clicked()
                {
                    am = AlphaMode::Blend;
                }
                if am != m.alpha_mode {
                    m.alpha_mode = am;
                    changed = true;
                }
            });
            if m.alpha_mode == AlphaMode::Mask {
                if ui
                    .add(egui::Slider::new(&mut m.alpha_cutoff, 0.0..=1.0).text("Alpha cutoff"))
                    .changed()
                {
                    changed = true;
                }
            }
            if ui
                .checkbox(&mut m.double_sided, "Double-sided")
                .changed()
            {
                changed = true;
            }
            if changed {
                editor.dirty_materials.push((e, name.to_string(), m));
            }
            let _ = world;
        });
}

// ============================================================
// Misc components
// ============================================================

fn inspector_tint(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Tint")
        .default_open(true)
        .show(ui, |ui| {
            let Some(t) = world.get_mut::<Tint>(e) else {
                return;
            };
            let mut color = t.0;
            ui.horizontal(|ui| {
                ui.label("Color:");
                if ui
                    .color_edit_button_rgba_unmultiplied(&mut color)
                    .changed()
                {
                    t.0 = color;
                    editor.undo_requested = true;
                }
                if ui.small_button("White").clicked() {
                    t.0 = [1.0, 1.0, 1.0, 1.0];
                    editor.undo_requested = true;
                }
            });
        });
}

fn inspector_health(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Health")
        .default_open(true)
        .show(ui, |ui| {
            let Some(h) = world.get_mut::<Health>(e) else {
                return;
            };
            ui.add(egui::Slider::new(&mut h.max, 1.0..=1000.0).text("Max"));
            ui.add(egui::Slider::new(&mut h.current, 0.0..=h.max).text("Current"));
        });
}

fn inspector_chase(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Chase")
        .default_open(true)
        .show(ui, |ui| {
            let Some(c) = world.get_mut::<Chase>(e) else {
                return;
            };
            ui.add(egui::Slider::new(&mut c.speed, 0.1..=20.0).text("Speed"));
            ui.add(egui::Slider::new(&mut c.stop_distance, 0.1..=10.0).text("Stop dist"));
        });
}

fn inspector_spinner(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Spinner")
        .default_open(true)
        .show(ui, |ui| {
            let Some(sp) = world.get_mut::<Spinner>(e) else {
                return;
            };
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
            ui.add(egui::DragValue::new(&mut sp.speed).speed(0.01).prefix("Speed "));
        });
}

fn inspector_velocity(ui: &mut egui::Ui, world: &mut World, e: Entity) {
    egui::CollapsingHeader::new("Velocity")
        .default_open(true)
        .show(ui, |ui| {
            let Some(v) = world.get_mut::<Velocity>(e) else {
                return;
            };
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
        });
}

fn inspector_rigidbody(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("RigidBody")
        .default_open(true)
        .show(ui, |ui| {
            let has_parent = world.has::<Parent>(e);
            if has_parent {
                if let Some(rb) = world.get::<RigidBody>(e) {
                    if rb.body_type != BodyType::Static {
                        ui.label(
                            egui::RichText::new("⚠ Physics disabled: has Parent")
                                .strong()
                                .color(egui::Color32::from_rgb(240, 130, 130)),
                        );
                    }
                }
            }
            let Some(rb) = world.get_mut::<RigidBody>(e) else {
                return;
            };
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
            match rb.body_type {
                BodyType::Dynamic => {
                    ui.add(
                        egui::Slider::new(&mut rb.mass, 0.01..=100.0)
                            .logarithmic(true)
                            .text("Mass"),
                    );
                    ui.add(
                        egui::Slider::new(&mut rb.gravity_scale, 0.0..=3.0)
                            .text("Gravity scale"),
                    );
                    ui.add(
                        egui::Slider::new(&mut rb.linear_damping, 0.0..=1.0)
                            .text("Linear damping"),
                    );
                    let mut v = rb.velocity.to_array();
                    ui.label("Velocity");
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            ui.add(
                                egui::DragValue::new(&mut v[i])
                                    .speed(0.1)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                        }
                    });
                    rb.velocity = Vec3::from_array(v);
                    if ui.small_button("Reset velocity").clicked() {
                        rb.velocity = Vec3::ZERO;
                        changed = true;
                    }
                }
                BodyType::Kinematic => {
                    let mut v = rb.velocity.to_array();
                    ui.label("Kinematic velocity");
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            ui.add(
                                egui::DragValue::new(&mut v[i])
                                    .speed(0.1)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                        }
                    });
                    rb.velocity = Vec3::from_array(v);
                }
                BodyType::Static => {
                    ui.label(
                        egui::RichText::new("Static: infinite mass, not moving.")
                            .small()
                            .weak(),
                    );
                }
            }
            if changed {
                editor.undo_requested = true;
            }
        });
}

fn inspector_collider(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("Collider")
        .default_open(true)
        .show(ui, |ui| {
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
            let Some(col) = world.get_mut::<Collider>(e) else {
                return;
            };
            match col {
                Collider::Sphere { radius } => {
                    ui.add(egui::Slider::new(radius, 0.05..=20.0).text("Radius"));
                }
                Collider::Aabb { half_extents } => {
                    let mut h = half_extents.to_array();
                    ui.label("Half extents");
                    ui.horizontal(|ui| {
                        for i in 0..3 {
                            ui.add(
                                egui::DragValue::new(&mut h[i])
                                    .speed(0.01)
                                    .prefix(["X ", "Y ", "Z "][i]),
                            );
                        }
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

fn inspector_physics_material(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    egui::CollapsingHeader::new("PhysicsMaterial")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("Presets:");
            ui.horizontal_wrapped(|ui| {
                let presets: [(&str, PhysicsMaterial); 5] = [
                    ("Wood", PhysicsMaterial::wood()),
                    ("Metal", PhysicsMaterial::metal()),
                    ("Rubber", PhysicsMaterial::rubber()),
                    ("Ice", PhysicsMaterial::ice()),
                    ("Concrete", PhysicsMaterial::concrete()),
                ];
                for (label, preset) in presets {
                    if ui.small_button(label).clicked() {
                        if let Some(m) = world.get_mut::<PhysicsMaterial>(e) {
                            *m = preset;
                            editor.undo_requested = true;
                        }
                    }
                }
            });
            ui.separator();
            let Some(m) = world.get_mut::<PhysicsMaterial>(e) else {
                return;
            };
            ui.add(egui::Slider::new(&mut m.restitution, 0.0..=1.0).text("Restitution"));
            ui.add(egui::Slider::new(&mut m.friction, 0.0..=2.0).text("Friction"));
        });
}

// ============================================================
// ComponentKind / remove_component / add_component_menu
// ============================================================

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

fn add_component_menu(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
) {
    use crate::game::components::*;
    let mut any = false;

    if !world.has::<Transform>(e) {
        any = true;
        if ui.button("Transform").clicked() {
            world.insert(e, Transform::at(Vec3::ZERO));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<MeshHandle>(e) {
        any = true;
        if ui.button("Mesh").clicked() {
            world.insert(e, MeshHandle("cube".into()));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<MaterialHandle>(e) {
        any = true;
        if ui.button("Material").clicked() {
            world.insert(e, MaterialHandle("flat_blue".into()));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<Tint>(e) {
        any = true;
        if ui.button("Tint").clicked() {
            world.insert(e, Tint([1.0, 1.0, 1.0, 1.0]));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<Visible>(e) {
        any = true;
        if ui.button("Visible (false)").clicked() {
            world.insert(e, Visible(false));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<TextureTiling>(e) {
        any = true;
        if ui.button("Texture Tiling").clicked() {
            world.insert(e, TextureTiling::default());
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<AudioSource>(e) {
        any = true;
        ui.menu_button("🔊 Audio Source", |ui| {
            let presets = [
                ("pickup (SFX)", AudioSource::new("pickup").with_bus(crate::game::audio::AudioBus::Sfx).with_range(1.0, 20.0)),
                ("shot (SFX)",   AudioSource::new("shot").with_bus(crate::game::audio::AudioBus::Sfx).with_range(1.0, 30.0)),
                ("ding (Music)", AudioSource::new("ding").with_bus(crate::game::audio::AudioBus::Music).with_range(0.5, 15.0)),
                ("pickup (UI, non-spatial)", AudioSource::non_spatial("pickup").with_bus(crate::game::audio::AudioBus::Ui)),
                ("ambient loop (Music)", AudioSource::looping("ding").with_bus(crate::game::audio::AudioBus::Music).with_range(2.0, 25.0)),
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
                ("Melee (fast)", crate::game::ai::AiAgent::new().with_speed(4.0).with_vision(18.0, 60_f32.to_radians()).with_attack(1.5, 10.0, 0.8)),
                ("Brute (slow)", crate::game::ai::AiAgent::new().with_speed(2.0).with_vision(12.0, 45_f32.to_radians()).with_attack(2.0, 25.0, 1.5)),
                ("Sniper (range)", crate::game::ai::AiAgent::new().with_speed(3.0).with_vision(30.0, 30_f32.to_radians()).with_attack(8.0, 15.0, 2.0)),
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
        if ui.button("👹 Enemy marker").clicked() {
            world.insert(e, crate::game::ai::Enemy);
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<crate::game::ai::AiTarget>(e) {
        any = true;
        if ui.button("🎯 AI Target").clicked() {
            world.insert(e, crate::game::ai::AiTarget);
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<crate::game::ai::PatrolPath>(e) {
        any = true;
        if ui.button("🚶 Patrol Path (circle)").clicked() {
            let origin = world
                .get::<Transform>(e)
                .map(|t| t.position)
                .unwrap_or(Vec3::ZERO);
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
        if ui.button("Elevator (2 floors)").clicked() {
            world.insert(e, Elevator::new(vec![0.0, 3.0], 2.0));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<SlidingDoor>(e) {
        any = true;
        if ui.button("Sliding Door").clicked() {
            world.insert(e, SlidingDoor::new(Vec3::new(0.7, 1.0, 1.5), Vec3::X, 1.4));
            editor.undo_requested = true;
            ui.close();
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
            ui.close();
        }
    }
    if !world.has::<Velocity>(e) {
        any = true;
        if ui.button("Velocity").clicked() {
            world.insert(e, Velocity::new(0.0, 0.0, 0.0));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<Chase>(e) {
        any = true;
        if ui.button("Chase (legacy)").clicked() {
            world.insert(e, Chase::new(3.0, 1.2));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<Parent>(e) {
        any = true;
        if ui.button("Parent (self-id)").clicked() {
            world.insert(e, Parent(e));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<Health>(e) || !world.has::<Interactable>(e) || !world.has::<Trigger>(e) {
        ui.separator();
    }
    if !world.has::<Health>(e) {
        any = true;
        if ui.button("Health").clicked() {
            world.insert(e, Health::new(100.0));
            editor.undo_requested = true;
            ui.close();
        }
    }
    if !world.has::<Interactable>(e) {
        any = true;
        ui.menu_button("Interactable", |ui| {
            if ui.button("Pickup").clicked() {
                world.insert(e, Interactable::Pickup);
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("Paint (red)").clicked() {
                world.insert(e, Interactable::Paint([1.0, 0.0, 0.0, 1.0]));
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("Toggle").clicked() {
                world.insert(e, Interactable::Toggle);
                editor.undo_requested = true;
                ui.close();
            }
        });
    }
    if !world.has::<Trigger>(e) {
        any = true;
        ui.menu_button("Trigger", |ui| {
            if ui.button("Teleport to (0, 2, 0)").clicked() {
                world.insert(
                    e,
                    Trigger::new(2.5, TriggerAction::Teleport([0.0, 2.0, 0.0])),
                );
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("Tint (green)").clicked() {
                world.insert(
                    e,
                    Trigger::new(2.5, TriggerAction::Tint([0.2, 1.0, 0.2, 1.0])),
                );
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("Despawn").clicked() {
                world.insert(e, Trigger::new(2.5, TriggerAction::Despawn));
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("Play Sound (pickup)").clicked() {
                world.insert(
                    e,
                    Trigger::repeatable(2.5, TriggerAction::PlaySound("pickup".to_string())),
                );
                editor.undo_requested = true;
                ui.close();
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
            ui.close();
        }
    }
    if !world.has::<SkeletonHandle>(e) {
        any = true;
        if ui.button("Skeleton Handle").clicked() {
            world.insert(e, SkeletonHandle(String::new()));
            editor.undo_requested = true;
            ui.close();
        }
    }
    ui.separator();
    if !world.has::<RigidBody>(e) {
        any = true;
        let has_parent = world.has::<Parent>(e);
        ui.menu_button("RigidBody", |ui| {
            if ui.button("Static").clicked() {
                world.insert(e, RigidBody::static_body());
                editor.undo_requested = true;
                ui.close();
            }
            if ui
                .add_enabled(!has_parent, egui::Button::new("Dynamic (mass 1)"))
                .clicked()
            {
                world.insert(e, RigidBody::dynamic(1.0));
                editor.undo_requested = true;
                ui.close();
            }
            if ui
                .add_enabled(!has_parent, egui::Button::new("Kinematic"))
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
            if ui.button("Sphere (r = 0.5)").clicked() {
                world.insert(e, Collider::sphere(0.5));
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("AABB (1×1×1)").clicked() {
                world.insert(e, Collider::aabb(Vec3::splat(0.5)));
                editor.undo_requested = true;
                ui.close();
            }
            if ui.button("Capsule (r = 0.35, h = 1.8)").clicked() {
                world.insert(e, Collider::capsule(0.35, 1.8));
                editor.undo_requested = true;
                ui.close();
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
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    ui.label(egui::RichText::new("Lights").strong());
    if !world.has::<DirectionalLight>(e) {
        any = true;
        if ui.button("☀ Directional Light").clicked() {
            world.insert(e, DirectionalLight::sun());
            editor.undo_requested = true;
            ui.close();
        }
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
                if ui.button(label).clicked() {
                    world.insert(e, l);
                    editor.undo_requested = true;
                    ui.close();
                }
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
        ui.label(
            egui::RichText::new("All components already present")
                .weak()
                .italics(),
        );
    }
}

// ============================================================
// Renderer panel + presets
// ============================================================

fn apply_preset(postfx: &mut PostFx, mut preset: PostFx) {
    preset.debug_view = postfx.debug_view;
    *postfx = preset;
}

fn preset_low() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.0;
    p.ssao_strength = 0.0;
    p.ibl_strength = 0.15;
    p.taa_strength = 0.0;
    p.taa_sharpening = 0.0;
    p.fxaa_strength = 1.0;
    p.fog_density = 0.0;
    p.volumetric_density = 0.0;
    p
}

fn preset_medium() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.4;
    p.ssao_strength = 0.5;
    p.ibl_strength = 0.3;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.05;
    p.fxaa_strength = 0.7;
    p.volumetric_density = 0.015;
    p.volumetric_scattering = 0.35;
    p.volumetric_phase_g = 0.5;
    p
}

fn preset_high() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.6;
    p.ssao_strength = 0.8;
    p.ibl_strength = 0.35;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.1;
    p.fxaa_strength = 0.5;
    p.volumetric_density = 0.025;
    p.volumetric_scattering = 0.4;
    p.volumetric_phase_g = 0.6;
    p
}

fn preset_ultra() -> PostFx {
    let mut p = PostFx::default();
    p.bloom_strength = 0.8;
    p.bloom_knee = 0.4;
    p.ssao_strength = 1.0;
    p.ssao_radius = 0.8;
    p.ibl_strength = 0.45;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.15;
    p.fxaa_strength = 0.3;
    p.vignette_strength = 0.15;
    p.volumetric_density = 0.04;
    p.volumetric_scattering = 0.5;
    p.volumetric_phase_g = 0.7;
    p
}

fn preset_rage_cinematic() -> PostFx {
    let mut p = PostFx::default();
    p.tonemapper = Tonemapper::ACESFilmic;
    p.exposure = 0.55;
    p.exposure_bias = 1.05;

    p.color_temperature = 0.30;
    p.color_tint = 0.05;
    p.color_contrast = 1.18;
    p.color_saturation = 0.88;
    p.color_lift = [0.005, 0.0, -0.005];
    p.color_gain = [1.05, 1.0, 0.92];
    p.color_gamma = [0.98, 1.0, 1.02];

    p.bloom_threshold = 1.1;
    p.bloom_strength = 0.55;
    p.bloom_knee = 0.55;
    p.bloom_radius = 1.2;

    p.fog_color = [0.65, 0.48, 0.35];
    p.fog_density = 0.010;
    p.fog_height_base = 0.0;
    p.fog_height_falloff = 0.08;
    p.volumetric_density = 0.018;
    p.volumetric_scattering = 0.45;
    p.volumetric_phase_g = 0.65;

    p.ssao_strength = 0.85;
    p.ssao_radius = 0.7;
    p.ibl_strength = 0.35;

    p.taa_strength = 1.0;
    p.taa_sharpening = 0.08;
    p.fxaa_strength = 0.4;

    p.vignette_strength = 0.22;
    p.film_grain = 0.04;
    p.chromatic_aberration = 0.02;

    p.dof_enabled = 1.0;
    p.dof_focus_distance = 12.0;
    p.dof_focus_range = 4.0;
    p.dof_max_blur = 6.0;
    p.dof_blur_falloff = 5.0;

    p.motion_blur_enabled = 1.0;
    p.motion_blur_intensity = 0.55;
    p.motion_blur_max_px = 32.0;
    p.motion_blur_samples = 12;

    p.lens_flare_intensity = 0.35;
    p.lens_flare_threshold = 2.2;
    p.lens_flare_ghosts = 5;
    p.lens_flare_streak = 0.10;

    p.shadow_bias = 0.0016;
    p.shadow_normal_bias = 3.5;
    p.shadow_fade_start = 140.0;
    p.shadow_fade_end = 200.0;

    p
}

fn preset_desert_storm() -> PostFx {
    let mut p = preset_rage_cinematic();
    p.exposure_bias = 0.9;
    p.color_temperature = 0.20;
    p.color_tint = -0.15;
    p.color_contrast = 1.10;
    p.color_saturation = 0.62;
    p.fog_density = 0.030;
    p.fog_color = [0.72, 0.58, 0.42];
    p.volumetric_density = 0.045;
    p.volumetric_scattering = 0.55;
    p.volumetric_phase_g = 0.75;
    p.vignette_strength = 0.30;
    p.film_grain = 0.06;
    p.lens_flare_intensity = 0.20;
    p.bloom_strength = 0.45;
    p
}

fn preset_night_urban() -> PostFx {
    let mut p = PostFx::default();
    p.tonemapper = Tonemapper::AgX;
    p.exposure = 0.75;
    p.exposure_bias = 1.0;
    p.color_temperature = -0.35;
    p.color_tint = 0.05;
    p.color_contrast = 1.22;
    p.color_saturation = 0.92;
    p.color_lift = [-0.01, 0.0, 0.012];
    p.color_gain = [0.94, 1.0, 1.08];
    p.bloom_threshold = 0.9;
    p.bloom_strength = 0.95;
    p.bloom_knee = 0.4;
    p.bloom_radius = 1.4;
    p.fog_color = [0.18, 0.22, 0.35];
    p.fog_density = 0.015;
    p.volumetric_density = 0.020;
    p.volumetric_scattering = 0.5;
    p.volumetric_phase_g = 0.7;
    p.ssao_strength = 1.1;
    p.ssao_radius = 0.8;
    p.ibl_strength = 0.15;
    p.taa_strength = 1.0;
    p.taa_sharpening = 0.12;
    p.fxaa_strength = 0.5;
    p.vignette_strength = 0.28;
    p.film_grain = 0.05;
    p.chromatic_aberration = 0.04;
    p.dof_enabled = 1.0;
    p.dof_focus_distance = 15.0;
    p.dof_focus_range = 5.0;
    p.dof_max_blur = 8.0;
    p.dof_blur_falloff = 6.0;
    p.motion_blur_enabled = 1.0;
    p.motion_blur_intensity = 0.6;
    p.motion_blur_max_px = 36.0;
    p.motion_blur_samples = 14;
    p.lens_flare_intensity = 0.55;
    p.lens_flare_threshold = 1.5;
    p.lens_flare_ghosts = 8;
    p.lens_flare_streak = 0.18;
    p
}

pub fn draw_renderer_panel(
    ui: &mut egui::Ui,
    postfx: &mut PostFx,
    editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    egui::CollapsingHeader::new("AI / Navmesh")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.button("🔨 Bake Navmesh").clicked() {
                    *action = Some(EditorAction::BakeNavmesh);
                }
            });
            ui.checkbox(&mut editor.settings.show_navmesh, "Show navmesh");
        });

    ui.separator();
    ui.label(egui::RichText::new("Quality preset").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("Low").clicked() { apply_preset(postfx, preset_low()); }
        if ui.button("Medium").clicked() { apply_preset(postfx, preset_medium()); }
        if ui.button("High").clicked() { apply_preset(postfx, preset_high()); }
        if ui.button("Ultra").clicked() { apply_preset(postfx, preset_ultra()); }
        if ui.button("Reset").clicked() { apply_preset(postfx, PostFx::default()); }
    });

    ui.separator();
    ui.label(egui::RichText::new("Style preset").strong());
    ui.horizontal_wrapped(|ui| {
        if ui.button("🎬 RAGE Cinematic").clicked() { apply_preset(postfx, preset_rage_cinematic()); }
        if ui.button("🌫 Desert Storm").clicked() { apply_preset(postfx, preset_desert_storm()); }
        if ui.button("🌆 Night Urban").clicked() { apply_preset(postfx, preset_night_urban()); }
    });

    ui.separator();
    egui::CollapsingHeader::new("Anti-aliasing")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.taa_strength, 0.0..=1.0).text("TAA strength"));
            ui.add(egui::Slider::new(&mut postfx.taa_sharpening, 0.0..=1.0).text("TAA sharpening"));
            ui.add(egui::Slider::new(&mut postfx.fxaa_strength, 0.0..=1.0).text("FXAA strength"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Post-processing")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.bloom_threshold, 0.1..=5.0).text("Bloom threshold"));
            ui.add(egui::Slider::new(&mut postfx.bloom_strength, 0.0..=3.0).text("Bloom strength"));
            ui.add(egui::Slider::new(&mut postfx.bloom_knee, 0.01..=2.0).text("Bloom knee"));
            ui.add(egui::Slider::new(&mut postfx.bloom_radius, 0.5..=3.0).text("Bloom radius"));
            ui.add(egui::Slider::new(&mut postfx.exposure, 0.1..=3.0).text("Exposure"));
        });

    ui.separator();
    egui::CollapsingHeader::new("🎨 Color grading")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Tonemapper:");
                let tm = &mut postfx.tonemapper;
                if ui.selectable_label(*tm == Tonemapper::AgX, "AgX").clicked() { *tm = Tonemapper::AgX; }
                if ui.selectable_label(*tm == Tonemapper::ACESFilmic, "ACES").clicked() { *tm = Tonemapper::ACESFilmic; }
                if ui.selectable_label(*tm == Tonemapper::Reinhard, "Reinhard").clicked() { *tm = Tonemapper::Reinhard; }
                if ui.selectable_label(*tm == Tonemapper::Uncharted2, "Uncharted2").clicked() { *tm = Tonemapper::Uncharted2; }
                if ui.selectable_label(*tm == Tonemapper::None, "None").clicked() { *tm = Tonemapper::None; }
            });
            ui.separator();
            ui.add(egui::Slider::new(&mut postfx.exposure_bias, 0.25..=4.0).logarithmic(true).text("Exposure bias"));
            ui.add(egui::Slider::new(&mut postfx.color_temperature, -1.0..=1.0).text("Temperature"));
            ui.add(egui::Slider::new(&mut postfx.color_tint, -1.0..=1.0).text("Tint"));
            ui.add(egui::Slider::new(&mut postfx.color_contrast, 0.5..=2.0).text("Contrast"));
            ui.add(egui::Slider::new(&mut postfx.color_saturation, 0.0..=2.0).text("Saturation"));
            ui.separator();
            ui.label(egui::RichText::new("ASC CDL").strong());
            ui.horizontal(|ui| {
                ui.label("Lift:");
                for i in 0..3 {
                    ui.add(
                        egui::DragValue::new(&mut postfx.color_lift[i])
                            .speed(0.005)
                            .range(-0.2..=0.2)
                            .prefix(["R ", "G ", "B "][i]),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Gain:");
                for i in 0..3 {
                    ui.add(
                        egui::DragValue::new(&mut postfx.color_gain[i])
                            .speed(0.005)
                            .range(0.0..=2.0)
                            .prefix(["R ", "G ", "B "][i]),
                    );
                }
            });
            ui.horizontal(|ui| {
                ui.label("Gamma:");
                for i in 0..3 {
                    ui.add(
                        egui::DragValue::new(&mut postfx.color_gamma[i])
                            .speed(0.005)
                            .range(0.1..=3.0)
                            .prefix(["R ", "G ", "B "][i]),
                    );
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
                if ui.button("Cinematic").clicked() {
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
                if ui.button("Cold horror").clicked() {
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

    ui.separator();
    egui::CollapsingHeader::new("✨ Lens flare")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.lens_flare_intensity, 0.0..=1.5).text("Intensity"));
            ui.add(egui::Slider::new(&mut postfx.lens_flare_threshold, 0.5..=8.0).logarithmic(true).text("Threshold"));
            let mut ghosts = postfx.lens_flare_ghosts as i32;
            if ui.add(egui::Slider::new(&mut ghosts, 0..=12).text("Ghost count")).changed() {
                postfx.lens_flare_ghosts = ghosts.max(0) as u32;
            }
            ui.add(egui::Slider::new(&mut postfx.lens_flare_streak, 0.0..=0.5).text("Streak length"));
            ui.horizontal_wrapped(|ui| {
                if ui.button("Reset").clicked() {
                    postfx.lens_flare_intensity = 0.0;
                    postfx.lens_flare_threshold = 1.5;
                    postfx.lens_flare_ghosts = 6;
                    postfx.lens_flare_streak = 0.15;
                }
                if ui.button("Cinematic").clicked() {
                    postfx.lens_flare_intensity = 0.4;
                    postfx.lens_flare_threshold = 2.0;
                    postfx.lens_flare_ghosts = 4;
                    postfx.lens_flare_streak = 0.12;
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("🎥 Depth of Field")
        .default_open(false)
        .show(ui, |ui| {
            let mut enabled = postfx.dof_enabled > 0.5;
            if ui.checkbox(&mut enabled, "Enable DOF").changed() {
                postfx.dof_enabled = if enabled { 1.0 } else { 0.0 };
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.add(egui::Slider::new(&mut postfx.dof_focus_distance, 0.5..=100.0).logarithmic(true).text("Focus distance (m)"));
                ui.add(egui::Slider::new(&mut postfx.dof_focus_range, 0.0..=30.0).text("Focus range (m)"));
                ui.add(egui::Slider::new(&mut postfx.dof_max_blur, 0.0..=32.0).text("Max blur (px)"));
                ui.add(egui::Slider::new(&mut postfx.dof_blur_falloff, 0.5..=20.0).logarithmic(true).text("Blur falloff"));
            });
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                if ui.button("Cinematic").clicked() {
                    postfx.dof_enabled = 1.0;
                    postfx.dof_focus_distance = 6.0;
                    postfx.dof_focus_range = 1.5;
                    postfx.dof_max_blur = 12.0;
                    postfx.dof_blur_falloff = 3.0;
                }
                if ui.button("Disable").clicked() {
                    postfx.dof_enabled = 0.0;
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("💨 Motion Blur")
        .default_open(false)
        .show(ui, |ui| {
            let mut enabled = postfx.motion_blur_enabled > 0.5;
            if ui.checkbox(&mut enabled, "Enable motion blur").changed() {
                postfx.motion_blur_enabled = if enabled { 1.0 } else { 0.0 };
            }
            ui.add_enabled_ui(enabled, |ui| {
                ui.add(egui::Slider::new(&mut postfx.motion_blur_intensity, 0.0..=2.0).text("Intensity"));
                ui.add(egui::Slider::new(&mut postfx.motion_blur_max_px, 4.0..=120.0).logarithmic(true).text("Max blur length (px)"));
                let mut samples = postfx.motion_blur_samples as i32;
                if ui.add(egui::Slider::new(&mut samples, 2..=24).text("Samples")).changed() {
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
                if ui.button("Cinematic").clicked() {
                    postfx.motion_blur_enabled = 1.0;
                    postfx.motion_blur_intensity = 0.8;
                    postfx.motion_blur_max_px = 48.0;
                    postfx.motion_blur_samples = 14;
                }
                if ui.button("Disable").clicked() {
                    postfx.motion_blur_enabled = 0.0;
                }
            });
        });

    ui.separator();
    egui::CollapsingHeader::new("SSAO")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.ssao_strength, 0.0..=2.0).text("Strength"));
            ui.add(egui::Slider::new(&mut postfx.ssao_radius, 0.05..=2.0).text("Radius"));
        });

    ui.separator();
    egui::CollapsingHeader::new("IBL")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.ibl_strength, 0.0..=3.0).text("IBL strength"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Fog (height)")
        .default_open(false)
        .show(ui, |ui| {
            let mut c = postfx.fog_color;
            if ui.color_edit_button_rgb(&mut c).changed() {
                postfx.fog_color = c;
            }
            ui.add(egui::Slider::new(&mut postfx.fog_density, 0.0..=0.2).text("Density"));
            ui.add(egui::Slider::new(&mut postfx.fog_height_base, -10.0..=20.0).text("Height base"));
            ui.add(egui::Slider::new(&mut postfx.fog_height_falloff, 0.0..=0.5).text("Height falloff"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Volumetric fog")
        .default_open(true)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.volumetric_density, 0.0..=0.02).logarithmic(true).text("Density"));
            ui.add(egui::Slider::new(&mut postfx.volumetric_scattering, 0.0..=1.0).text("Scattering"));
            ui.add(egui::Slider::new(&mut postfx.volumetric_phase_g, 0.0..=0.9).text("Phase g"));
        });

    ui.separator();
    egui::CollapsingHeader::new("Screen effects")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.vignette_strength, 0.0..=1.0).text("Vignette"));
            ui.add(egui::Slider::new(&mut postfx.film_grain, 0.0..=1.0).text("Film grain"));
            ui.add(egui::Slider::new(&mut postfx.chromatic_aberration, 0.0..=1.0).text("Chromatic ab."));
        });

    ui.separator();
    egui::CollapsingHeader::new("Shadows")
        .default_open(false)
        .show(ui, |ui| {
            ui.add(egui::Slider::new(&mut postfx.shadow_bias, 0.0..=0.01).text("Depth bias"));
            ui.add(egui::Slider::new(&mut postfx.shadow_normal_bias, 0.0..=10.0).text("Slope bias"));
            ui.add(egui::Slider::new(&mut postfx.shadow_fade_start, 50.0..=250.0).text("Fade start"));
            ui.add(egui::Slider::new(&mut postfx.shadow_fade_end, 60.0..=300.0).text("Fade end"));
        });

    ui.separator();
    egui::CollapsingHeader::new("LOD")
        .default_open(false)
        .show(ui, |ui| {
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
    egui::CollapsingHeader::new("Play")
        .default_open(false)
        .show(ui, |ui| {
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
            ui.add(egui::Slider::new(&mut p.coyote_time, 0.0..=0.5).text("Coyote time (s)").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut p.jump_buffer_time, 0.0..=0.5).text("Jump buffer (s)").fixed_decimals(2));
            ui.add(egui::Slider::new(&mut p.ground_accel_tau, 0.01..=0.3).text("Ground accel τ (s)").fixed_decimals(3));
            ui.add(egui::Slider::new(&mut p.air_accel_tau, 0.05..=1.0).text("Air accel τ (s)").fixed_decimals(2));

            ui.separator();
            ui.label("Player capsule");
            ui.add(egui::Slider::new(&mut p.player_radius, 0.1..=1.0).text("Radius"));
            ui.add(egui::Slider::new(&mut p.player_height, 0.5..=3.0).text("Height"));

            ui.separator();
            ui.label("Combat");
            ui.add(egui::Slider::new(&mut p.max_health, 10.0..=500.0).text("Max HP"));
            ui.add(egui::Slider::new(&mut p.max_ammo, 0..=500).text("Max ammo"));
            ui.add(egui::Slider::new(&mut p.damage_per_shot, 1.0..=200.0).text("Damage"));
            ui.add(egui::Slider::new(&mut p.gun_range, 5.0..=500.0).text("Range"));

            ui.separator();
            ui.checkbox(&mut p.show_crosshair, "Show crosshair");
            ui.checkbox(&mut p.show_hud, "Show HUD");
        });
}

fn draw_fly_settings(ui: &mut egui::Ui, editor: &mut EditorState) {
    egui::CollapsingHeader::new("Fly (RMB)")
        .default_open(false)
        .show(ui, |ui| {
            ui.label("WASD — move, E/Q — up/down");
            ui.label("Shift — faster, Ctrl — slower");
            ui.add(egui::Slider::new(&mut editor.fly_speed, 1.0..=100.0).text("Fly speed"));
            ui.add(egui::Slider::new(&mut editor.fly_sensitivity, 0.0005..=0.01).text("Fly sensitivity"));
        });
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

    let positions: Vec<Vec3> = with_tf.iter().map(|(_, t)| t.position).collect();
    let pos_common = common_vec3(positions.iter().copied());
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

    let scales: Vec<Vec3> = with_tf.iter().map(|(_, t)| t.scale).collect();
    let scale_common = common_vec3(scales.iter().copied());
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

fn common_vec3<I: Iterator<Item = Vec3>>(mut it: I) -> Option<Vec3> {
    let first = it.next()?;
    for v in it {
        if (v - first).length() > 1e-4 {
            return None;
        }
    }
    Some(first)
}

// ============================================================
// Prefabs / Assets (переедет в panels/content.rs на Неделе 2 ч.2)
// ============================================================

pub fn draw_prefabs_section(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Prefabs");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("📁");
        ui.add(
            egui::TextEdit::singleline(&mut editor.prefabs_dir)
                .desired_width(140.0)
                .hint_text("prefabs"),
        );
        if ui.small_button("⟳").clicked() {
            *action = Some(EditorAction::RefreshPrefabs);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.add(
            egui::TextEdit::singleline(&mut editor.prefab_save_name)
                .desired_width(120.0)
                .hint_text("my_prefab"),
        );
        let can_save =
            !editor.selected.is_empty() && !editor.prefab_save_name.trim().is_empty();
        if ui
            .add_enabled(can_save, egui::Button::new("💾 Save Sel"))
            .clicked()
        {
            *action = Some(EditorAction::SavePrefab);
        }
    });
    ui.separator();
    let prefab_count = editor.prefab_list.len();
    if prefab_count == 0 {
        ui.label(egui::RichText::new("no prefabs found").weak().italics());
    } else {
        ui.label(format!("{} files", prefab_count));
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .max_height(200.0)
            .show(ui, |ui| {
                for (idx, path) in editor.prefab_list.iter().enumerate() {
                    let display = crate::scene::prefab::prefab_display_name(path);
                    let resp = ui.button(format!("📦 {}", display));
                    if resp.clicked() {
                        *action = Some(EditorAction::InstantiatePrefab(idx as u32));
                    }
                }
            });
    }
}

pub fn draw_assets_section(
    ui: &mut egui::Ui,
    assets: &UiAssets<'_>,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Assets");
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        if ui.button("➕ Load sRGB (color)…").clicked() {
            *action = Some(EditorAction::LoadTextures);
        }
        if ui.button("➕ Load Linear (data)…").clicked() {
            *action = Some(EditorAction::LoadTexturesLinear);
        }
    });

    ui.separator();
    let tex_count = assets.texture_list.len();
    if tex_count == 0 {
        ui.label(egui::RichText::new("no textures loaded").weak().italics());
        return;
    }
    ui.label(format!("{} textures", tex_count));

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(180.0)
        .show(ui, |ui| {
            for (name, _w, _h, is_srgb) in assets.texture_list {
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
                    .inner;
                resp.context_menu(|ui| {
                    if ui.button("Remove texture").clicked() {
                        *action = Some(EditorAction::RemoveTexture(name.clone()));
                        ui.close();
                    }
                });
            }
        });
}

// ============================================================
// Command palette (переедет в panels/command_palette.rs)
// ============================================================

#[derive(Clone)]
struct PaletteCommand {
    label: &'static str,
    action: EditorAction,
}

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
        PaletteCommand { label: "Terrain · Re-generate splat", action: EditorAction::TerrainRegenerate },
        PaletteCommand { label: "Terrain · Save R16", action: EditorAction::TerrainSaveFile },
        PaletteCommand { label: "Terrain · Export PNG", action: EditorAction::TerrainSavePNG },
        PaletteCommand { label: "Terrain · Load (PNG/R16)", action: EditorAction::TerrainLoadFile },
    ]
}

fn fuzzy_match(query: &str, label: &str) -> Option<usize> {
    if query.is_empty() {
        return Some(0);
    }
    let q = query.to_lowercase();
    let l = label.to_lowercase();
    l.find(&q)
}

pub fn draw_command_palette(
    ctx: &egui::Context,
    state: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    let mut close = false;
    egui::Area::new(egui::Id::new("command_palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            ctx.screen_rect().center().x - 250.0,
            ctx.screen_rect().top() + 100.0,
        ))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(500.0);
                    ui.set_max_width(500.0);
                    ui.label(egui::RichText::new("Command palette").strong().small());
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.command_palette_query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Type to filter commands…"),
                    );
                    response.request_focus();

                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));

                    let commands = palette_commands();
                    let query = state.command_palette_query.clone();
                    let mut filtered: Vec<(usize, &PaletteCommand)> = commands
                        .iter()
                        .filter_map(|c| {
                            fuzzy_match(&query, c.label).map(|pos| (pos, c))
                        })
                        .collect();
                    filtered.sort_by(|a, b| {
                        a.0.cmp(&b.0).then_with(|| a.1.label.cmp(b.1.label))
                    });

                    if !filtered.is_empty() {
                        if up {
                            state.command_palette_selected =
                                state.command_palette_selected.saturating_sub(1);
                        }
                        if down {
                            state.command_palette_selected =
                                (state.command_palette_selected + 1)
                                    .min(filtered.len() - 1);
                        }
                        if state.command_palette_selected >= filtered.len() {
                            state.command_palette_selected = filtered.len() - 1;
                        }
                    } else {
                        state.command_palette_selected = 0;
                    }

                    if enter && !filtered.is_empty() {
                        let cmd = filtered[state.command_palette_selected].1.clone();
                        *action = Some(cmd.action);
                        close = true;
                    }
                    if esc {
                        close = true;
                    }

                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .auto_shrink([false; 2])
                        .show(ui, |ui| {
                            if filtered.is_empty() {
                                ui.label(
                                    egui::RichText::new("no matching commands")
                                        .weak()
                                        .italics(),
                                );
                            }
                            for (idx, (_, cmd)) in filtered.iter().enumerate() {
                                let selected = idx == state.command_palette_selected;
                                let resp = ui.selectable_label(selected, cmd.label);
                                if resp.clicked() {
                                    *action = Some(cmd.action.clone());
                                    close = true;
                                }
                                if resp.hovered() {
                                    state.command_palette_selected = idx;
                                }
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
// Helpers — texture_picker / debug_button
// ============================================================

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
        egui::ComboBox::from_id_salt(id)
            .selected_text(display)
            .width(170.0)
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "(none)").clicked() {
                    *current = None;
                    changed = true;
                }
                if textures.is_empty() {
                    ui.label(
                        egui::RichText::new("(no textures loaded)")
                            .weak()
                            .italics(),
                    );
                }
                for (name, _w, _h, is_srgb) in textures {
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
                        if ui.selectable_label(selected, name).clicked() {
                            *current = Some(name.clone());
                            changed = true;
                        }
                    });
                }
            });
    });
    changed
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

// ============================================================
// `would_create_cycle` — нужен inspector_hierarchy, поэтому живёт здесь
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