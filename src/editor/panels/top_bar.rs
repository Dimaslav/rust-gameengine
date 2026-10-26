//! Верхняя полоса редактора: Play / Undo / Redo / Save / Load /
//! FBX / панели-toggle.

use crate::editor::gizmo::GizmoMode;
use crate::editor::panels::Stats;
use crate::editor::{EditorAction, EditorState};

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    stats: &Stats,
    action: &mut Option<EditorAction>,
) {
    egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.heading("Rust Engine 3D");
            if ui
                .button("⌘")
                .on_hover_text("Command palette (Ctrl+P)")
                .clicked()
            {
                state.command_palette_open = true;
                state.command_palette_query.clear();
                state.command_palette_selected = 0;
            }
            ui.separator();
            if ui
                .add_enabled(state.undo.can_undo(), egui::Button::new("↶ Undo"))
                .on_hover_text("Ctrl+Z")
                .clicked()
            {
                *action = Some(EditorAction::Undo);
            }
            if ui
                .add_enabled(state.undo.can_redo(), egui::Button::new("↷ Redo"))
                .on_hover_text("Ctrl+Y")
                .clicked()
            {
                *action = Some(EditorAction::Redo);
            }
            ui.separator();

            if state.play.active {
                if state.play.paused {
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
            } else if ui
                .button("▶ Play")
                .on_hover_text("Run game (F9)")
                .clicked()
            {
                *action = Some(EditorAction::TogglePlay);
            }
            ui.separator();

            if !state.play.active {
                let m = &mut state.gizmo.mode;
                if ui
                    .selectable_label(*m == GizmoMode::Translate, "T")
                    .on_hover_text("Translate (1)")
                    .clicked()
                {
                    *m = GizmoMode::Translate;
                }
                if ui
                    .selectable_label(*m == GizmoMode::Rotate, "R")
                    .on_hover_text("Rotate (2)")
                    .clicked()
                {
                    *m = GizmoMode::Rotate;
                }
                if ui
                    .selectable_label(*m == GizmoMode::Scale, "S")
                    .on_hover_text("Scale (3)")
                    .clicked()
                {
                    *m = GizmoMode::Scale;
                }
                ui.toggle_value(&mut state.gizmo.snap_enabled, "Snap")
                    .on_hover_text("Snap без Ctrl (0.5 м / 15°)");
                ui.separator();
            }

            if ui
                .add_enabled(
                    !state.selected.is_empty(),
                    egui::Button::new("Copy (Ctrl+C)"),
                )
                .clicked()
            {
                *action = Some(EditorAction::CopyEntity);
            }
            if ui
                .add_enabled(
                    !state.clipboard_entities.is_empty(),
                    egui::Button::new("Paste (Ctrl+V)"),
                )
                .clicked()
            {
                *action = Some(EditorAction::PasteEntity);
            }
            ui.separator();

            if ui.button("New").on_hover_text("New empty scene").clicked() {
                *action = Some(EditorAction::NewScene);
            }
            if ui.button("Save").clicked() {
                *action = Some(EditorAction::Save);
            }
            if ui.button("Load").clicked() {
                *action = Some(EditorAction::Load);
            }
            ui.add(
                egui::TextEdit::singleline(&mut state.save_path)
                    .desired_width(140.0)
                    .hint_text("scene.ron"),
            );

            let recents = state.settings.recent_scenes.clone();
            ui.menu_button("⏷", |ui| {
                if recents.is_empty() {
                    ui.label(egui::RichText::new("no recent scenes").weak().italics());
                } else {
                    for p in &recents {
                        if ui.button(p).clicked() {
                            *action = Some(EditorAction::LoadPath(p.clone()));
                            ui.close();
                        }
                    }
                    ui.separator();
                    if ui.button("Clear list").clicked() {
                        state.settings.recent_scenes.clear();
                        ui.close();
                    }
                }
            })
            .response
            .on_hover_text("Recent scenes");

            ui.separator();
            if ui
                .button("FBX All")
                .on_hover_text("Экспорт всей сцены в .fbx")
                .clicked()
            {
                *action = Some(EditorAction::ExportFbxAll);
            }
            if ui
                .add_enabled(
                    !state.selected.is_empty(),
                    egui::Button::new("FBX Sel"),
                )
                .on_hover_text("Экспорт выделения в .fbx")
                .clicked()
            {
                *action = Some(EditorAction::ExportFbxSelected);
            }
            if ui
                .button("FBX Import")
                .on_hover_text("Импорт ASCII или binary FBX")
                .clicked()
            {
                *action = Some(EditorAction::ImportFbx);
            }
            ui.separator();
            ui.label(format!("FPS: {:.1}", stats.fps));
            ui.separator();
            ui.label(format!("Entities: {}", stats.entities));

            if state.play.active {
                ui.separator();
                if state.play.paused {
                    ui.label(
                        egui::RichText::new("⏸ PAUSED")
                            .strong()
                            .color(egui::Color32::from_rgb(230, 200, 130)),
                    );
                } else {
                    ui.label(
                        egui::RichText::new("● PLAY")
                            .strong()
                            .color(egui::Color32::from_rgb(230, 90, 90)),
                    );
                }
            } else if state.flying {
                ui.separator();
                ui.label(
                    egui::RichText::new("✦ FLY")
                        .strong()
                        .color(egui::Color32::from_rgb(90, 180, 230)),
                );
            } else if state.terrain.active {
                ui.separator();
                ui.label(
                    egui::RichText::new("🏔 TERRAIN")
                        .strong()
                        .color(egui::Color32::from_rgb(220, 160, 90)),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.toggle_value(&mut state.layout.show_renderer_panel, "Renderer");
                ui.toggle_value(&mut state.layout.show_inspector_panel, "Inspector");
                ui.toggle_value(&mut state.layout.show_hierarchy_panel, "Hierarchy");
                ui.toggle_value(&mut state.layout.show_stats_panel, "Stats");
                ui.toggle_value(&mut state.layout.show_bookmarks_panel, "📷");
                ui.toggle_value(&mut state.layout.show_audio_panel, "🔊");
                ui.toggle_value(&mut state.layout.show_content_browser, "📁")
                    .on_hover_text("Content Browser");
                ui.toggle_value(&mut state.layout.show_script_panel, "📜")
                    .on_hover_text("Scripts");
            });
        });
    });
}