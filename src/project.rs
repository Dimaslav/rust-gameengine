use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectManifest {
    pub name: String,
    #[serde(default)]
    pub engine_version: String,
    #[serde(default = "default_assets")]
    pub assets_root: String,
    #[serde(default)]
    pub entry_scene: Option<String>,
}

fn default_assets() -> String {
    "assets".to_string()
}

impl ProjectManifest {
    pub fn load(path: &Path) -> Result<Self> {
        let text = std::fs::read_to_string(path)
            .with_context(|| format!("failed to read project manifest: {}", path.display()))?;
        let manifest: Self = ron::from_str(&text)
            .with_context(|| format!("failed to parse project manifest: {}", path.display()))?;
        Ok(manifest)
    }
}

impl Default for ProjectManifest {
    fn default() -> Self {
        Self {
            name: "Unnamed".to_string(),
            engine_version: env!("CARGO_PKG_VERSION").to_string(),
            assets_root: default_assets(),
            entry_scene: Some("scene.ron".to_string()),
        }
    }
}

#[derive(Debug, Clone)]
pub struct Project {

    pub root: PathBuf,
    pub manifest: ProjectManifest,
}

impl Project {
    /// Открыть проект из явного пути к `project.ron`.
    pub fn open(manifest_path: impl AsRef<Path>) -> Result<Self> {
        let manifest_path = manifest_path.as_ref();
        let manifest = ProjectManifest::load(manifest_path)?;
        let root = manifest_path
            .parent()
            .map(|p| {
                if p.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    p.to_path_buf()
                }
            })
            .unwrap_or_else(|| PathBuf::from("."));
        Ok(Self { root, manifest })
    }

    /// Fallback: CWD — корень, `assets_root = "assets"`.
    /// Соответствует поведению движка до появления `project.ron`.
    pub fn from_cwd() -> Self {
        Self {
            root: PathBuf::from("."),
            manifest: ProjectManifest::default(),
        }
    }

    /// `true`, если проект не найден и мы работаем в fallback-режиме.
    pub fn is_cwd(&self) -> bool {
        self.root == Path::new(".")
    }

    /// Автопоиск проекта по правилам из doc-комментария модуля.
    pub fn find_or_default() -> Result<Self> {
        // 1. --project <path>
        let args: Vec<String> = std::env::args().collect();
        let mut explicit: Option<PathBuf> = None;
        let mut i = 0;
        while i < args.len() {
            if args[i] == "--project" && i + 1 < args.len() {
                explicit = Some(PathBuf::from(&args[i + 1]));
                break;
            }
            i += 1;
        }
        if let Some(p) = explicit {
            // `--project` может указывать как на корень проекта,
            // так и на сам `project.ron`.
            let manifest_path = if p.is_file() {
                p
            } else {
                p.join("project.ron")
            };
            log::info!(
                "Project: using explicit --project path {}",
                manifest_path.display()
            );
            return Self::open(&manifest_path);
        }

        // 2. ./project.ron
        if Path::new("project.ron").exists() {
            log::info!("Project: using ./project.ron");
            return Self::open("project.ron");
        }

        // 3. ./projects/*/project.ron
        if let Ok(entries) = std::fs::read_dir("projects") {
            let candidates: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_dir() && p.join("project.ron").exists())
                .collect();
            if candidates.len() == 1 {
                log::info!(
                    "Project: auto-detected {}",
                    candidates[0].display()
                );
                return Self::open(candidates[0].join("project.ron"));
            }
            if candidates.len() > 1 {
                log::warn!(
                    "Project: multiple projects in `projects/` ({}). \
                     Use --project <path> to select one. Falling back to CWD.",
                    candidates.len()
                );
            }
        }

        // 4. Fallback.
        log::info!(
            "Project: no project.ron found; running with CWD as project root \
             (assets_root = \"assets\")."
        );
        Ok(Self::from_cwd())
    }

    /// `chdir` в корень проекта. Идемпотентно (для `from_cwd` — no-op).
    ///
    /// После успешного вызова все относительные пути
    /// (`assets/`, `saves/`, `editor.ron` и т.п.) разрешаются от корня.
    pub fn chdir(&self) -> Result<()> {
        if self.is_cwd() {
            return Ok(());
        }
        let abs = std::env::current_dir()
            .context("read current dir")?
            .join(&self.root);
        log::info!("Project: changing CWD to {}", abs.display());
        std::env::set_current_dir(&abs)
            .with_context(|| format!("chdir to {}", abs.display()))?;
        Ok(())
    }

    /// Абсолютный путь к папке с ассетами.
    /// **Внимание:** вычислять до `chdir()`. После `chdir` используй
    /// `manifest.assets_root` напрямую — он уже относителен корня.
    pub fn assets_path(&self) -> PathBuf {
        self.root.join(&self.manifest.assets_root)
    }

    /// Абсолютный путь к entry-scene (если задана).
    pub fn entry_scene_path(&self) -> Option<PathBuf> {
        self.manifest
            .entry_scene
            .as_ref()
            .map(|s| self.root.join(s))
    }

    /// Стандартные подпути проекта.
    pub fn saves_path(&self) -> PathBuf {
        self.root.join("saves")
    }
    pub fn prefabs_path(&self) -> PathBuf {
        self.root.join("prefabs")
    }
    pub fn scripts_path(&self) -> PathBuf {
        self.assets_path().join("scripts")
    }
    pub fn settings_path(&self) -> PathBuf {
        self.root.join("settings.ron")
    }
    pub fn editor_path(&self) -> PathBuf {
        self.root.join("editor.ron")
    }
    pub fn input_path(&self) -> PathBuf {
        self.root.join("input.ron")
    }
}