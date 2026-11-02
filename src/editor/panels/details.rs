//! Inspector (Details panel).
//!
//! ## Что нового (Шаг 3a)
//!
//! * **Строка поиска** `details_filter` — фильтрует секции по имени
//!   группы/компонента («phys» → Physics, «coll» → Collider, …).
//! * **Группировка** — компоненты разложены по секциям:
//!   * Transform
//!   * Rendering (Mesh, Material, Tint, TextureTiling, Decal)
//!   * Physics (RigidBody, Collider, PhysicsMaterial)
//!   * Gameplay (Health, Trigger, Interactable, Elevator, SlidingDoor,
//!     Spinner, Velocity, Chase)
//!   * Lights (DirectionalLight, PointLight)
//!   * Audio (AudioSource)
//!   * AI (AiAgent, PatrolPath, Enemy, AiTarget)
//!   * Hierarchy
//!   * Material `<name>` (секция редактирования материала)
//!
//! Плоский список из прошлой версии был неудобен при большом числе
//! компонентов — теперь можно свернуть ненужные группы.

use crate::ecs::{Entity, World};
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
use crate::render::{AlphaMode, Material};
use glam::Vec3;

// ============================================================
// Точка входа
// ============================================================

pub fn draw(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    world: &mut World,
    assets: &UiAssets<'_>,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Inspector");
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
    draw_single(ui, world, e, editor, assets, action);
}

fn draw_single(
    ui: &mut egui::Ui,
    world: &mut World,
    e: Entity,
    editor: &mut EditorState,
    assets: &UiAssets<'_>,
    action: &mut Option<EditorAction>,
) {
    // --- Header: имя + quick actions ---
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

    // --- Filter ---
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("🔍");
        ui.add(
            egui::TextEdit::singleline(&mut editor.details_filter)
                .desired_width(140.0)
                .hint_text("component name…"),
        );
        if !editor.details_filter.is_empty() && ui.small_button("✖").clicked() {
            editor.details_filter.clear();
        }
    });
    ui.separator();

    let filter = editor.details_filter.to_lowercase();
    let matches = |group: &str| -> bool {
        filter.is_empty() || group.to_lowercase().contains(&filter)
    };

    // === Transform ===
    if world.has::<Transform>(e) && matches("transform") {
        inspector_transform(ui, world, e, editor);
    }

    // === Rendering ===
    {
        let has = world.has::<MeshHandle>(e)
            || world.has::<MaterialHandle>(e)
            || world.has::<Tint>(e)
            || world.has::<TextureTiling>(e)
            || world.has::<crate::game::decals::Decal>(e);
        if has
            && (matches("rendering")
                || matches("mesh")
                || matches("material")
                || matches("tint")
                || matches("decal")
                || matches("texture"))
        {
            egui::CollapsingHeader::new("Rendering")
                .default_open(true)
                .show(ui, |ui| {
                    if world.has::<MeshHandle>(e) || world.has::<MaterialHandle>(e) {
                        inspector_geometry(ui, world, e, assets);
                    }
                    if world.has::<Tint>(e) {
                        inspector_tint(ui, world, e, editor);
                    }
                    if world.has::<TextureTiling>(e) {
                        inspector_texture_tiling(ui, world, e, editor);
                    }
                    if world.has::<crate::game::decals::Decal>(e) {
                        inspector_decal(ui, world, e, editor);
                    }
                });
        }
    }

    // === Physics ===
    {
        let has = world.has::<RigidBody>(e)
            || world.has::<Collider>(e)
            || world.has::<PhysicsMaterial>(e);
        if has
            && (matches("physics")
                || matches("rigid")
                || matches("collider")
                || matches("physicsmaterial"))
        {
            egui::CollapsingHeader::new("Physics")
                .default_open(true)
                .show(ui, |ui| {
                    if world.has::<RigidBody>(e) {
                        inspector_rigidbody(ui, world, e, editor);
                    }
                    if world.has::<Collider>(e) {
                        inspector_collider(ui, world, e, editor);
                    }
                    if world.has::<PhysicsMaterial>(e) {
                        inspector_physics_material(ui, world, e, editor);
                    }
                });
        }
    }

    // === Gameplay ===
    {
        let has = world.has::<Health>(e)
            || world.has::<Trigger>(e)
            || world.has::<Interactable>(e)
            || world.has::<Spinner>(e)
            || world.has::<Velocity>(e)
            || world.has::<Chase>(e)
            || world.has::<Elevator>(e)
            || world.has::<SlidingDoor>(e);
        if has
            && (matches("gameplay")
                || matches("health")
                || matches("trigger")
                || matches("interactable")
                || matches("spinner")
                || matches("velocity")
                || matches("chase")
                || matches("elevator")
                || matches("door"))
        {
            egui::CollapsingHeader::new("Gameplay")
                .default_open(true)
                .show(ui, |ui| {
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
                    if world.has::<Elevator>(e) {
                        inspector_elevator(ui, world, e, editor);
                    }
                    if world.has::<SlidingDoor>(e) {
                        inspector_sliding_door(ui, world, e, editor);
                    }
                });
        }
    }

    // === Lights ===
    {
        let has = world.has::<DirectionalLight>(e) || world.has::<PointLight>(e);
        if has && (matches("light") || matches("sun") || matches("point")) {
            egui::CollapsingHeader::new("Lights")
                .default_open(true)
                .show(ui, |ui| {
                    if world.has::<DirectionalLight>(e) {
                        inspector_directional_light(ui, world, e, editor);
                    }
                    if world.has::<PointLight>(e) {
                        inspector_point_light(ui, world, e, editor);
                    }
                });
        }
    }

    // === Audio ===
    if world.has::<AudioSource>(e) && matches("audio") {
        egui::CollapsingHeader::new("Audio")
            .default_open(true)
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("🔊 AudioSource — см. окно")
                        .small()
                        .weak()
                        .italics(),
                );
            });
    }

    // === AI ===
    {
        let has = world.has::<crate::game::ai::AiAgent>(e)
            || world.has::<crate::game::ai::PatrolPath>(e)
            || world.has::<crate::game::ai::Enemy>(e)
            || world.has::<crate::game::ai::AiTarget>(e);
        if has && (matches("ai") || matches("agent") || matches("enemy") || matches("patrol"))
        {
            egui::CollapsingHeader::new("AI")
                .default_open(true)
                .show(ui, |ui| {
                    if world.has::<crate::game::ai::AiAgent>(e) {
                        ui.label(
                            egui::RichText::new(
                                "AiAgent (секция в разработке)",
                            )
                            .small()
                            .weak()
                            .italics(),
                        );
                    }
                    if world.has::<crate::game::ai::PatrolPath>(e) {
                        ui.label(
                            egui::RichText::new(
                                "PatrolPath (секция в разработке)",
                            )
                            .small()
                            .weak()
                            .italics(),
                        );
                    }
                    if world.has::<crate::game::ai::Enemy>(e) {
                        ui.label("👹 Enemy marker");
                    }
                    if world.has::<crate::game::ai::AiTarget>(e) {
                        ui.label("🎯 AI Target");
                    }
                });
        }
    }

    // === Hierarchy ===
    if (world.has::<Parent>(e) || editor.selected.len() >= 2) && matches("hierarchy") {
        inspector_hierarchy(ui, world, e, editor);
    }

    // === Material (секция редактирования) ===
    if let Some((name, original)) = &assets.selected_material {
        if matches("material") {
            inspector_material(ui, world, e, editor, action, assets, name, original);
        }
    }

    ui.separator();
    if ui.button("Deselect").clicked() {
        editor.selected.clear();
    }
}

// ============================================================
// Lights / Decal
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
// Geometry / texture tiling
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
// ComponentKind / remove / add
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
// Multi-edit
// ============================================================

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

// ============================================================
// Helpers
// ============================================================

fn common_vec3<I: Iterator<Item = Vec3>>(mut it: I) -> Option<Vec3> {
    let first = it.next()?;
    for v in it {
        if (v - first).length() > 1e-4 {
            return None;
        }
    }
    Some(first)
}

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