//! Content Browser: дерево ассетов проекта.
//! Показывает всё, что нашла `AssetDatabase`.

use std::collections::BTreeMap;

use crate::assets::{AssetDatabase, AssetId, AssetKind};

pub fn draw_window(
    ctx: &egui::Context,
    open: &mut bool,
    db: &mut AssetDatabase,
) {
    if !*open {
        return;
    }

    egui::Window::new("📁 Content Browser")
        .open(open)
        .default_pos(egui::pos2(340.0, 400.0))
        .default_width(520.0)
        .default_height(380.0)
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("{} assets", db.len()));
                if ui.small_button("⟳ Rescan").clicked() {
                    db.rescan();
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

            // Snapshot — чтобы не держать borrow на db.
            let mut groups: BTreeMap<&'static str, Vec<(AssetId, String, String)>> =
                BTreeMap::new();
            for meta in db.iter() {
                groups
                    .entry(meta.kind.name())
                    .or_default()
                    .push((meta.id, meta.stem(), meta.path.to_string_lossy().into_owned()));
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false; 2])
                .show(ui, |ui| {
                    if groups.is_empty() {
                        ui.label(
                            egui::RichText::new(
                                "assets/ пуста. Положи .png / .glb / .hdr — они появятся здесь.",
                            )
                            .weak()
                            .italics(),
                        );
                        return;
                    }

                    for (kind_name, list) in groups {
                        egui::CollapsingHeader::new(format!("{} ({})", kind_name, list.len()))
                            .default_open(true)
                            .show(ui, |ui| {
                                for (id, stem, path_str) in list {
                                    draw_asset_row(ui, db, id, &stem, &path_str);
                                }
                            });
                    }
                });
        });
}

fn draw_asset_row(
    ui: &mut egui::Ui,
    db: &mut AssetDatabase,
    id: AssetId,
    stem: &str,
    path_str: &str,
) {
    let (icon, kind, srgb, mips, scale, excluded) = match db.get(id) {
        Some(m) => (
            m.kind.icon(),
            m.kind,
            m.import_settings.srgb,
            m.import_settings.generate_mips,
            m.import_settings.import_scale,
            m.import_settings.excluded_from_build,
        ),
        None => return,
    };

    let mut new_srgb = srgb;
    let mut new_mips = mips;
    let mut new_scale = scale;
    let mut new_excluded = excluded;
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.label(format!("{} {}", icon, stem)).on_hover_text(path_str);

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if kind == AssetKind::Texture {
                if ui
                    .selectable_label(new_srgb, if new_srgb { "sRGB" } else { "Linear" })
                    .on_hover_text("sRGB для albedo/emissive, Linear для normal/MR/AO")
                    .clicked()
                {
                    new_srgb = !new_srgb;
                    changed = true;
                }
                if ui
                    .selectable_label(new_mips, "mips")
                    .on_hover_text("Генерировать мипмапы")
                    .clicked()
                {
                    new_mips = !new_mips;
                    changed = true;
                }
            }

            if kind == AssetKind::Mesh {
                if ui
                    .add(
                        egui::DragValue::new(&mut new_scale)
                            .speed(0.01)
                            .range(0.001..=1000.0)
                            .prefix("×"),
                    )
                    .changed()
                {
                    changed = true;
                }
            }

            if ui
                .selectable_label(new_excluded, "🗑")
                .on_hover_text("Не включать в билд")
                .clicked()
            {
                new_excluded = !new_excluded;
                changed = true;
            }
        });
    });

    if changed {
        let mut settings = match db.get(id) {
            Some(m) => m.import_settings.clone(),
            None => return,
        };
        settings.srgb = new_srgb;
        settings.generate_mips = new_mips;
        settings.import_scale = new_scale;
        settings.excluded_from_build = new_excluded;
        if let Err(e) = db.update_import_settings(id, settings) {
            log::warn!("Failed to save asset settings: {}", e);
        }
    }
}