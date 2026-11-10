use crate::ecs::World;
use crate::editor::panels::{content, outliner, stats, Stats, UiAssets};
use crate::editor::{EditorAction, EditorState};

pub fn draw(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    world: &mut World,
    assets: &UiAssets<'_>,
    stats: &Stats,
    action: &mut Option<EditorAction>,
) {
    if !(state.layout.show_stats_panel || state.layout.show_hierarchy_panel) {
        return;
    }

    let panel_response = egui::Panel::left("left_panel")
        .default_size(state.layout.left_panel_width)
        .resizable(true)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Hierarchy");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⚙").clicked() {
                        // Меню настроек
                    }
                });
            });
            ui.separator();

            // Поиск
            ui.horizontal(|ui| {
                ui.label("🔍");
                ui.add(
                    egui::TextEdit::singleline(&mut state.search_filter)
                        .desired_width(f32::INFINITY)
                        .hint_text("Search..."),
                );
            });
            ui.add_space(4.0);

            egui::ScrollArea::vertical()
                .id_salt("left_panel_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    // === БЛОК СЕТКИ И ПРИВЯЗКИ ===
                    ui.heading("Grid & Snap");
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.snap_enabled, "Snap to Grid");
                    });
                    ui.horizontal(|ui| {
                        ui.label("Grid Size:");
                        egui::ComboBox::from_id_salt("grid_size_combo")
                            .selected_text(format!("{:.2} m", state.grid_size))
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut state.grid_size, 0.1, "0.1 m (10 cm)");
                                ui.selectable_value(&mut state.grid_size, 0.5, "0.5 m");
                                ui.selectable_value(&mut state.grid_size, 1.0, "1 m");
                                ui.selectable_value(&mut state.grid_size, 10.0, "10 m");
                                ui.selectable_value(&mut state.grid_size, 100.0, "100 m");
                            });
                    });
                    ui.separator();

                    if state.layout.show_stats_panel {
                        stats::draw_section(ui, stats);
                    }
                    if state.layout.show_hierarchy_panel {
                        ui.separator();
                        outliner::draw_section(ui, state, world, action);
                        ui.separator();
                        content::draw_prefabs_section(ui, state, action);
                        ui.separator();
                        content::draw_assets_section(ui, assets, action);
                    }
                });
        });
    state.layout.left_panel_width = panel_response.response.rect.width();
}