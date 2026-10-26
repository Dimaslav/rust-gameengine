//! Оверлеи: box select, context menu, pause.

use crate::ecs::World;
use crate::editor::{EditorAction, EditorState};

pub fn draw_box_select_overlay(ctx: &egui::Context, editor: &EditorState) {
    let Some(bs) = editor.box_select else { return; };
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
        egui::CornerRadius::ZERO,
        egui::Color32::from_rgba_unmultiplied(90, 180, 255, 32),
    );
    painter.rect_stroke(
        rect,
        egui::CornerRadius::ZERO,
        egui::Stroke::new(
            1.5_f32,
            egui::Color32::from_rgba_unmultiplied(120, 200, 255, 230),
        ),
        egui::StrokeKind::Inside,
    );
}

pub fn draw_pause_overlay(
    ctx: &egui::Context,
    _editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    let screen = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("pause_dim_layer"),
    ));
    painter.rect_filled(
        screen,
        egui::CornerRadius::ZERO,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 160),
    );

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

pub fn draw_context_menu(
    ctx: &egui::Context,
    editor: &mut EditorState,
    _world: &mut World,
    action: &mut Option<EditorAction>,
) {
    let Some(pos) = editor.context_menu_pos else { return; };
    let ppp = ctx.pixels_per_point();
    let p = egui::pos2(pos.0 / ppp, pos.1 / ppp);
    let area = egui::Area::new(egui::Id::new("viewport_ctx_menu"))
        .fixed_pos(p)
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_min_width(200.0);
                let has_sel = !editor.selected.is_empty();
                if ui
                    .add_enabled(has_sel, egui::Button::new("Focus (F)"))
                    .clicked()
                {
                    *action = Some(EditorAction::FocusSelected);
                    editor.context_menu_pos = None;
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("Duplicate (Ctrl+D)"))
                    .clicked()
                {
                    *action = Some(EditorAction::Duplicate);
                    editor.context_menu_pos = None;
                }
                if ui
                    .add_enabled(has_sel, egui::Button::new("Delete"))
                    .clicked()
                {
                    *action = Some(EditorAction::DeleteSelected);
                    editor.context_menu_pos = None;
                }
                ui.separator();
                if ui
                    .add_enabled(has_sel, egui::Button::new("Copy (Ctrl+C)"))
                    .clicked()
                {
                    *action = Some(EditorAction::CopyEntity);
                    editor.context_menu_pos = None;
                }
                if ui
                    .add_enabled(
                        !editor.clipboard_entities.is_empty(),
                        egui::Button::new("Paste (Ctrl+V)"),
                    )
                    .clicked()
                {
                    *action = Some(EditorAction::PasteEntity);
                    editor.context_menu_pos = None;
                }
                ui.separator();
                if ui
                    .add_enabled(has_sel, egui::Button::new("Make Material Unique"))
                    .clicked()
                {
                    *action = Some(EditorAction::MakeMaterialUnique);
                    editor.context_menu_pos = None;
                }
                ui.separator();
                if ui
                    .add_enabled(has_sel, egui::Button::new("Export FBX (selection)"))
                    .clicked()
                {
                    *action = Some(EditorAction::ExportFbxSelected);
                    editor.context_menu_pos = None;
                }
                if ui.button("Import FBX…").clicked() {
                    *action = Some(EditorAction::ImportFbx);
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