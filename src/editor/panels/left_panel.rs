//! Левая панель: Stats + Outliner + Prefabs + Assets.

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

    let panel_response = egui::containers::panel::Panel::left("left_panel")
        .default_size(state.layout.left_panel_width)
        .resizable(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
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