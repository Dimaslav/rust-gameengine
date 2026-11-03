//! Локализация: словари `key → text`, загрузка из RON, глобальный `t!()`.
//!
//! ## Изменения
//!
//! * `Locale` кэшируется по языку в `HashMap<String, &'static Locale>`.
//!   `set_language("ru")` дважды подряд не утекает — второй вызов
//!   переиспользует уже загруженный словарь.
//! * Добавлен `tr_dyn(&str) -> String` для динамических ключей
//!   (`format!("item.{}", id)`). `tr()` по-прежнему принимает
//!   `&'static str` и возвращает `&'static str` — это самый частый путь.

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
        Ok(Self {
            id: file.id,
            entries: file.entries,
        })
    }

    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.entries.get(key).map(|s| s.as_str()).unwrap_or(key)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

// ============================================================
// Глобальный singleton с кэшем по языку
// ============================================================

/// Кэш загруженных языков. Каждый `Locale` утекает в кучу один раз,
/// чтобы `tr()` мог вернуть `&'static str`. Повторный `set_language`
/// того же языка возвращает кэшированный экземпляр — без утечки.
static CACHE: RwLock<Option<HashMap<String, &'static Locale>>> = RwLock::new(None);
static CURRENT: RwLock<Option<&'static Locale>> = RwLock::new(None);

pub fn set_language(id: &str) -> bool {
    // Fast path: уже загружен.
    {
        let cache_guard = CACHE.read().unwrap();
        if let Some(cache) = cache_guard.as_ref()
            && let Some(&cached) = cache.get(id) {
                *CURRENT.write().unwrap() = Some(cached);
                return true;
            }
    }

    // Load.
    let path = format!("assets/locale/{}.ron", id);
    match Locale::load_from_file(&path) {
        Ok(loc) => {
            let leaked: &'static Locale = Box::leak(Box::new(loc));
            {
                let mut cache_guard = CACHE.write().unwrap();
                let cache = cache_guard.get_or_insert_with(HashMap::new);
                cache.insert(id.to_string(), leaked);
            }
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

/// Перевод по `'static` ключу. Если язык не загружен или ключа нет —
/// возвращает сам ключ (видно в UI, что забыли добавить).
pub fn tr(key: &'static str) -> &'static str {
    let guard = CURRENT.read().unwrap();
    match *guard {
        Some(loc) => loc.get(key),
        None => key,
    }
}

/// Перевод по динамическому ключу. Возвращает `String`.
/// Используйте для `format!("item.{}", id)` — `tr()` не примет
/// non-'static строку.
pub fn tr_dyn(key: &str) -> String {
    let guard = CURRENT.read().unwrap();
    match *guard {
        Some(loc) => loc.get(key).to_string(),
        None => key.to_string(),
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
    ($key:expr) => {
        $crate::game::locale::tr_dyn($key)
    };
    ($key:literal, $($k:ident = $v:expr),+ $(,)?) => {{
        let mut s = $crate::game::locale::tr($key).to_string();
        $(
            s = s.replace(&format!("{{{}}}", stringify!($k)), &$v.to_string());
        )+
        s
    }};
}