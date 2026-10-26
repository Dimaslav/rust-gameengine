//! Правая панель: Inspector + Renderer.

use crate::ecs::World;
use crate::editor::panels::UiAssets;
use crate::editor::{EditorAction, EditorState};
use crate::render::PostFx;

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    world: &mut World,
    assets: &UiAssets<'_>,
    postfx: &mut PostFx,
    action: &mut Option<EditorAction>,
) {
    if !(state.layout.show_inspector_panel || state.layout.show_renderer_panel) {
        return;
    }
    let panel_response = egui::SidePanel::right("right_panel")
        .default_width(state.layout.right_panel_width)
        .resizable(true)
        .show(ctx, |ui| {
            if state.layout.show_inspector_panel {
                // TODO(Неделя 2, часть 2): перенести в panels/details.rs.
                crate::editor::ui::draw_inspector_panel(ui, state, world, assets, action);
            }
            if state.layout.show_renderer_panel {
                ui.separator();
                egui::CollapsingHeader::new("Renderer")
                    .default_open(true)
                    .show(ui, |ui| {
                        // TODO(Неделя 2, часть 2): перенести в panels/renderer.rs.
                        crate::editor::ui::draw_renderer_panel(
                            ui, postfx, state, action,
                        );
                    });
            }
        });
    state.layout.right_panel_width = panel_response.response.rect.width();
}