use super::MenuActions;
use crate::app_state::{
    AppSettings, AudioSettings, GraphicsSettings, QualityPreset, WindowSettings,
    RESOLUTIONS,
};

pub fn draw(ctx: &egui::Context, settings: &mut AppSettings, actions: &mut MenuActions) {
    let screen = ctx.screen_rect();
    let center = screen.center();

    egui::Area::new(egui::Id::new("settings_menu"))
        .fixed_pos(egui::pos2(center.x - 300.0, center.y - 280.0))
        .show(ctx, |ui| {
            ui.set_width(600.0);

            ui.vertical_centered(|ui| {
                ui.add_space(16.0);
                ui.label(
                    egui::RichText::new("SETTINGS")
                        .size(32.0)
                        .strong()
                        .color(egui::Color32::from_rgb(200, 210, 230)),
                );
                ui.add_space(24.0);
            });

            egui::ScrollArea::vertical()
                .max_height(460.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    egui::CollapsingHeader::new("🖥   Window")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            draw_window(ui, &mut settings.window);
                            ui.add_space(4.0);
                        });
                    ui.add_space(6.0);

                    egui::CollapsingHeader::new("🎨   Graphics")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            draw_graphics(ui, &mut settings.graphics);
                            ui.add_space(4.0);
                        });
                    ui.add_space(6.0);

                    egui::CollapsingHeader::new("🔊   Audio")
                        .default_open(true)
                        .show(ui, |ui| {
                            ui.add_space(4.0);
                            draw_audio(ui, &mut settings.audio);
                            ui.add_space(4.0);
                        });
                });

            ui.add_space(20.0);
            ui.horizontal(|ui| {
                if ui.add_sized([160.0, 38.0], egui::Button::new("←  Back")).clicked() {
                    actions.back = true;
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_sized([160.0, 38.0], egui::Button::new("✓  Apply")).clicked() {
                        actions.apply_settings = true;
                    }
                });
            });
        });
}

fn draw_window(ui: &mut egui::Ui, s: &mut WindowSettings) {
    ui.checkbox(&mut s.vsync, "V-Sync")
        .on_hover_text("Синхронизация с частотой монитора.");
    ui.checkbox(&mut s.fullscreen, "Fullscreen");

    ui.horizontal(|ui| {
        ui.label("Resolution:");
        let current = RESOLUTIONS.get(s.resolution_index).map(|r| r.2).unwrap_or("?");
        egui::ComboBox::from_id_salt("res_combo")
            .selected_text(current)
            .width(220.0)
            .show_ui(ui, |ui| {
                for (i, (_, _, label)) in RESOLUTIONS.iter().enumerate() {
                    ui.selectable_value(&mut s.resolution_index, i, *label);
                }
            });
    });
}

fn draw_graphics(ui: &mut egui::Ui, s: &mut GraphicsSettings) {
    ui.horizontal(|ui| {
        ui.label("Quality preset:");
        for preset in QualityPreset::all() {
            if ui
                .selectable_label(s.quality_preset == preset, preset.label())
                .on_hover_text(preset.description())
                .clicked()
            {
                s.quality_preset = preset;
            }
        }
    });
    ui.label(
        egui::RichText::new(s.quality_preset.description())
            .small()
            .weak()
            .italics(),
    );

    ui.add(egui::Slider::new(&mut s.render_scale, 0.5..=1.0).text("Render scale"));
    ui.add(egui::Slider::new(&mut s.fov, 60.0..=110.0).text("FOV (deg)"));
    ui.checkbox(&mut s.show_fps, "Show FPS counter");
}

fn draw_audio(ui: &mut egui::Ui, s: &mut AudioSettings) {
    ui.add(egui::Slider::new(&mut s.master, 0.0..=1.0).text("Master"));
    ui.add(egui::Slider::new(&mut s.sfx, 0.0..=1.0).text("SFX"));
    ui.add(egui::Slider::new(&mut s.music, 0.0..=1.0).text("Music"));
    ui.add(egui::Slider::new(&mut s.voice, 0.0..=1.0).text("Voice"));
    ui.add(egui::Slider::new(&mut s.ui, 0.0..=1.0).text("UI"));
}