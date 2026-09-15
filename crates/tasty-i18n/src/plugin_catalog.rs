//! Shared host/SDK plugin catalog loading. Only the selected plugin's user file
//! is read; installed assets remain immutable across upgrades.

use std::collections::HashMap;
use std::path::{Component, Path, PathBuf};

/// User override for an installed plugin. `user_lang_dir` is the host's `lang`
/// directory, not the plugin's data directory. Missing roots never fall back to
/// the process working directory or the SDK process's own home.
pub fn override_path(user_lang_dir: &Path, plugin_id: &str, locale: &str) -> Option<PathBuf> {
    if !crate::is_valid_code(locale)
        || !tasty_utils::plugin_id::is_valid_plugin_id(plugin_id)
        || !matches!(
            Path::new(plugin_id).components().next(),
            Some(Component::Normal(_))
        )
    {
        return None;
    }
    Some(if crate::is_builtin_code(locale) {
        user_lang_dir
            .join("plugins")
            .join(plugin_id)
            .join(format!("{locale}.toml"))
    } else {
        user_lang_dir
            .join(locale)
            .join("plugins")
            .join(format!("{plugin_id}.toml"))
    })
}

/// Installed English → installed active locale → user's active-locale override.
/// Blank overlay values mean "keep the preceding layer". Invalid/oversized user
/// files are ignored with a warning, using the same bound as host language packs.
pub fn load(
    lang_dir: &Path,
    locale: &str,
    plugin_id: &str,
    user_lang_dir: Option<&Path>,
) -> HashMap<String, String> {
    let mut strings = HashMap::new();
    overlay(&mut strings, &lang_dir.join("en.toml"), false, false);
    if locale != "en" && crate::is_valid_code(locale) {
        overlay(
            &mut strings,
            &lang_dir.join(format!("{locale}.toml")),
            true,
            false,
        );
    }
    if let Some(path) = user_lang_dir.and_then(|root| override_path(root, plugin_id, locale)) {
        overlay(&mut strings, &path, true, true);
    }
    strings
}

fn overlay(strings: &mut HashMap<String, String>, path: &Path, drop_blank: bool, user: bool) {
    let value = if user {
        match crate::read_user_toml(path) {
            Ok(Some(value)) => value,
            Ok(None) => return,
            Err(error) => {
                tracing::warn!(
                    "plugin translation override {} ignored: {error}",
                    path.display()
                );
                return;
            }
        }
    } else {
        // Installed files retain their existing unbounded read policy. User
        // files use the bounded language-pack reader above.
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        let Ok(value) = text.parse::<toml::Value>() else {
            return;
        };
        value
    };
    let mut entries = HashMap::new();
    crate::flatten_catalog_toml("", &value, &mut entries);
    if drop_blank {
        crate::drop_blank_values_warned(
            &mut entries,
            &format!("plugin translations {}", path.display()),
            "keep the preceding translation layer",
        );
    }
    strings.extend(entries);
}
