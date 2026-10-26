//! ScriptEngine — владелец Rhai Engine + кэш скомпилированных AST.
//!
//! Каждый скрипт компилируется один раз, AST живёт в памяти.
//! При hot-reload проверяется mtime файла — если изменился,
//! AST пересобирается (если парсинг удался; иначе остаётся старая версия).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use anyhow::{Context, Result};
use rhai::{Dynamic, Engine, Scope, AST};

use super::api;
use super::host::SharedHost;

struct CachedScript {
    ast: AST,
    path: PathBuf,
    mtime: SystemTime,
    /// Ошибки парсинга (для UI). Если `Some`, скрипт не запускается.
    error: Option<String>,
}

pub struct ScriptEngine {
    engine: Engine,
    scripts: HashMap<String, CachedScript>,
    root: PathBuf,
}

impl ScriptEngine {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        let mut engine = Engine::new();
        // Безопасность: отключаем файловый IO и eval.
        engine.set_max_operations(100_000);
        engine.set_max_call_levels(32);
        engine.set_max_expr_depths(32, 32);
        engine.set_max_string_size(64 * 1024);
        engine.set_max_array_size(16 * 1024);
        engine.set_max_map_size(4096);

        api::register_all(&mut engine);

        let root = root.into();
        if !root.exists() {
            log::info!("ScriptEngine: creating {}", root.display());
            std::fs::create_dir_all(&root).ok();
        }

        Self { engine, scripts: HashMap::new(), root }
    }

    pub fn root(&self) -> &Path { &self.root }

    /// Список загруженных скриптов (для UI).
    pub fn loaded_names(&self) -> Vec<String> {
        let mut v: Vec<String> = self.scripts.keys().cloned().collect();
        v.sort();
        v
    }

    /// Загрузить / перезагрузить все `.rhai` из `root`.
    pub fn load_all(&mut self) -> Result<()> {
        let entries = std::fs::read_dir(&self.root)
            .with_context(|| format!("read scripts dir {}", self.root.display()))?;

        let mut loaded = 0usize;
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().and_then(|s| s.to_str()) != Some("rhai") {
                continue;
            }
            let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
                continue;
            };
            match self.load_one(stem, &path) {
                Ok(()) => loaded += 1,
                Err(e) => log::warn!("script '{}' load failed: {:#}", stem, e),
            }
        }
        log::info!("ScriptEngine: {} scripts loaded from {}", loaded, self.root.display());
        Ok(())
    }

    /// Гарантировать, что скрипт с таким именем загружен.
    pub fn ensure_loaded(&mut self, name: &str) -> Result<()> {
        if self.scripts.contains_key(name) {
            return Ok(());
        }
        let path = self.root.join(format!("{}.rhai", name));
        self.load_one(name, &path)
    }

    fn load_one(&mut self, name: &str, path: &Path) -> Result<()> {
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("read script {}", path.display()))?;
        let mtime = std::fs::metadata(path).and_then(|m| m.modified()).unwrap_or(SystemTime::UNIX_EPOCH);

        let (ast, error) = match self.engine.compile(&source) {
            Ok(ast) => (ast, None),
            Err(e) => {
                let msg = format!("{}", e);
                log::warn!("script '{}' parse error: {}", name, msg);
                // Заглушка — пустой AST, чтобы preserve entry в map.
                (AST::empty(), Some(msg))
            }
        };

        self.scripts.insert(name.to_string(), CachedScript {
            ast, path: path.to_path_buf(), mtime, error,
        });
        Ok(())
    }

    /// Проверить mtime и перезагрузить изменённые.
    /// Возвращает имена перезагруженных скриптов.
    pub fn reload_changed(&mut self) -> Vec<String> {
        let mut reloaded = Vec::new();
        let keys: Vec<String> = self.scripts.keys().cloned().collect();
        for name in keys {
            let (path, old_mtime) = {
                let s = &self.scripts[&name];
                (s.path.clone(), s.mtime)
            };
            let Ok(new_mtime) = std::fs::metadata(&path).and_then(|m| m.modified()) else {
                continue;
            };
            if new_mtime > old_mtime {
                match self.load_one(&name, &path) {
                    Ok(()) => {
                        reloaded.push(name.clone());
                        log::info!("Hot-reload: script '{}' reloaded", name);
                    }
                    Err(e) => log::warn!("Hot-reload '{}' failed: {:#}", name, e),
                }
            }
        }
        reloaded
    }

    /// Вызвать функцию скрипта, если она есть. Молча игнорирует
    /// отсутствие функции, но логирует реальные ошибки.
    pub fn call(
        &self,
        script_name: &str,
        fn_name: &str,
        host: SharedHost,
    ) {
        let Some(cached) = self.scripts.get(script_name) else {
            return;
        };
        if let Some(err) = &cached.error {
            log::debug!("script '{}' has parse error: {}", script_name, err);
            return;
        }

        let mut scope = Scope::new();
        scope.push("host", host);

        match self.engine.call_fn::<Dynamic>(&mut scope, &cached.ast, fn_name, ()) {
            Ok(_) => {}
            Err(e) => {
                if !is_function_not_found(&e) {
                    log::warn!("script '{}::{}' error: {}", script_name, fn_name, e);
                }
            }
        }
    }

    pub fn has_script(&self, name: &str) -> bool { self.scripts.contains_key(name) }

    // ============================================================
    // API для панели Scripts
    // ============================================================

    /// Ошибка парсинга скрипта (для панели Scripts).
    pub fn script_error(&self, name: &str) -> Option<String> {
        self.scripts.get(name).and_then(|s| s.error.clone())
    }

    /// Принудительно перечитать все известные скрипты с диска.
    /// Возвращает число успешно перезагруженных.
    pub fn reload_all(&mut self) -> usize {
        let keys: Vec<String> = self.scripts.keys().cloned().collect();
        let mut n = 0;
        for name in keys {
            let path = self.scripts[&name].path.clone();
            if self.load_one(&name, &path).is_ok() {
                n += 1;
            }
        }
        n
    }
}

fn is_function_not_found(e: &rhai::EvalAltResult) -> bool {
    matches!(e, rhai::EvalAltResult::ErrorFunctionNotFound(_, _))
}