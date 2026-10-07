//! AssetDatabase: сканирует `assets/`, строит индекс, следит за
//! изменениями, обеспечивает lookup по ID и по пути.

use std::collections::HashMap;
use std::hash::Hasher;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use super::id::AssetId;
use super::meta::{AssetKind, AssetMeta, AssetMetaFile, ImportSettings};

pub struct AssetDatabase {
    by_id: HashMap<AssetId, AssetMeta>,
    by_path: HashMap<PathBuf, AssetId>,
    root: PathBuf,
    /// Счётчик изменений с последнего скана.
    pub revision: u64,
}

impl AssetDatabase {
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        if !root.exists() {
            std::fs::create_dir_all(&root)
                .with_context(|| format!("create assets dir {}", root.display()))?;
        }

        let mut db = Self {
            by_id: HashMap::new(),
            by_path: HashMap::new(),
            root,
            revision: 0,
        };
        db.rescan();
        Ok(db)
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Полное сканирование. Вызывается при старте и по кнопке в UI.
    pub fn rescan(&mut self) {
        self.by_id.clear();
        self.by_path.clear();

        let root = self.root.clone();
        self.scan_dir(&root);

        self.revision += 1;
        log::info!(
            "AssetDatabase: scanned {} → {} assets",
            self.root.display(),
            self.by_id.len()
        );
    }

    fn scan_dir(&mut self, dir: &Path) {
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };

        for entry in entries.flatten() {
            let path = entry.path();

            if path.is_dir() {
                self.scan_dir(&path);
                continue;
            }

            // Игнорируем `.meta`-файлы.
            if path.to_string_lossy().ends_with(".meta") {
                continue;
            }

            let Some(kind) = Self::kind_of(&path) else {
                continue;
            };
            self.register(&path, kind);
        }
    }

    fn kind_of(path: &Path) -> Option<AssetKind> {
        let ext = path.extension()?.to_str()?.to_lowercase();
        let stem = path.file_stem()?.to_str()?.to_lowercase();

        Some(match ext.as_str() {
            "glb" | "gltf" | "fbx" | "obj" | "dae" => AssetKind::Mesh,
            "png" | "jpg" | "jpeg" | "exr" | "hdr" | "tga" | "dds" | "bmp" | "tif" | "tiff"
            | "webp" | "qoi" => AssetKind::Texture,
            "wav" | "ogg" | "mp3" | "flac" => AssetKind::Audio,
            "wgsl" | "glsl" | "vert" | "frag" => AssetKind::Shader,
            "ron" if stem.ends_with(".prefab") => AssetKind::Prefab,
            "ron" if stem.ends_with(".scene") => AssetKind::Scene,
            "ron" if stem.ends_with(".anim_events") => AssetKind::Animation,
            "ron" => AssetKind::Scene,
            "rs" | "rhai" | "lua" => AssetKind::Script,
            _ => return None,
        })
    }

    fn register(&mut self, path: &Path, kind: AssetKind) -> AssetId {
        let meta_path = Self::meta_path_for(path);

        let (id, import_settings, dependencies) = match std::fs::read_to_string(&meta_path) {
            Ok(text) => match ron::from_str::<AssetMetaFile>(&text) {
                Ok(f) => (f.id, f.import_settings, f.dependencies),
                Err(e) => {
                    log::warn!(
                        "AssetDatabase: bad .meta {}: {}. Regenerating.",
                        meta_path.display(),
                        e
                    );
                    self.write_fresh_meta(path, &meta_path, kind)
                }
            },
            Err(_) => self.write_fresh_meta(path, &meta_path, kind),
        };

        let content_hash = Self::hash_file(path).unwrap_or(0);

        let meta = AssetMeta {
            id,
            path: path.to_path_buf(),
            kind,
            content_hash,
            import_settings,
            dependencies,
        };

        self.by_id.insert(id, meta);
        self.by_path.insert(path.to_path_buf(), id);
        id
    }

    fn write_fresh_meta(
        &self,
        path: &Path,
        meta_path: &Path,
        kind: AssetKind,
    ) -> (AssetId, ImportSettings, Vec<AssetId>) {
        let id = AssetId::new_random();
        let settings = Self::default_settings_for(&kind, path);

        let file = AssetMetaFile {
            id,
            kind,
            import_settings: settings.clone(),
            dependencies: Vec::new(),
        };

        if let Ok(text) = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default()) {
            if let Err(e) = std::fs::write(meta_path, text) {
                log::warn!(
                    "AssetDatabase: cannot write .meta {}: {}",
                    meta_path.display(),
                    e
                );
            }
        }

        (id, settings, Vec::new())
    }

    fn default_settings_for(kind: &AssetKind, path: &Path) -> ImportSettings {
        let mut s = ImportSettings::default();
        if matches!(kind, AssetKind::Texture) {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("")
                .to_lowercase();
            if name.contains("normal")
                || name.contains("_nrm")
                || name.contains("metallic")
                || name.contains("roughness")
                || name.contains("_mr")
                || name.contains("ao")
                || name.contains("height")
                || name.contains("displacement")
                || name.contains("mask")
            {
                s.srgb = false;
            }
        }
        s
    }

    pub fn meta_path_for(path: &Path) -> PathBuf {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("asset");
        path.with_file_name(format!("{}.meta", name))
    }

    fn hash_file(path: &Path) -> std::io::Result<u64> {
        let bytes = std::fs::read(path)?;
        let mut h = std::collections::hash_map::DefaultHasher::new();
        h.write_u64(bytes.len() as u64);
        // Быстрая свёртка: первые 4KB + последние 4KB + размер.
        // Для детекта изменений этого хватает и не читает весь файл
        // для больших ассетов.
        let head_len = bytes.len().min(4096);
        h.write(&bytes[..head_len]);
        if bytes.len() > 8192 {
            let tail_start = bytes.len() - 4096;
            h.write(&bytes[tail_start..]);
        }
        Ok(h.finish())
    }

    // ============================================================
    // Lookup
    // ============================================================

    pub fn get(&self, id: AssetId) -> Option<&AssetMeta> {
        self.by_id.get(&id)
    }

    pub fn get_mut(&mut self, id: AssetId) -> Option<&mut AssetMeta> {
        self.by_id.get_mut(&id)
    }

    pub fn get_by_path(&self, path: &Path) -> Option<&AssetMeta> {
        self.by_path.get(path).and_then(|id| self.by_id.get(id))
    }

    pub fn id_of_path(&self, path: &Path) -> Option<AssetId> {
        self.by_path.get(path).copied()
    }

    pub fn path_of(&self, id: AssetId) -> Option<&Path> {
        self.by_id.get(&id).map(|m| m.path.as_path())
    }

    // ============================================================
    // Итерация
    // ============================================================

    pub fn iter(&self) -> impl Iterator<Item = &AssetMeta> {
        self.by_id.values()
    }

    pub fn of_kind(&self, kind: AssetKind) -> impl Iterator<Item = &AssetMeta> {
        self.by_id.values().filter(move |m| m.kind == kind)
    }

    pub fn len(&self) -> usize {
        self.by_id.len()
    }

    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }

    /// Все имена текстур (stem файлов) — для ComboBox в инспекторе.
    pub fn texture_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.of_kind(AssetKind::Texture).map(|m| m.stem()).collect();
        v.sort();
        v
    }

    // ============================================================
    // Hot-reload
    // ============================================================

    /// Проверить изменения. Возвращает ID ассетов, чьи файлы изменились.
    pub fn detect_changes(&mut self) -> Vec<AssetId> {
        let mut changed = Vec::new();

        let paths: Vec<(AssetId, PathBuf, u64)> = self
            .by_id
            .values()
            .map(|m| (m.id, m.path.clone(), m.content_hash))
            .collect();

        for (id, path, old_hash) in paths {
            if !path.exists() {
                continue;
            }
            let Ok(new_hash) = Self::hash_file(&path) else {
                continue;
            };
            if new_hash != old_hash {
                if let Some(meta) = self.by_id.get_mut(&id) {
                    meta.content_hash = new_hash;
                }
                changed.push(id);
            }
        }

        if !changed.is_empty() {
            self.revision += 1;
            log::debug!("AssetDatabase: {} assets changed", changed.len());
        }

        changed
    }

    /// Обновить настройки импорта (сохраняет `.meta`).
    pub fn update_import_settings(
        &mut self,
        id: AssetId,
        settings: ImportSettings,
    ) -> Result<()> {
        let Some(meta) = self.by_id.get_mut(&id) else {
            anyhow::bail!("asset {} not found", id);
        };

        meta.import_settings = settings.clone();

        let file = AssetMetaFile {
            id,
            kind: meta.kind,
            import_settings: settings,
            dependencies: meta.dependencies.clone(),
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())?;
        let meta_path = Self::meta_path_for(&meta.path);
        std::fs::write(&meta_path, text)?;
        self.revision += 1;
        Ok(())
    }

    /// Записать зависимости ассета.
    pub fn set_dependencies(&mut self, id: AssetId, deps: Vec<AssetId>) -> Result<()> {
        let Some(meta) = self.by_id.get_mut(&id) else {
            anyhow::bail!("asset {} not found", id);
        };
        meta.dependencies = deps.clone();

        let file = AssetMetaFile {
            id,
            kind: meta.kind,
            import_settings: meta.import_settings.clone(),
            dependencies: deps,
        };
        let text = ron::ser::to_string_pretty(&file, ron::ser::PrettyConfig::default())?;
        std::fs::write(Self::meta_path_for(&meta.path), text)?;
        Ok(())
    }
}