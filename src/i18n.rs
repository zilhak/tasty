#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]
//! Internationalization (i18n) module.
//! Loads translation strings from TOML files at startup.
//! Language is configured in config.toml `general.language` field.
//! Changing language requires restart.
//!
//! Plugins can dynamically register/unregister translation namespaces via
//! [`register_namespace`] / [`unregister_namespace`]. Namespace strings are
//! `Box::leak`ed to satisfy `&'static str` lookup contract — the per-plugin
//! string set is small and bounded so the leak is acceptable.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use tasty_utils::path::tasty_home;

/// Global translation store, initialized once at startup.
static TRANSLATIONS: OnceLock<Translations> = OnceLock::new();

pub struct Translations {
    /// Built-in + user override strings. Frozen after `init`.
    base: HashMap<String, &'static str>,
    /// Per-plugin namespace overlays. Looked up after `base` misses.
    /// Iteration order is not stable; collisions across namespaces resolve
    /// to whichever shows up first in the iteration — plugins should prefix
    /// their keys with their plugin id to avoid collisions in practice.
    namespaces: RwLock<HashMap<String, HashMap<String, &'static str>>>,
    /// Active language code; used by `register_namespace` to decide which
    /// language file to load from a plugin's lang dir.
    language: String,
}

impl Translations {
    /// Load translations for the given language code (e.g., "en", "ko").
    /// Looks for files in:
    /// 1. ~/.tasty/lang/{code}.toml (user override)
    /// 2. Built-in defaults (embedded in binary)
    fn load(language: &str) -> Self {
        let mut strings: HashMap<String, String> = HashMap::new();

        // Start with built-in English as base (always available)
        let en_toml = include_str!("../lang/en.toml");
        Self::parse_toml_into(&mut strings, en_toml);

        // If not English, overlay the requested language from built-in
        if language != "en" {
            let builtin = match language {
                "ko" => Some(include_str!("../lang/ko.toml")),
                "ja" => Some(include_str!("../lang/ja.toml")),
                _ => None,
            };
            if let Some(toml_str) = builtin {
                Self::parse_toml_into(&mut strings, toml_str);
            }
        }

        // Overlay user's custom translation file if it exists
        if let Some(user_path) = Self::user_lang_path(language)
            && let Ok(content) = std::fs::read_to_string(&user_path)
        {
            Self::parse_toml_into(&mut strings, &content);
            tracing::info!("loaded user translations from {}", user_path.display());
        }

        tracing::info!(
            "i18n: loaded {} strings for language '{}'",
            strings.len(),
            language
        );

        let base: HashMap<String, &'static str> =
            strings.into_iter().map(|(k, v)| (k, leak_str(v))).collect();

        Self {
            base,
            namespaces: RwLock::new(HashMap::new()),
            language: language.to_string(),
        }
    }

    /// Parse a TOML string with nested tables into flat dotted keys.
    /// e.g., [settings.tab] general = "General" -> "settings.tab.general" = "General"
    fn parse_toml_into(map: &mut HashMap<String, String>, toml_str: &str) {
        if let Ok(value) = toml_str.parse::<toml::Value>() {
            Self::flatten_toml("", &value, map);
        }
    }

    fn flatten_toml(prefix: &str, value: &toml::Value, map: &mut HashMap<String, String>) {
        match value {
            toml::Value::Table(table) => {
                for (key, val) in table {
                    let full_key = if prefix.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", prefix, key)
                    };
                    Self::flatten_toml(&full_key, val, map);
                }
            }
            toml::Value::String(s) => {
                map.insert(prefix.to_string(), s.clone());
            }
            // Ignore non-string leaf values
            _ => {}
        }
    }

    fn user_lang_path(language: &str) -> Option<PathBuf> {
        tasty_home().map(|dir| dir.join("lang").join(format!("{}.toml", language)))
    }

    /// Get a translated string by key. Falls back to the key itself if not found.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        if let Some(s) = self.base.get(key) {
            return s;
        }
        if let Ok(ns) = self.namespaces.read() {
            for map in ns.values() {
                if let Some(s) = map.get(key) {
                    return s;
                }
            }
        }
        key
    }

    /// Get a translated string with a format argument replacing `{}`.
    pub fn get_fmt(&self, key: &str, arg: &str) -> String {
        let template = self.get(key);
        template.replace("{}", arg)
    }

    /// Replace `{}` with `arg1`, `arg2` in order (one occurrence each).
    /// Unlike `get_fmt`, this uses `replacen(_, 1)` so only the first two `{}` placeholders are replaced.
    pub fn get_fmt2(&self, key: &str, arg1: &str, arg2: &str) -> String {
        let template = self.get(key);
        let first = template.replacen("{}", arg1, 1);
        first.replacen("{}", arg2, 1)
    }

    /// Register a plugin namespace. `lang_dir` is expected to contain
    /// `<lang>.toml` files — `en.toml` is loaded as the base, then the
    /// active language file is overlaid on top.
    ///
    /// If `namespace` was previously registered, its entries are replaced.
    pub fn register_namespace(&self, namespace: &str, lang_dir: &Path) {
        let mut strings: HashMap<String, String> = HashMap::new();

        // English fallback first.
        let en_path = lang_dir.join("en.toml");
        if let Ok(s) = std::fs::read_to_string(&en_path) {
            Self::parse_toml_into(&mut strings, &s);
        }

        if self.language != "en" {
            let lang_path = lang_dir.join(format!("{}.toml", self.language));
            if let Ok(s) = std::fs::read_to_string(&lang_path) {
                Self::parse_toml_into(&mut strings, &s);
            }
        }

        let leaked: HashMap<String, &'static str> =
            strings.into_iter().map(|(k, v)| (k, leak_str(v))).collect();

        let count = leaked.len();
        if let Ok(mut ns) = self.namespaces.write() {
            ns.insert(namespace.to_string(), leaked);
        }
        tracing::info!(
            "i18n: registered namespace '{}' with {} strings (lang_dir={})",
            namespace,
            count,
            lang_dir.display()
        );
    }

    /// Remove a previously registered namespace. Strings remain in memory
    /// (`Box::leak`) but are no longer reachable through `get`.
    pub fn unregister_namespace(&self, namespace: &str) {
        if let Ok(mut ns) = self.namespaces.write() {
            ns.remove(namespace);
        }
        tracing::info!("i18n: unregistered namespace '{}'", namespace);
    }
}

fn leak_str(s: String) -> &'static str {
    Box::leak(s.into_boxed_str())
}

/// Initialize the global translation store. Call once at startup.
pub fn init(language: &str) {
    // OnceLock::set은 이미 set된 경우만 Err. i18n은 부팅 시 1회만 호출되는 시드
    // 데이터라 두 번째 호출은 의도적 no-op — Err를 panic이나 로그 없이 그대로 무시.
    let _already_set: Result<_, _> = TRANSLATIONS.set(Translations::load(language));
}

/// Get a translated string by key.
/// Shorthand for accessing the global store.
pub fn t(key: &str) -> &str {
    TRANSLATIONS.get().map(|tr| tr.get(key)).unwrap_or(key)
}

/// 활성 language code. 부팅 시 [`init`]에 전달된 값. 미초기화면 `"en"` fallback.
/// 호스트가 plugin spawn 시 `TASTY_LOCALE` 환경변수로 전달하는 등에 사용.
pub fn current_language() -> &'static str {
    TRANSLATIONS
        .get()
        .map(|tr| tr.language.as_str())
        .unwrap_or("en")
}

/// Get a translated string with a format argument.
pub fn t_fmt(key: &str, arg: &str) -> String {
    TRANSLATIONS
        .get()
        .map(|tr| tr.get_fmt(key, arg))
        .unwrap_or_else(|| key.replace("{}", arg))
}

/// Get a translated string with two format arguments replacing the first two `{}` placeholders in order.
pub fn t_fmt2(key: &str, arg1: &str, arg2: &str) -> String {
    TRANSLATIONS
        .get()
        .map(|tr| tr.get_fmt2(key, arg1, arg2))
        .unwrap_or_else(|| key.replacen("{}", arg1, 1).replacen("{}", arg2, 1))
}

/// Register a plugin's translation namespace. No-op if `init` has not been
/// called yet (translations not initialized).
pub fn register_namespace(namespace: &str, lang_dir: &Path) {
    if let Some(tr) = TRANSLATIONS.get() {
        tr.register_namespace(namespace, lang_dir);
    } else {
        tracing::warn!(
            "i18n: register_namespace('{}') called before init — ignored",
            namespace
        );
    }
}

/// Unregister a plugin's translation namespace.
pub fn unregister_namespace(namespace: &str) {
    if let Some(tr) = TRANSLATIONS.get() {
        tr.unregister_namespace(namespace);
    }
}

/// Plugin manager 가 의존하는 trait 의 본 바이너리 impl. boot wiring 에서
/// `Arc::new(BinI18nRegistrar)` 로 주입.
pub struct BinI18nRegistrar;

impl tasty_plugin_protocol::host_port::I18nNamespaceRegistrar for BinI18nRegistrar {
    fn register(&self, namespace: &str, lang_dir: &Path) {
        register_namespace(namespace, lang_dir);
    }
    fn unregister(&self, namespace: &str) {
        unregister_namespace(namespace);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn namespace_register_lookup() {
        // 직접 Translations 인스턴스로 테스트 (전역 OnceLock와 격리)
        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        // 임시 lang dir 없이 직접 namespace 삽입해 lookup만 확인
        {
            let mut ns = tr.namespaces.write().unwrap();
            let mut m: HashMap<String, &'static str> = HashMap::new();
            m.insert("plugin.x.title".to_string(), "Refresh");
            ns.insert("com.example.x".to_string(), m);
        }
        assert_eq!(tr.get("plugin.x.title"), "Refresh");
        assert_eq!(tr.get("missing.key"), "missing.key");
    }

    #[test]
    fn base_takes_precedence_over_namespace() {
        let mut base: HashMap<String, &'static str> = HashMap::new();
        base.insert("shared.key".to_string(), "FromBase");
        let tr = Translations {
            base,
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        {
            let mut ns = tr.namespaces.write().unwrap();
            let mut m: HashMap<String, &'static str> = HashMap::new();
            m.insert("shared.key".to_string(), "FromPlugin");
            ns.insert("com.example.x".to_string(), m);
        }
        assert_eq!(tr.get("shared.key"), "FromBase");
    }

    #[test]
    fn unregister_namespace_removes_strings() {
        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "en".to_string(),
        };
        {
            let mut ns = tr.namespaces.write().unwrap();
            let mut m: HashMap<String, &'static str> = HashMap::new();
            m.insert("plugin.x.title".to_string(), "Refresh");
            ns.insert("com.example.x".to_string(), m);
        }
        assert_eq!(tr.get("plugin.x.title"), "Refresh");
        tr.unregister_namespace("com.example.x");
        assert_eq!(tr.get("plugin.x.title"), "plugin.x.title");
    }

    #[test]
    fn register_namespace_from_lang_dir() {
        // 임시 디렉토리에 en.toml/ko.toml 만들어서 register_namespace 검증
        let tmp = std::env::temp_dir().join(format!(
            "tasty-i18n-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&tmp).unwrap();
        std::fs::write(
            tmp.join("en.toml"),
            "[plugin.x]\ntitle = \"Refresh\"\nbody = \"OnlyEn\"\n",
        )
        .unwrap();
        std::fs::write(tmp.join("ko.toml"), "[plugin.x]\ntitle = \"새로고침\"\n").unwrap();

        let tr = Translations {
            base: HashMap::new(),
            namespaces: RwLock::new(HashMap::new()),
            language: "ko".to_string(),
        };
        tr.register_namespace("com.example.x", &tmp);
        // ko에서 정의된 키
        assert_eq!(tr.get("plugin.x.title"), "새로고침");
        // en에만 있는 키는 fallback으로 노출
        assert_eq!(tr.get("plugin.x.body"), "OnlyEn");

        std::fs::remove_dir_all(&tmp).ok();
    }
}
