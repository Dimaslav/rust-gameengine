//! Панель Scripts: список загруженных `.rhai`, привязка/отвязка
//! от entity, показ ошибок парсинга, ручная перезагрузка.

use crate::ecs::{Entity, World};
use crate::editor::{EditorAction, EditorState};
use crate::game::components::Name;
use crate::scripting::ScriptSystem;

pub fn draw_window(
    ctx: &egui::Context,
    open: &mut bool,
    editor: &mut EditorState,
    world: &World,
    scripts: &ScriptSystem,
    action: &mut Option<EditorAction>,
) {
    if !*open {
        return;
    }

    egui::Window::new("📜 Scripts")
        .open(open)
        .default_pos(egui::pos2(360.0, 160.0))
        .default_width(460.0)
        .default_height(520.0)
        .resizable(true)
        .show(ctx, |ui| {
            // ============================================================
            // Заголовок + reload
            // ============================================================
            ui.horizontal(|ui| {
                let n = scripts.engine.loaded_names().len();
                ui.label(format!("{} scripts loaded", n));
                if ui
                    .small_button("⟳ Reload all")
                    .on_hover_text("Перечитать все .rhai с диска")
                    .clicked()
                {
                    *action = Some(EditorAction::ReloadScripts);
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(scripts.engine.root().display().to_string())
                        .small()
                        .weak()
                        .monospace(),
                );
            });
            ui.separator();

            // ============================================================
            // Список файлов
            // ============================================================
            ui.label(egui::RichText::new("Loaded scripts").strong());
            let names = scripts.engine.loaded_names();

            if names.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "assets/scripts/ пуста. Положи .rhai файл — он появится здесь.",
                    )
                    .weak()
                    .italics(),
                );
            } else {
                let primary = editor.primary();

                egui::ScrollArea::vertical()
                    .id_salt("scripts_list")
                    .max_height(160.0)
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        for name in &names {
                            let err = scripts.engine.script_error(name);

                            ui.horizontal(|ui| {
                                if err.is_some() {
                                    ui.label(
                                        egui::RichText::new("⚠")
                                            .color(egui::Color32::from_rgb(230, 130, 130)),
                                    )
                                    .on_hover_text("parse error");
                                } else {
                                    ui.label(
                                        egui::RichText::new("✓")
                                            .color(egui::Color32::from_rgb(130, 200, 130)),
                                    );
                                }

                                ui.label(egui::RichText::new(name).monospace());

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if let Some(e) = primary {
                                            if ui
                                                .small_button("→ selected")
                                                .on_hover_text(format!(
                                                    "Attach '{}' to #{}",
                                                    name, e
                                                ))
                                                .clicked()
                                            {
                                                *action = Some(EditorAction::AttachScript {
                                                    entity: e,
                                                    script: name.clone(),
                                                });
                                            }
                                        }
                                    },
                                );
                            });

                            if let Some(e) = &err {
                                ui.label(
                                    egui::RichText::new(format!("   {}", e))
                                        .small()
                                        .weak()
                                        .italics()
                                        .color(egui::Color32::from_rgb(230, 130, 130)),
                                );
                            }
                        }
                    });
            }

            ui.separator();

            // ============================================================
            // Привязанные entity
            // ============================================================
            ui.label(egui::RichText::new("Entities with scripts").strong());

            let scripted: Vec<(Entity, String, bool, bool)> = world
                .query::<crate::scripting::ScriptComponent>()
                .map(|(e, c)| (e, c.script.clone(), c.started, c.enabled))
                .collect();

            if scripted.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "Ни одна entity не имеет ScriptComponent. \
                         Выдели объект и нажми «→ selected».",
                    )
                    .weak()
                    .italics(),
                );
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("scripted_entities")
                    .auto_shrink([false; 2])
                    .show(ui, |ui| {
                        for (e, script, started, enabled) in scripted {
                            ui.horizontal(|ui| {
                                let name = world
                                    .get::<Name>(e)
                                    .map(|n| n.0.clone())
                                    .unwrap_or_else(|| format!("#{}", e));

                                // Select
                                if ui
                                    .small_button("🎯")
                                    .on_hover_text("Select this entity")
                                    .clicked()
                                {
                                    *action = Some(EditorAction::SelectEntity(e));
                                }

                                let label = egui::RichText::new(&name).monospace();
                                let label = if editor.is_selected(e) {
                                    label.color(egui::Color32::from_rgb(230, 200, 130))
                                } else {
                                    label
                                };
                                ui.label(label);

                                ui.label("→");
                                ui.label(egui::RichText::new(&script).monospace());

                                if !enabled {
                                    ui.label(
                                        egui::RichText::new("(disabled)").small().weak(),
                                    );
                                }
                                if !started {
                                    ui.label(
                                        egui::RichText::new("(pending on_start)")
                                            .small()
                                            .weak()
                                            .italics(),
                                    );
                                }

                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui
                                            .small_button("✖")
                                            .on_hover_text("Detach script")
                                            .clicked()
                                        {
                                            *action = Some(EditorAction::DetachScript(e));
                                        }
                                    },
                                );
                            });
                        }
                    });
            }

            ui.separator();
            ui.label(
                egui::RichText::new(
                    "Tip: hot-reload работает по mtime (раз в 0.5 сек). \
                     Отредактируй .rhai и сохрани — on_start сбросится, \
                     скрипт перезапустится.",
                )
                .small()
                .weak()
                .italics(),
            );
        });
}