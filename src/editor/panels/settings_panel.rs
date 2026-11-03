use crate::editor::hotkeys::HotkeyAction;
use crate::editor::panels::Stats;
use crate::editor::theme::Theme;
use crate::editor::EditorState;

pub fn draw_window(
    ctx: &egui::Context,
    state: &mut EditorState,
    _stats: &Stats,
) {
    if !state.settings_open { return; }

    let mut open = state.settings_open;

    egui::Window::new("⚙ Settings")
        .open(&mut open)
        .default_pos(egui::pos2(360.0, 180.0))
        .default_width(520.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.label(egui::RichText::new("Theme").strong());
            ui.separator();

            let mut new_theme: Option<Theme> = None;
            for theme in Theme::ALL {
                let selected = state.theme == *theme;
                if ui.selectable_label(selected, theme.label())
                    .on_hover_text(theme.tooltip())
                    .clicked() && !selected
                {
                    new_theme = Some(*theme);
                }
            }
            if let Some(t) = new_theme {
                state.theme = t;
                state.apply_theme(ctx);
            }

            ui.separator();
            ui.label(egui::RichText::new("Theme applies immediately and is saved to editor.ron.")
                .small().weak().italics());

            ui.add_space(12.0);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Hotkeys").strong());
                if ui.small_button("↺ Reset all")
                    .on_hover_text("Reset every binding to its default").clicked()
                {
                    state.hotkeys.reset_all();
                    state.hotkey_capture = None;
                }
            });
            ui.separator();

            let mut current_cat = "";
            egui::ScrollArea::vertical().max_height(360.0).auto_shrink([false; 2])
                .show(ui, |ui| {
                    for action in HotkeyAction::ALL {
                        let cat = action.category();
                        if cat != current_cat {
                            if !current_cat.is_empty() { ui.add_space(6.0); }
                            current_cat = cat;
                            ui.label(egui::RichText::new(cat).small().strong()
                                .color(ui.visuals().weak_text_color()));
                        }
                        draw_hotkey_row(ui, state, *action);
                    }
                });

            ui.add_space(8.0);
            ui.separator();
            ui.label(egui::RichText::new(
                "Click a binding to rebind. Esc during capture cancels."
            ).small().weak().italics());
        });

    state.settings_open = open;

    if let Some(action) = state.hotkey_capture {
        draw_capture_overlay(ctx, state, action);
    }
}

fn draw_hotkey_row(ui: &mut egui::Ui, state: &mut EditorState, action: HotkeyAction) {
    let binding = state.hotkeys.get(action);
    let bookmark_suffix = action.bookmark_slot().map(|i| format!(" {}", i + 1)).unwrap_or_default();

    ui.horizontal(|ui| {
        let label = format!("{}{}", action.label(), bookmark_suffix);
        ui.label(label);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.small_button("↺").on_hover_text("Reset to default").clicked() {
                state.hotkeys.reset(action);
            }
            if ui.add_enabled(binding.is_some(), egui::Button::new("✖").small())
                .on_hover_text("Unbind").clicked()
            {
                state.hotkeys.remove(action);
            }
            let label = binding.map(|b| b.display()).unwrap_or_else(|| "(unbound)".to_string());
            if ui.button(label).on_hover_text("Click to rebind").clicked() {
                state.hotkey_capture = Some(action);
            }
        });
    });
}

fn draw_capture_overlay(
    ctx: &egui::Context,
    state: &mut EditorState,
    action: HotkeyAction,
) {
    // ИЗМЕНЕНО (egui 0.37): content_rect() вместо screen_rect
    let screen = ctx.input(|i| i.content_rect());
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::unique("hotkey_capture_dim"),
    ));
    painter.rect_filled(screen, egui::CornerRadius::ZERO,
        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 140));

    egui::Area::new(egui::Id::unique("hotkey_capture"))
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .order(egui::Order::Foreground)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::symmetric(32, 24))
                .show(ui, |ui| {
                    ui.vertical_centered(|ui| {
                        ui.label(egui::RichText::new(format!(
                            "Press a key for: {}", action.label()
                        )).heading().color(egui::Color32::from_rgb(230, 200, 130)));
                        ui.add_space(8.0);
                        ui.label(egui::RichText::new("Esc — cancel").small().weak());
                        ui.add_space(16.0);
                        if ui.button("Cancel").clicked() {
                            state.hotkey_capture = None;
                        }
                    });
                });
        });
}