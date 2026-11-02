//! Command palette (Ctrl+P).

use crate::editor::{EditorAction, EditorState};

#[derive(Clone)]
struct PaletteCommand {
    label: &'static str,
    action: EditorAction,
}

fn palette_commands() -> Vec<PaletteCommand> {
    vec![
        PaletteCommand { label: "File · Save scene", action: EditorAction::Save },
        PaletteCommand { label: "File · Load scene", action: EditorAction::Load },
        PaletteCommand { label: "File · New scene", action: EditorAction::NewScene },
        PaletteCommand { label: "Audio · Load sound…", action: EditorAction::LoadSound },
        PaletteCommand { label: "AI · Bake navmesh", action: EditorAction::BakeNavmesh },
        PaletteCommand { label: "FBX · Export all", action: EditorAction::ExportFbxAll },
        PaletteCommand { label: "FBX · Export selected", action: EditorAction::ExportFbxSelected },
        PaletteCommand { label: "FBX · Import…", action: EditorAction::ImportFbx },
        PaletteCommand { label: "Textures · Load sRGB (color)…", action: EditorAction::LoadTextures },
        PaletteCommand { label: "Textures · Load Linear (normal/data)…", action: EditorAction::LoadTexturesLinear },
        PaletteCommand { label: "Scene · Add cube", action: EditorAction::AddCube },
        PaletteCommand { label: "Scene · Add sphere", action: EditorAction::AddSphere },
        PaletteCommand { label: "Scene · Duplicate selection", action: EditorAction::Duplicate },
        PaletteCommand { label: "Scene · Delete selection", action: EditorAction::DeleteSelected },
        PaletteCommand { label: "Scene · Select all", action: EditorAction::SelectAll },
        PaletteCommand { label: "Scene · Deselect all", action: EditorAction::DeselectAll },
        PaletteCommand { label: "Scene · Invert selection", action: EditorAction::InvertSelection },
        PaletteCommand { label: "Scene · Cleanup empty entities", action: EditorAction::CleanupEmptyEntities },
        PaletteCommand { label: "Scene · Copy selection", action: EditorAction::CopyEntity },
        PaletteCommand { label: "Scene · Paste", action: EditorAction::PasteEntity },
        PaletteCommand { label: "Camera · Focus on selection", action: EditorAction::FocusSelected },
        PaletteCommand { label: "Camera · Front view", action: EditorAction::CameraPreset(0) },
        PaletteCommand { label: "Camera · Back view", action: EditorAction::CameraPreset(1) },
        PaletteCommand { label: "Camera · Right view", action: EditorAction::CameraPreset(2) },
        PaletteCommand { label: "Camera · Left view", action: EditorAction::CameraPreset(3) },
        PaletteCommand { label: "Camera · Top view", action: EditorAction::CameraPreset(4) },
        PaletteCommand { label: "Camera · Bottom view", action: EditorAction::CameraPreset(5) },
        PaletteCommand { label: "Camera · Isometric view", action: EditorAction::CameraPreset(6) },
        PaletteCommand { label: "Edit · Undo", action: EditorAction::Undo },
        PaletteCommand { label: "Edit · Redo", action: EditorAction::Redo },
        PaletteCommand { label: "Play · Toggle play mode", action: EditorAction::TogglePlay },
        PaletteCommand { label: "Play · Spawn player here", action: EditorAction::SpawnPlayerHere },
        PaletteCommand { label: "Prefab · Save selection…", action: EditorAction::SavePrefab },
        PaletteCommand { label: "Prefab · Refresh list", action: EditorAction::RefreshPrefabs },
        PaletteCommand { label: "Terrain · Re-generate splat", action: EditorAction::TerrainRegenerate },
        PaletteCommand { label: "Terrain · Save R16", action: EditorAction::TerrainSaveFile },
        PaletteCommand { label: "Terrain · Export PNG", action: EditorAction::TerrainSavePNG },
        PaletteCommand { label: "Terrain · Load (PNG/R16)", action: EditorAction::TerrainLoadFile },
    ]
}

fn fuzzy_match(query: &str, label: &str) -> Option<usize> {
    if query.is_empty() { return Some(0); }
    let q = query.to_lowercase();
    let l = label.to_lowercase();
    l.find(&q)
}

pub fn draw(
    ctx: &egui::Context,
    state: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    let mut close = false;

    // ИЗМЕНЕНО (egui 0.37): content_rect() возвращает Rect
    let screen = ctx.input(|i| i.content_rect());

    egui::Area::new(egui::Id::unique("command_palette"))
        .order(egui::Order::Foreground)
        .fixed_pos(egui::pos2(
            screen.center().x - 250.0,
            screen.top() + 100.0,
        ))
        .show(ctx, |ui| {
            egui::Frame::popup(ui.style())
                .inner_margin(egui::Margin::same(12))
                .show(ui, |ui| {
                    ui.set_min_width(500.0);
                    ui.set_max_width(500.0);
                    ui.label(egui::RichText::new("Command palette").strong().small());
                    let response = ui.add(
                        egui::TextEdit::singleline(&mut state.command_palette_query)
                            .desired_width(f32::INFINITY)
                            .hint_text("Type to filter commands…"),
                    );
                    response.request_focus();

                    let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                    let esc = ui.input(|i| i.key_pressed(egui::Key::Escape));
                    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));

                    let commands = palette_commands();
                    let query = state.command_palette_query.clone();
                    let mut filtered: Vec<(usize, &PaletteCommand)> = commands
                        .iter()
                        .filter_map(|c| fuzzy_match(&query, c.label).map(|pos| (pos, c)))
                        .collect();
                    filtered.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.label.cmp(b.1.label)));

                    if !filtered.is_empty() {
                        if up {
                            state.command_palette_selected = state.command_palette_selected.saturating_sub(1);
                        }
                        if down {
                            state.command_palette_selected = (state.command_palette_selected + 1).min(filtered.len() - 1);
                        }
                        if state.command_palette_selected >= filtered.len() {
                            state.command_palette_selected = filtered.len() - 1;
                        }
                    } else {
                        state.command_palette_selected = 0;
                    }

                    if enter && !filtered.is_empty() {
                        let cmd = filtered[state.command_palette_selected].1.clone();
                        *action = Some(cmd.action);
                        close = true;
                    }
                    if esc { close = true; }

                    ui.add_space(6.0);
                    egui::ScrollArea::vertical()
                        .max_height(320.0).auto_shrink([false; 2])
                        .show(ui, |ui| {
                            if filtered.is_empty() {
                                ui.label(egui::RichText::new("no matching commands").weak().italics());
                            }
                            for (idx, (_, cmd)) in filtered.iter().enumerate() {
                                let selected = idx == state.command_palette_selected;
                                let resp = ui.selectable_label(selected, cmd.label);
                                if resp.clicked() {
                                    *action = Some(cmd.action.clone());
                                    close = true;
                                }
                                if resp.hovered() {
                                    state.command_palette_selected = idx;
                                }
                            }
                        });
                });
        });
    if close {
        state.command_palette_open = false;
        state.command_palette_query.clear();
        state.command_palette_selected = 0;
    }
}