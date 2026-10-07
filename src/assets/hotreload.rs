//! Polling-based hot-reload. Раз в ~0.75 сек проверяет, не
//! изменились ли ассеты, и передаёт изменения в `Renderer`.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use super::database::AssetDatabase;
use super::id::AssetId;
use super::meta::AssetKind;
use crate::render::Renderer;

const INTERVAL_SEC: f32 = 0.75;

pub struct HotReload {
    last_check: Instant,
    interval: Duration,
}

impl HotReload {
    pub fn new() -> Self {
        Self {
            last_check: Instant::now(),
            interval: Duration::from_secs_f32(INTERVAL_SEC),
        }
    }

    /// Вызывается каждый кадр. Возвращает список перезагруженных ассетов.
    pub fn tick(
        &mut self,
        db: &mut AssetDatabase,
        renderer: &mut Renderer,
    ) -> Vec<AssetId> {
        if self.last_check.elapsed() < self.interval {
            return Vec::new();
        }
        self.last_check = Instant::now();

        let changed = db.detect_changes();
        if changed.is_empty() {
            return Vec::new();
        }

        let mut reloaded: Vec<AssetId> = Vec::new();
        let mut seen_textures: HashSet<String> = HashSet::new();

        for id in &changed {
            let Some(meta) = db.get(*id) else {
                continue;
            };

            match meta.kind {
                AssetKind::Texture => {
                    let tex_name = meta.stem();
                    if !seen_textures.insert(tex_name.clone()) {
                        continue;
                    }

                    let path_str = meta.path.to_string_lossy().into_owned();
                    let is_srgb = meta.import_settings.srgb;

                    let result = if is_srgb {
                        renderer.load_texture(&tex_name, &path_str)
                    } else {
                        renderer.load_texture_linear(&tex_name, &path_str)
                    };

                    match result {
                        Ok(()) => {
                            log::info!(
                                "HotReload: texture '{}' reloaded from {}",
                                tex_name,
                                path_str
                            );
                            rebuild_materials_using(renderer, &tex_name);
                            reloaded.push(*id);
                        }
                        Err(e) => log::warn!(
                            "HotReload: failed to reload texture '{}': {}",
                            tex_name,
                            e
                        ),
                    }
                }
                _ => {
                    log::debug!(
                        "HotReload: {} changed ({}), no reloader yet",
                        meta.file_name(),
                        meta.kind.name()
                    );
                }
            }
        }

        reloaded
    }
}

impl Default for HotReload {
    fn default() -> Self {
        Self::new()
    }
}

/// Пересобрать bind-group'ы всех материалов, ссылающихся на `texture_name`.
fn rebuild_materials_using(renderer: &mut Renderer, texture_name: &str) {
    let mut to_rebuild: Vec<(String, crate::render::Material)> = Vec::new();

    for (name, mat) in renderer.materials.iter() {
        let refs = mat.referenced_textures();
        if refs.iter().any(|n| *n == texture_name) {
            to_rebuild.push((name.clone(), mat.clone()));
        }
    }

    for (name, mat) in to_rebuild {
        renderer.update_material(&name, mat);
    }
}