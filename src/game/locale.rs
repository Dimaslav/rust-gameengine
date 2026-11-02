//! Локализация: словари `key → text`, загрузка из RON, глобальный `t!()`.

use std::collections::HashMap;
use std::path::Path;
use std::sync::RwLock;

use anyhow::{Context, Result};

#[derive(Debug, Default, Clone)]
pub struct Locale {
    pub id: String,
    entries: HashMap<String, String>,
}

impl Locale {
    pub fn load_from_file(path: impl AsRef<Path>) -> Result<Self> {
        #[derive(serde::Deserialize)]
        struct File {
            id: String,
            #[serde(default)]
            entries: HashMap<String, String>,
        }
        let text = std::fs::read_to_string(path.as_ref())
            .with_context(|| format!("read locale {}", path.as_ref().display()))?;
        let file: File = ron::from_str(&text)
            .with_context(|| format!("parse locale {}", path.as_ref().display()))?;
        Ok(Self { id: file.id, entries: file.entries })
    }

    /// Лайфтайм `key` привязан к лайфтайму `self` — это позволяет
    /// вернуть `&'static str` при вызове от `&'static Locale`.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.entries.get(key).map(|s| s.as_str()).unwrap_or(key)
    }

    pub fn get_fmt(&self, key: &str, args: &[(&str, &str)]) -> String {
        let mut s = self.get(key).to_string();
        for (k, v) in args {
            s = s.replace(&format!("{{{}}}", k), v);
        }
        s
    }

    pub fn len(&self) -> usize { self.entries.len() }
    pub fn is_empty(&self) -> bool { self.entries.is_empty() }
}

// ============================================================
// Глобальный singleton (заменяемый)
// ============================================================

/// Каждый загруженный `Locale` утекает в кучу (`Box::leak`), чтобы
/// ссылки на него имели `'static`. При смене языка старый остаётся
/// жить — это нормально, `tr()` может вернуть строку из старого словаря,
/// и она всё равно валидна. Утечка — несколько килобайт на язык.
static CURRENT: RwLock<Option<&'static Locale>> = RwLock::new(None);

/// Загрузить язык. Возвращает `true`, если язык сменился.
pub fn set_language(id: &str) -> bool {
    let path = format!("assets/locale/{}.ron", id);
    match Locale::load_from_file(&path) {
        Ok(loc) => {
            let leaked: &'static Locale = Box::leak(Box::new(loc));
            *CURRENT.write().unwrap() = Some(leaked);
            log::info!("locale: loaded '{}'", id);
            true
        }
        Err(e) => {
            log::error!("locale: failed to load '{}': {:#}", path, e);
            false
        }
    }
}

pub fn current_id() -> Option<String> {
    CURRENT.read().unwrap().map(|l| l.id.clone())
}

/// Перевод по ключу. Если язык не загружен или ключа нет — возвращает
/// сам ключ (это удобно для дебага: видно, что забыли добавить).
pub fn tr(key: &'static str) -> &'static str {
    let guard = CURRENT.read().unwrap();
    match *guard {
        Some(loc) => loc.get(key),
        None => key,
    }
}

// ============================================================
// Макрос
// ============================================================

#[macro_export]
macro_rules! t {
    ($key:literal) => {
        $crate::game::locale::tr($key)
    };
    ($key:literal, $($k:ident = $v:expr),+ $(,)?) => {{
        let mut s = $crate::game::locale::tr($key).to_string();
        $(
            s = s.replace(&format!("{{{}}}", stringify!($k)), &$v.to_string());
        )+
        s
    }};
}