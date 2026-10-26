//! Левая панель: Stats + Outliner + Prefabs + Assets.

use crate::ecs::World;
use crate::editor::panels::{outliner, stats, Stats, UiAssets};
use crate::editor::{EditorAction, EditorState};

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    world: &mut World,
    assets: &UiAssets<'_>,
    stats: &Stats,
    action: &mut Option<EditorAction>,
) {
    if !(state.layout.show_stats_panel || state.layout.show_hierarchy_panel) {
        return;
    }
    let panel_response = egui::SidePanel::left("left_panel")
        .default_width(state.layout.left_panel_width)
        .resizable(true)
        .show(ctx, |ui| {
            if state.layout.show_stats_panel {
                stats::draw_section(ui, stats);
            }
            if state.layout.show_hierarchy_panel {
                ui.separator();
                outliner::draw_section(ui, state, world, action);
                ui.separator();
                // TODO(Неделя 2, часть 2): перенести в panels/content.rs.
                crate::editor::ui::draw_prefabs_section(ui, state, action);
                ui.separator();
                crate::editor::ui::draw_assets_section(ui, assets, action);
            }
        });
    state.layout.left_panel_width = panel_response.response.rect.width();
}