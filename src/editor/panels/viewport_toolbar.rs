use crate::editor::EditorState;

pub fn draw(ui: &mut egui::Ui, state: &mut EditorState) {
    egui::Frame::NONE
        .fill(egui::Color32::from_rgb(40, 40, 40))
        .inner_margin(egui::Margin::symmetric(8, 4))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                // Вкладки
                ui.selectable_value(&mut state.active_workspace, crate::editor::layout::Workspace::Layout, "Scene");
                ui.selectable_value(&mut state.active_workspace, crate::editor::layout::Workspace::LevelDesign, "Game");
                ui.selectable_value(&mut state.active_workspace, crate::editor::layout::Workspace::Modeling, "Renderer");
                
                ui.separator();
                
                // Shaded / Wireframe
                egui::ComboBox::from_id_salt("shaded_mode")
                    .selected_text("Shaded")
                    .show_ui(ui, |ui| {
                        ui.label("Shaded");
                        ui.label("Wireframe");
                        ui.label("Unlit");
                    });

                // Gizmos
                egui::ComboBox::from_id_salt("gizmos")
                    .selected_text("Gizmos")
                    .show_ui(ui, |ui| {
                        ui.checkbox(&mut state.gizmo.snap_enabled, "Snap to Grid");
                        ui.checkbox(&mut state.layout.show_palette_bar, "Show Palette");
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⛶").clicked() {}
                });
            });
        });
}