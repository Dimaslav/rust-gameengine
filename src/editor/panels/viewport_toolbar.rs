//! Floating viewport toolbar — T / R / S / Snap в верхнем левом углу viewport'а.

use crate::editor::gizmo::GizmoMode;
use crate::editor::EditorState;

pub fn draw(ctx: &egui::Context, viewport_rect: egui::Rect, state: &mut EditorState) {
    if state.play.active { return; }

    // FIX (P3.1): тулбар привязан к viewport'у (CentralPanel), а не
    // к content_rect() (= весь экран). Передаётся из `draw_all`.
    let pos = viewport_rect.min + egui::vec2(12.0, 12.0);

    egui::Area::new(egui::Id::unique("viewport_toolbar"))
        .fixed_pos(pos)
        .order(egui::Order::Middle)
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .fill(egui::Color32::from_rgba_unmultiplied(20, 22, 28, 220))
                .stroke(egui::Stroke::new(1.0,
                    egui::Color32::from_rgba_unmultiplied(80, 90, 110, 200)))
                .inner_margin(egui::Margin::symmetric(6, 4))
                .corner_radius(egui::CornerRadius::same(6))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        let m = &mut state.gizmo.mode;
                        if ui.selectable_label(*m == GizmoMode::Translate, "T")
                            .on_hover_text("Translate (1)").clicked()
                        { *m = GizmoMode::Translate; }
                        if ui.selectable_label(*m == GizmoMode::Rotate, "R")
                            .on_hover_text("Rotate (2)").clicked()
                        { *m = GizmoMode::Rotate; }
                        if ui.selectable_label(*m == GizmoMode::Scale, "S")
                            .on_hover_text("Scale (3)").clicked()
                        { *m = GizmoMode::Scale; }

                        ui.add_space(4.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, egui::CornerRadius::ZERO,
                            egui::Color32::from_rgba_unmultiplied(80, 90, 110, 150));
                        ui.add_space(4.0);

                        ui.toggle_value(&mut state.gizmo.snap_enabled, "Snap")
                            .on_hover_text("Snap без Ctrl (0.5 м / 15°)");

                        ui.add_space(4.0);
                        let (rect, _) = ui.allocate_exact_size(egui::vec2(1.0, 20.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, egui::CornerRadius::ZERO,
                            egui::Color32::from_rgba_unmultiplied(80, 90, 110, 150));
                        ui.add_space(4.0);

                        let has_sel = !state.selected.is_empty();
                        if ui.add_enabled(has_sel, egui::Button::new("F").frame(false))
                            .on_hover_text("Focus selected (F)").clicked()
                        {
                            state.pending_action = Some(crate::editor::EditorAction::FocusSelected);
                        }
                    });
                });
        });
}