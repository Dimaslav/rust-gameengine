use crate::editor::gizmo::GizmoMode;
use crate::editor::panels::Stats;
use crate::editor::{EditorAction, EditorState};
use crate::game::world_gen::Biome;
use egui::containers::menu::MenuBar;

fn random_seed() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42)
        ^ (std::process::id() as u64)
}

fn random_biome(seed: u64) -> Biome {
    let idx = (seed % 3) as usize;
    Biome::ALL[idx]
}

pub fn draw_menu(ui: &mut egui::Ui, state: &mut EditorState, action: &mut Option<EditorAction>) {
    egui::Panel::top("menu_bar").show(ui, |ui| {
        MenuBar::new().ui(ui, |ui| {
            ui.menu_button("File", |ui| {
                if ui.button("New Scene").clicked() {
                    *action = Some(EditorAction::NewScene);
                    ui.close();
                }
                if ui.button("Open...").clicked() {
                    *action = Some(EditorAction::Load);
                    ui.close();
                }
                if ui.button("Save").clicked() {
                    *action = Some(EditorAction::Save);
                    ui.close();
                }
                if ui.button("Save As...").clicked() {
                    *action = Some(EditorAction::QuickSave);
                    ui.close();
                }
                ui.separator();
                if ui.button("Exit").clicked() {
                    std::process::exit(0);
                }
            });
            ui.menu_button("Edit", |ui| {
                if ui.button("Undo").clicked() {
                    *action = Some(EditorAction::Undo);
                    ui.close();
                }
                if ui.button("Redo").clicked() {
                    *action = Some(EditorAction::Redo);
                    ui.close();
                }
            });
            ui.menu_button("Assets", |ui| {
                if ui.button("Refresh Prefabs").clicked() {
                    *action = Some(EditorAction::RefreshPrefabs);
                    ui.close();
                }
                if ui.button("Import FBX...").clicked() {
                    *action = Some(EditorAction::ImportFbx);
                    ui.close();
                }
                if ui.button("Export FBX (All)").clicked() {
                    *action = Some(EditorAction::ExportFbxAll);
                    ui.close();
                }
            });
            ui.menu_button("GameObject", |ui| {
                if ui.button("Create Empty").clicked() {
                    ui.close();
                }
                if ui.button("3D Object / Cube").clicked() {
                    *action = Some(EditorAction::AddCube);
                    ui.close();
                }
                if ui.button("3D Object / Sphere").clicked() {
                    *action = Some(EditorAction::AddSphere);
                    ui.close();
                }
                ui.separator();
                
                // Полностью случайный мир (Биом + Seed)
                if ui.button("🎲 Random World").on_hover_text("Generate a completely random world (any biome)").clicked() {
                    let new_seed = random_seed();
                    let biome = random_biome(new_seed);
                    state.last_gen_config.seed = new_seed;
                    
                    log::info!("Requesting RANDOM world: {:?} seed={}", biome, new_seed);
                    
                    *action = Some(EditorAction::GenerateWorld {
                        biome,
                        seed: new_seed,
                    });
                    ui.close();
                }
                
                ui.separator();
                ui.menu_button("Generate Specific Biome", |ui| {
                    for biome in Biome::ALL {
                        if ui
                            .button(biome.label())
                            .on_hover_text(format!("{} (Random Seed)", biome.description()))
                            .clicked()
                        {
                            let new_seed = random_seed();
                            state.last_gen_config.seed = new_seed;
                            
                            log::info!("Requesting world: {:?} seed={}", biome, new_seed);
                            
                            *action = Some(EditorAction::GenerateWorld {
                                biome: *biome,
                                seed: new_seed,
                            });
                            ui.close();
                        }
                    }
                });
            });
            ui.menu_button("Component", |ui| {
                ui.label("(Use Inspector '+' button)");
            });
            ui.menu_button("Window", |ui| {
                ui.checkbox(&mut state.layout.show_inspector_panel, "Inspector");
                ui.checkbox(&mut state.layout.show_stats_panel, "Stats");
                ui.checkbox(&mut state.layout.show_content_browser, "Content Browser");
                ui.checkbox(&mut state.layout.show_script_panel, "Scripts");
                ui.checkbox(&mut state.layout.show_audio_panel, "Audio");
            });
            ui.menu_button("Help", |ui| {
                ui.label("Rust Engine 3D v0.3.0");
            });
        });
    });
}

pub fn draw_toolbar(
    ui: &mut egui::Ui,
    state: &mut EditorState,
    stats: &Stats,
    action: &mut Option<EditorAction>,
) {
    egui::Panel::top("toolbar")
        .frame(
            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(35, 35, 35))
                .inner_margin(egui::Margin::symmetric(4, 4)),
        )
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(state.gizmo.mode == GizmoMode::Translate, "✥")
                    .clicked()
                {
                    state.gizmo.mode = GizmoMode::Translate;
                }
                if ui
                    .selectable_label(state.gizmo.mode == GizmoMode::Rotate, "⟳")
                    .clicked()
                {
                    state.gizmo.mode = GizmoMode::Rotate;
                }
                if ui
                    .selectable_label(state.gizmo.mode == GizmoMode::Scale, "⤡")
                    .clicked()
                {
                    state.gizmo.mode = GizmoMode::Scale;
                }

                ui.separator();
                if ui.button("Save").clicked() {
                    *action = Some(EditorAction::Save);
                }

                ui.with_layout(
                    egui::Layout::left_to_right(egui::Align::Center).with_main_justify(true),
                    |ui| {
                        let avail = ui.available_width();
                        ui.add_space(avail / 2.0 - 50.0);

                        let play_icon = if state.play.active { "⏹" } else { "▶" };
                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new(play_icon).size(18.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            *action = Some(EditorAction::TogglePlay);
                        }

                        if ui
                            .add(
                                egui::Button::new(egui::RichText::new("⏸").size(18.0))
                                    .frame(false),
                            )
                            .clicked()
                        {
                            *action = Some(EditorAction::PausePlay);
                        }
                    },
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label("v0.3.0 (dev)");
                    ui.label(
                        egui::RichText::new(format!("{:.0} FPS", stats.fps))
                            .color(egui::Color32::GREEN),
                    );
                    ui.label("Vulkan");
                    ui.separator();
                    ui.label(if state.play.active { "Playing" } else { "Ready" });
                });
            });
        });
}