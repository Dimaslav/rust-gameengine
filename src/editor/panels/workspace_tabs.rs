use crate::editor::layout::{LayoutState, Workspace};
use crate::editor::EditorState;

pub fn draw_inline(ui: &mut egui::Ui, state: &mut EditorState) {
    for ws in Workspace::ALL {
        let selected = state.active_workspace == *ws;
        let resp = ui
            .selectable_label(selected, ws.label())
            .on_hover_text(ws.tooltip());

        if resp.clicked() {
            state.switch_workspace(*ws);
        }

        // Правый клик — сброс workspace'а к пресету.
        let ws_for_menu = *ws;
        resp.context_menu(|ui| {
            if ui.button("Reset to default").clicked() {
                state.saved_workspaces.remove(&ws_for_menu);
                if state.active_workspace == ws_for_menu {
                    state.layout = LayoutState::preset_for(ws_for_menu);
                }
                ui.close();
            }
        });
    }
    ui.separator();
}