//! Панель Palette (верхняя полоса).
//!
//! Выбор примитива / света / decal для click-to-place. В Шаге 3
//! переделаем в floating-toolbar.

use crate::editor::palette::{PaletteItem, PaletteState};
use crate::editor::EditorState;

pub fn draw(ctx: &egui::Context, state: &mut EditorState) {
    egui::TopBottomPanel::top("palette_bar").show(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new("🖌 Brush:").strong());
            for &item in PaletteItem::all_primitives() {
                brush_button(ui, &mut state.palette, item);
            }

            ui.separator();
            ui.label("💡 Lights:");
            for &item in PaletteItem::all_lights() {
                brush_button(ui, &mut state.palette, item);
            }

            ui.separator();
            ui.label("🎨 Decals:");
            for &item in PaletteItem::all_decals() {
                brush_button(ui, &mut state.palette, item);
            }

            ui.separator();
            ui.label("Presets:");
            for &item in PaletteItem::all_presets() {
                brush_button(ui, &mut state.palette, item);
            }

            ui.separator();

            if state.palette.active.is_some() {
                let is_light = state.palette.active.map(|i| i.is_light()).unwrap_or(false);
                if ui
                    .button("✖ Clear (Esc)")
                    .on_hover_text("Снять кисть")
                    .clicked()
                {
                    state.palette.active = None;
                }
                ui.checkbox(&mut state.palette.keep_active, "Keep");
                if !is_light {
                    ui.checkbox(&mut state.palette.snap_to_grid, "Snap");
                    ui.add(
                        egui::DragValue::new(&mut state.palette.grid_step)
                            .speed(0.01)
                            .range(0.05..=10.0)
                            .prefix("step: "),
                    );
                }
            } else {
                ui.label(egui::RichText::new("no brush").weak().italics());
            }
        });
    });
}

fn brush_button(ui: &mut egui::Ui, palette: &mut PaletteState, item: PaletteItem) {
    let selected = palette.active == Some(item);
    if ui
        .selectable_label(selected, item.label())
        .on_hover_text(format!("Click to place {}", item.label()))
        .clicked()
    {
        palette.active = if selected { None } else { Some(item) };
    }
}