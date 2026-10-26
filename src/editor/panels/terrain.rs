//! Панель Terrain (верхняя полоса).
//!
//! В Шаге 1 — `TopBottomPanel::top`, как было. В Шаге 3 переделаем в
//! floating-toolbar поверх viewport'а.

use crate::editor::terrain_tool::TerrainBrush;
use crate::editor::{EditorAction, EditorState};
use crate::render::terrain::Falloff;

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    egui::TopBottomPanel::top("terrain_bar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("🏔 Terrain").strong());

            let tool = &mut state.terrain;

            ui.toggle_value(&mut tool.active, "Enable")
                .on_hover_text("LMB по вьюпорту — применить кисть");

            if !tool.active {
                ui.label(egui::RichText::new("(brush disabled)").weak().italics());
                return;
            }

            ui.separator();
            ui.label("Brush:");
            for brush in [
                TerrainBrush::Raise,
                TerrainBrush::Lower,
                TerrainBrush::Smooth,
                TerrainBrush::Flatten,
            ] {
                if ui.selectable_label(tool.brush == brush, brush.label()).clicked() {
                    tool.brush = brush;
                }
            }

            ui.separator();
            ui.label("Paint:");
            for i in 0..4u8 {
                let b = TerrainBrush::Paint(i);
                if ui.selectable_label(tool.brush == b, b.label()).clicked() {
                    tool.brush = b;
                }
            }

            ui.separator();
            ui.add(egui::Slider::new(&mut tool.radius, 0.5..=200.0)
                .logarithmic(true)
                .text("Radius (m)"));
            ui.add(egui::Slider::new(&mut tool.strength, 0.01..=1.0).text("Strength"));

            ui.label("Falloff:");
            let mut idx = match tool.falloff {
                Falloff::Constant => 0u8,
                Falloff::Linear => 1,
                Falloff::Smooth => 2,
            };
            ui.selectable_value(&mut idx, 0, "Const");
            ui.selectable_value(&mut idx, 1, "Linear");
            ui.selectable_value(&mut idx, 2, "Smooth");
            tool.falloff = match idx {
                0 => Falloff::Constant,
                1 => Falloff::Linear,
                _ => Falloff::Smooth,
            };

            if matches!(tool.brush, TerrainBrush::Flatten) {
                ui.separator();
                ui.add(
                    egui::Slider::new(&mut tool.flatten_target, -50.0..=200.0)
                        .text("Target Y"),
                );
                if ui.small_button("Use cursor Y").clicked() {
                    if let Some(p) = tool.cursor_world {
                        tool.flatten_target = p.y;
                    }
                }
            }

            ui.separator();
            if ui.button("⟳ Re-generate splat").clicked() {
                *action = Some(EditorAction::TerrainRegenerate);
            }
            if ui.button("💾 Save R16").clicked() {
                *action = Some(EditorAction::TerrainSaveFile);
            }
            if ui.button("🖼 Export PNG").clicked() {
                *action = Some(EditorAction::TerrainSavePNG);
            }
            if ui.button("📂 Load (PNG/R16)").clicked() {
                *action = Some(EditorAction::TerrainLoadFile);
            }

            ui.separator();
            if let Some(p) = tool.cursor_world {
                ui.label(
                    egui::RichText::new(format!(
                        "cursor ({:.1}, {:.1}, {:.1})",
                        p.x, p.y, p.z
                    ))
                    .small()
                    .weak()
                    .monospace(),
                );
            } else {
                ui.label(
                    egui::RichText::new("cursor —")
                        .small()
                        .weak()
                        .monospace(),
                );
            }
        });
    });
}