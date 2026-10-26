//! Плавающее окно Camera Bookmarks.
//!
//! 9 слотов: `Ctrl+1..9` — сохранить, `Alt+1..9` — перейти (хоткеи в
//! `app.rs`). Здесь — UI-список слотв.

use crate::editor::{EditorAction, EditorState};

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    if !state.layout.show_bookmarks_panel {
        return;
    }
    let mut open = state.layout.show_bookmarks_panel;

    egui::Window::new("📷 Camera Bookmarks")
        .open(&mut open)
        .default_pos(egui::pos2(680.0, 120.0))
        .default_width(260.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new("Ctrl+1..9 — save · Alt+1..9 — goto")
                    .small()
                    .weak(),
            );
            ui.separator();
            for i in 0..9 {
                ui.horizontal(|ui| {
                    ui.label(format!("Slot {}:", i + 1));
                    if state.settings.camera_bookmarks.is_empty(i) {
                        ui.label(egui::RichText::new("(empty)").weak().italics());
                    } else if ui.small_button("↺").clicked() {
                        *action = Some(EditorAction::GotoCameraBookmark(i));
                    }
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.small_button("💾").clicked() {
                                *action = Some(EditorAction::SaveCameraBookmark(i));
                            }
                        },
                    );
                });
            }
        });

    state.layout.show_bookmarks_panel = open;
}