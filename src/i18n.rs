//! Internationalization (i18n) module.
//! Loads translation strings from TOML files at startup.
//! Language is configured in config.toml `general.language` field.
//! Changing language requires restart.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::OnceLock;

use directories::BaseDirs;

/// Global translation store, initialized once at startup.
static TRANSLATIONS: OnceLock<Translations> = OnceLock::new();

pub struct Translations {
    strings: HashMap<String, String>,
}

impl Translations {
    /// Load translations for the given language code (e.g., "en", "ko").
    /// Looks for files in:
    /// 1. ~/.config/tasty/lang/{code}.toml (user override)
    /// 2. Built-in defaults (embedded in binary)
    fn load(language: &str) -> Self {
        let mut strings = HashMap::new();

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
        if let Some(user_path) = Self::user_lang_path(language) {
            if let Ok(content) = std::fs::read_to_string(&user_path) {
                Self::parse_toml_into(&mut strings, &content);
                tracing::info!("loaded user translations from {}", user_path.display());
            }
        }

        tracing::info!(
            "i18n: loaded {} strings for language '{}'",
            strings.len(),
            language
        );

        Self { strings }
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
        BaseDirs::new().map(|dirs| {
            dirs.config_dir()
                .join("tasty")
                .join("lang")
                .join(format!("{}.toml", language))
        })
    }

    /// Get a translated string by key. Falls back to the key itself if not found.
    pub fn get<'a>(&'a self, key: &'a str) -> &'a str {
        self.strings.get(key).map(|s| s.as_str()).unwrap_or(key)
    }

    /// Get a translated string with a format argument replacing `{}`.
    pub fn get_fmt(&self, key: &str, arg: &str) -> String {
        let template = self.get(key);
        template.replace("{}", arg)
    }

}

/// Initialize the global translation store. Call once at startup.
pub fn init(language: &str) {
    let _ = TRANSLATIONS.set(Translations::load(language));
}

/// Get a translated string by key.
/// Shorthand for accessing the global store.
pub fn t(key: &str) -> &str {
    TRANSLATIONS.get().map(|tr| tr.get(key)).unwrap_or(key)
}

/// Get a translated string with a format argument.
pub fn t_fmt(key: &str, arg: &str) -> String {
    TRANSLATIONS
        .get()
        .map(|tr| tr.get_fmt(key, arg))
        .unwrap_or_else(|| key.replace("{}", arg))
}

