use crate::editor::panels::AudioSnapshot;
use crate::editor::{EditorAction, EditorState};

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    audio: &AudioSnapshot,
    action: &mut Option<EditorAction>,
) {
    if !state.layout.show_audio_panel {
        return;
    }
    let mut open = state.layout.show_audio_panel;

    egui::Window::new("🔊 Audio")
        .open(&mut open)
        .default_pos(egui::pos2(320.0, 380.0))
        .default_width(320.0)
        .resizable(true)
        .show(ctx, |ui| {
            if !audio.available {
                ui.label(
                    egui::RichText::new("audio unavailable")
                        .italics()
                        .color(egui::Color32::from_rgb(230, 130, 130)),
                );
                return;
            }

            ui.label(egui::RichText::new("Buses").strong());
            ui.label(
                egui::RichText::new("Master applies on top of the others.")
                    .small()
                    .weak()
                    .italics(),
            );
            ui.add_space(2.0);

            for (bus, vol) in &audio.bus_volumes {
                let mut v = *vol;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(format!("{:<6}", bus.name())).monospace());
                    let r = ui.add(
                        egui::Slider::new(&mut v, 0.0..=1.0)
                            .show_value(true)
                            .fixed_decimals(2),
                    );
                    if r.changed() {
                        *action = Some(EditorAction::SetBusVolume(*bus, v));
                    }
                    if ui
                        .small_button("↺")
                        .on_hover_text(format!("Reset to {:.2}", bus.default_volume()))
                        .clicked()
                    {
                        *action = Some(EditorAction::SetBusVolume(*bus, bus.default_volume()));
                    }
                });
            }

            ui.separator();
            ui.horizontal(|ui| {
                if ui
                    .button("➕ Load sound…")
                    .on_hover_text("wav / ogg / flac / mp3")
                    .clicked()
                {
                    *action = Some(EditorAction::LoadSound);
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("active: {}", audio.active_count))
                        .small()
                        .weak(),
                );
            });

            ui.separator();
            ui.label(format!("{} sounds loaded", audio.sound_names.len()));
            egui::ScrollArea::vertical()
                .max_height(220.0)
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for name in &audio.sound_names {
                        ui.horizontal(|ui| {
                            if ui
                                .small_button("▶")
                                .on_hover_text("Preview on SFX bus")
                                .clicked()
                            {
                                *action = Some(EditorAction::PreviewSound(name.clone()));
                            }
                            ui.label(egui::RichText::new(name).monospace());
                        });
                    }
                });
        });

    state.layout.show_audio_panel = open;
}