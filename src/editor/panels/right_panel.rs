use crate::ecs::World;
use crate::editor::panels::{details, renderer, UiAssets};
use crate::editor::{EditorAction, EditorState};
use crate::render::PostFx;

pub fn draw(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    world: &mut World,
    assets: &UiAssets<'_>,
    postfx: &mut PostFx,
    action: &mut Option<EditorAction>,
) {
    if !(state.layout.show_inspector_panel || state.layout.show_renderer_panel) {
        return;
    }

    let panel_response = egui::Panel::right("right_panel")
        .default_size(state.layout.right_panel_width)
        .resizable(true)
        .show(ui, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.heading("Inspector");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("☰").clicked() {
                        // Контекстное меню
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .id_salt("right_panel_scroll")
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if state.layout.show_inspector_panel {
                        details::draw(ui, state, world, assets, action);
                    }
                    if state.layout.show_renderer_panel {
                        ui.separator();
                        egui::CollapsingHeader::new("Renderer")
                            .default_open(false)
                            .show(ui, |ui| {
                                renderer::draw(ui, postfx, state, action);
                            });
                    }
                });
        });
    state.layout.right_panel_width = panel_response.response.rect.width();
}