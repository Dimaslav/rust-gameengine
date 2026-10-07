use super::MenuActions;

pub fn draw(ctx: &egui::Context, has_save: bool, actions: &mut MenuActions) {
    let screen = ctx.screen_rect();
    let center = screen.center();

    egui::Area::new(egui::Id::new("main_menu"))
        .fixed_pos(egui::pos2(center.x - 200.0, center.y - 200.0))
        .show(ctx, |ui| {
            ui.set_width(400.0);
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.label(
                    egui::RichText::new("RUST ENGINE 3D")
                        .size(44.0)
                        .strong()
                        .color(egui::Color32::from_rgb(230, 200, 130)),
                );
                ui.add_space(4.0);
                ui.label(
                    egui::RichText::new(format!("v{}", env!("CARGO_PKG_VERSION")))
                        .size(12.0)
                        .weak(),
                );
                ui.add_space(48.0);

                if ui.add_sized([300.0, 46.0], egui::Button::new("▶   New Game")).clicked() {
                    actions.new_game = true;
                }
                ui.add_space(8.0);

                let cont = ui.add_enabled(
                    has_save,
                    egui::Button::new("📂   Continue")
                        .min_size(egui::vec2(300.0, 46.0)),
                );
                if cont.clicked() {
                    actions.continue_game = true;
                }
                ui.add_space(8.0);

                if ui.add_sized([300.0, 46.0], egui::Button::new("⚙   Settings")).clicked() {
                    actions.open_settings = true;
                }
                ui.add_space(8.0);

                if ui.add_sized([300.0, 46.0], egui::Button::new("✕   Quit")).clicked() {
                    actions.quit = true;
                }

                ui.add_space(40.0);
                ui.label(
                    egui::RichText::new("FPS / RPG engine written in Rust")
                        .small()
                        .weak()
                        .italics(),
                );
                ui.label(
                    egui::RichText::new("github.com/your-username/rust-engine")
                        .small()
                        .weak(),
                );
            });
        });
}