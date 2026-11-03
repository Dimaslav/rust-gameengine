use std::collections::BTreeMap;

use crate::assets::{AssetDatabase, AssetKind, AssetMeta};
use crate::editor::panels::UiAssets;
use crate::editor::{EditorAction, EditorState, ThumbnailCache, ThumbnailState};

// ============================================================
// Content Browser (модальное окно)
// ============================================================

pub fn draw_window(
    ctx: &egui::Context,
    open: &mut bool,
    db: &mut AssetDatabase,
    thumbnails: &mut ThumbnailCache,
) {
    if !*open {
        return;
    }

    let mut open_local = *open;
    egui::Window::new("📁 Content Browser")
        .open(&mut open_local)
        .default_pos(egui::pos2(340.0, 400.0))
        .default_width(720.0)
        .default_height(480.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} assets", db.len()));
                if ui.small_button("⟳ Rescan").clicked() {
                    db.rescan();
                    thumbnails.clear();
                }
                if ui.small_button("🗑 Clear cache").clicked() {
                    thumbnails.clear();
                }
                ui.separator();
                ui.label(
                    egui::RichText::new(format!("root: {}", db.root().display()))
                        .small()
                        .weak()
                        .monospace(),
                );
            });
            ui.separator();

            let mut groups: BTreeMap<&'static str, Vec<AssetMeta>> = BTreeMap::new();
            for meta in db.iter() {
                groups.entry(meta.kind.name()).or_default().push(meta.clone());
            }
            let ordered: Vec<(&'static str, Vec<AssetMeta>)> = groups.into_iter().collect();

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if ordered.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "assets/ пуста. Положи .png / .glb / .hdr — они появятся здесь.",
                            )
                            .weak()
                            .italics(),
                        );
                        return;
                    }

                    for (kind_name, list) in ordered {
                        egui::CollapsingHeader::new(format!(
                            "{} ({})",
                            kind_name,
                            list.len()
                        ))
                        .default_open(true)
                        .show(ui, |ui| {
                            draw_card_grid(ui, ctx, db, thumbnails, &list);
                        });
                    }
                });
        });

    *open = open_local;
}

// ============================================================
// Сетка карточек
// ============================================================

const CARD_SIZE: f32 = 96.0;
const CARD_GAP: f32 = 8.0;
const CARD_NAME_HEIGHT: f32 = 20.0;
const THUMB_RESOLUTION: u32 = 128;

fn draw_card_grid(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    db: &mut AssetDatabase,
    thumbnails: &mut ThumbnailCache,
    list: &[AssetMeta],
) {
    let full_width = ui.available_width().max(CARD_SIZE);
    let per_row = ((full_width + CARD_GAP) / (CARD_SIZE + CARD_GAP))
        .floor()
        .max(1.0) as usize;

    for chunk in list.chunks(per_row) {
        ui.horizontal(|ui| {
            for meta in chunk {
                draw_card(ui, ctx, db, thumbnails, meta);
            }
        });
        ui.add_space(CARD_GAP);
    }
}

fn draw_card(
    ui: &mut egui::Ui,
    ctx: &egui::Context,
    db: &mut AssetDatabase,
    thumbnails: &mut ThumbnailCache,
    meta: &AssetMeta,
) {
    let stem = meta.stem();
    let kind = meta.kind;
    let full_path = meta.path.to_string_lossy().into_owned();
    let asset_id = meta.id;

    ensure_thumbnail(ctx, thumbnails, &stem, kind, &full_path);

    let card_size = egui::vec2(CARD_SIZE, CARD_SIZE + CARD_NAME_HEIGHT);
        let drag_id = egui::Id::unique(("asset_drag", asset_id));
    
    // ИЗМЕНЕНО: оборачиваем в dnd_drag_source
    let response = ui.dnd_drag_source(drag_id, asset_id, |ui| {
        let (rect, response) = ui.allocate_exact_size(card_size, egui::Sense::click());

        let hovered = response.hovered();
        let bg = if hovered {
            egui::Color32::from_rgb(50, 55, 68)
        } else {
            egui::Color32::from_rgb(32, 35, 44)
        };
        let border = if hovered {
            egui::Color32::from_rgb(120, 170, 230)
        } else {
            egui::Color32::from_rgb(60, 66, 80)
        };

        ui.painter().rect_filled(rect, egui::CornerRadius::same(4), bg);
        ui.painter().rect_stroke(
            rect,
            egui::CornerRadius::same(4),
            egui::Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );

        let thumb_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(4.0, 4.0),
            egui::vec2(CARD_SIZE - 8.0, CARD_SIZE - 8.0),
        );

        match thumbnails.get(&stem) {
            Some(ThumbnailState::Loaded(handle)) => {
                let painter = ui.painter_at(thumb_rect);
                painter.image(
                    handle.id(),
                    thumb_rect,
                    egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                    egui::Color32::WHITE,
                );
            }
            Some(ThumbnailState::Failed) | Some(ThumbnailState::NoSource) | None => {
                let icon = kind.icon();
                let font_id = egui::FontId::proportional(40.0);
                ui.painter().text(
                    thumb_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    icon,
                    font_id,
                    egui::Color32::from_rgb(200, 210, 230),
                );
            }
        }

        let name_rect = egui::Rect::from_min_size(
            rect.min + egui::vec2(0.0, CARD_SIZE),
            egui::vec2(CARD_SIZE, CARD_NAME_HEIGHT),
        );
        let text = truncate(&stem, 14);
        ui.painter().text(
            name_rect.center(),
            egui::Align2::CENTER_CENTER,
            &text,
            egui::FontId::proportional(11.0),
            egui::Color32::from_rgb(220, 225, 235),
        );

        response
    }).response;

    response
        .clone()
        .on_hover_text(format!("{}\n{}", full_path, kind.name()));

    response.context_menu(|ui| {
        if kind == AssetKind::Texture {
            let is_srgb = meta.import_settings.srgb;
            let mut new_srgb = is_srgb;
            if ui
                .selectable_label(new_srgb, if new_srgb { "sRGB ✓" } else { "sRGB" })
                .clicked()
            {
                new_srgb = !new_srgb;
                if let Some(m) = db.get(meta.id) {
                    let mut s = m.import_settings.clone();
                    s.srgb = new_srgb;
                    if let Err(e) = db.update_import_settings(meta.id, s) {
                        log::warn!("Failed to save asset settings: {}", e);
                    }
                }
                thumbnails.invalidate(&stem);
                ui.close();
            }
            if ui.button("Remove texture from renderer").clicked() {
                ui.close();
            }
        }
    });
}

// ============================================================
// Загрузка / кэш превью
// ============================================================

fn ensure_thumbnail(
    ctx: &egui::Context,
    cache: &mut ThumbnailCache,
    stem: &str,
    kind: AssetKind,
    full_path: &str,
) {
    if cache.contains(stem) {
        return;
    }

    if kind != AssetKind::Texture {
        cache.insert_no_source(stem.to_string());
        return;
    }

    match load_thumbnail_from_file(full_path) {
        Ok((w, h, rgba)) => {
            let image = egui::ColorImage::from_rgba_unmultiplied(
                [w as usize, h as usize],
                &rgba,
            );
            let handle = ctx.load_texture(
                format!("thumb::{}", stem),
                image,
                egui::TextureOptions::LINEAR,
            );
            cache.insert_loaded(stem.to_string(), handle);
        }
        Err(e) => {
            log::debug!("Thumbnail load failed for '{}': {}", full_path, e);
            cache.insert_failed(stem.to_string());
        }
    }
}

fn load_thumbnail_from_file(path: &str) -> Result<(u32, u32, Vec<u8>), String> {
    let img = image::open(path).map_err(|e| format!("open: {}", e))?;
    let thumb = img.thumbnail(THUMB_RESOLUTION, THUMB_RESOLUTION);
    let rgba = thumb.to_rgba8();
    let (w, h) = rgba.dimensions();
    Ok((w, h, rgba.into_raw()))
}

// ============================================================
// Утилиты
// ============================================================

fn truncate(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(max_chars.saturating_sub(1)).collect();
        out.push('…');
        out
    }
}

// ============================================================
// Prefabs section
// ============================================================

pub fn draw_prefabs_section(
    ui: &mut egui::Ui,
    editor: &mut EditorState,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Prefabs");
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("📁");
        ui.add(
            egui::TextEdit::singleline(&mut editor.prefabs_dir)
                .desired_width(140.0)
                .hint_text("prefabs"),
        );
        if ui.small_button("⟳").clicked() {
            *action = Some(EditorAction::RefreshPrefabs);
        }
    });
    ui.horizontal(|ui| {
        ui.label("Name:");
        ui.add(
            egui::TextEdit::singleline(&mut editor.prefab_save_name)
                .desired_width(120.0)
                .hint_text("my_prefab"),
        );
        let can_save =
            !editor.selected.is_empty() && !editor.prefab_save_name.trim().is_empty();
        if ui
            .add_enabled(can_save, egui::Button::new("💾 Save Sel"))
            .clicked()
        {
            *action = Some(EditorAction::SavePrefab);
        }
    });
    ui.separator();
    let prefab_count = editor.prefab_list.len();
    if prefab_count == 0 {
        ui.label(egui::RichText::new("no prefabs found").weak().italics());
    } else {
        ui.label(format!("{} files", prefab_count));
        egui::ScrollArea::vertical()
            .auto_shrink([false; 2])
            .max_height(200.0)
            .show(ui, |ui| {
                for (idx, path) in editor.prefab_list.iter().enumerate() {
                    let display = crate::scene::prefab::prefab_display_name(path);
                    let resp = ui.button(format!("📦 {}", display));
                    if resp.clicked() {
                        *action = Some(EditorAction::InstantiatePrefab(idx as u32));
                    }
                }
            });
    }
}

// ============================================================
// Assets section
// ============================================================

pub fn draw_assets_section(
    ui: &mut egui::Ui,
    assets: &UiAssets<'_>,
    action: &mut Option<EditorAction>,
) {
    ui.heading("Assets");
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        if ui.button("➕ Load sRGB (color)…").clicked() {
            *action = Some(EditorAction::LoadTextures);
        }
        if ui.button("➕ Load Linear (data)…").clicked() {
            *action = Some(EditorAction::LoadTexturesLinear);
        }
    });

    ui.separator();
    let tex_count = assets.texture_list.len();
    if tex_count == 0 {
        ui.label(egui::RichText::new("no textures loaded").weak().italics());
        return;
    }
    ui.label(format!("{} textures", tex_count));

    egui::ScrollArea::vertical()
        .auto_shrink([false; 2])
        .max_height(180.0)
        .show(ui, |ui| {
            for (name, _w, _h, is_srgb) in assets.texture_list {
                let tag = if *is_srgb { "[sRGB]" } else { "[Linear]" };
                let tag_color = if *is_srgb {
                    egui::Color32::from_rgb(230, 200, 130)
                } else {
                    egui::Color32::from_rgb(150, 200, 230)
                };
                let resp = ui
                    .horizontal(|ui| {
                        ui.label(
                            egui::RichText::new(tag)
                                .small()
                                .monospace()
                                .color(tag_color),
                        );
                        ui.button(format!("🖼 {}", name))
                    })
                    .inner;
                resp.context_menu(|ui| {
                    if ui.button("Remove texture").clicked() {
                        *action = Some(EditorAction::RemoveTexture(name.clone()));
                        ui.close();
                    }
                });
            }
        });
}