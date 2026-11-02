use crate::ecs::World;
use crate::editor::scene_validation::{IssueSeverity, SceneIssueKind};
use crate::editor::{EditorAction, EditorState};

pub fn draw_window(
    ctx: &egui::Context,
    editor: &mut EditorState,
    world: &World,
    action: &mut Option<EditorAction>,
) {
    if !editor.show_validation_panel { return; }
    let mut open_local = editor.show_validation_panel;

    egui::Window::new("🔍 Scene Validation")
        .open(&mut open_local)
        .default_pos(egui::pos2(340.0, 200.0))
        .default_width(720.0)
        .default_height(500.0)
        .resizable(true)
        .show(ctx, |ui| {
            let errors = editor.validation_issues.iter()
                .filter(|i| i.severity == IssueSeverity::Error).count();
            let warnings = editor.validation_issues.iter()
                .filter(|i| i.severity == IssueSeverity::Warning).count();
            let infos = editor.validation_issues.iter()
                .filter(|i| i.severity == IssueSeverity::Info).count();

            ui.horizontal(|ui| {
                if errors > 0 {
                    ui.label(egui::RichText::new(format!("❌ {} errors", errors))
                        .color(egui::Color32::from_rgb(230, 100, 100)).strong());
                } else {
                    ui.label(egui::RichText::new("✅ 0 errors")
                        .color(egui::Color32::from_rgb(100, 200, 130)));
                }
                ui.separator();
                if warnings > 0 {
                    ui.label(egui::RichText::new(format!("⚠ {} warnings", warnings))
                        .color(egui::Color32::from_rgb(230, 200, 100)));
                } else {
                    ui.label(egui::RichText::new("0 warnings").weak());
                }
                ui.separator();
                if infos > 0 {
                    ui.label(egui::RichText::new(format!("ℹ {} info", infos)).weak());
                } else {
                    ui.label(egui::RichText::new("0 info").weak());
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("⟳ Re-validate").clicked() {
                        *action = Some(EditorAction::ValidateScene);
                    }
                });
            });
            ui.separator();

            if editor.validation_issues.is_empty() {
                ui.add_space(20.0);
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("🎉 Scene is clean!")
                        .size(18.0).strong()
                        .color(egui::Color32::from_rgb(100, 200, 130)));
                    ui.label(egui::RichText::new("No issues found.").weak().italics());
                });
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    for issue in &editor.validation_issues {
                        let (icon, color) = match issue.severity {
                            IssueSeverity::Error =>
                                ("❌", egui::Color32::from_rgb(230, 100, 100)),
                            IssueSeverity::Warning =>
                                ("⚠", egui::Color32::from_rgb(230, 200, 100)),
                            IssueSeverity::Info =>
                                ("ℹ", egui::Color32::from_rgb(150, 180, 220)),
                        };

                        egui::Frame::group(ui.style())
                            .inner_margin(egui::Margin::symmetric(8, 6))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(icon).color(color));
                                    ui.label(egui::RichText::new(&issue.message).color(color));

                                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                        if !issue.entities.is_empty() {
                                            if ui.small_button("🗑 Delete")
                                                .on_hover_text("Delete the offending entities")
                                                .clicked() {
                                                    *action = Some(EditorAction::DeleteEntities(
                                                        issue.entities.clone()
                                                    ));
                                                }
                                            if ui.small_button("🎯 Select").clicked() {
                                                *action = Some(EditorAction::SelectEntities(
                                                    issue.entities.clone()
                                                ));
                                            }
                                        }

                                        match issue.kind {
                                            SceneIssueKind::BrokenParent
                                            | SceneIssueKind::CyclicParent => {
                                                if ui.small_button("🔧 Remove Parent").clicked() {
                                                    *action = Some(EditorAction::RemoveParent(
                                                        issue.entities.clone()
                                                    ));
                                                }
                                            }
                                            SceneIssueKind::MissingMesh => {
                                                if ui.small_button("🔧 Use cube").clicked() {
                                                    *action = Some(EditorAction::ReplaceMissingMesh(
                                                        issue.entities.clone(),
                                                        "cube".to_string(),
                                                    ));
                                                }
                                            }
                                            SceneIssueKind::MissingMaterial => {
                                                if ui.small_button("🔧 Use flat_blue").clicked() {
                                                    *action = Some(EditorAction::ReplaceMissingMaterial(
                                                        issue.entities.clone(),
                                                        "flat_blue".to_string(),
                                                    ));
                                                }
                                            }
                                            _ => {}
                                        }
                                    });
                                });

                                if !issue.entities.is_empty() && issue.entities.len() <= 10 {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(egui::RichText::new("Entities:").small().weak());
                                        for &e in issue.entities.iter().take(10) {
                                            let label = entity_label(world, e);
                                            if ui.small_button(label).clicked() {
                                                *action = Some(EditorAction::SelectEntity(e));
                                            }
                                        }
                                        if issue.entities.len() > 10 {
                                            ui.label(
                                                egui::RichText::new(format!(
                                                    "... +{}", issue.entities.len() - 10
                                                )).small().weak()
                                            );
                                        }
                                    });
                                }
                            });
                        ui.add_space(2.0);
                    }
                });
        });

    editor.show_validation_panel = open_local;
}

fn entity_label(world: &World, e: crate::ecs::Entity) -> String {
    use crate::game::components::{MeshHandle, Name};
    if let Some(n) = world.get::<Name>(e) {
        format!("#{} {}", e, n.0)
    } else if let Some(mh) = world.get::<MeshHandle>(e) {
        format!("#{} ({})", e, mh.0)
    } else {
        format!("#{}", e)
    }
}