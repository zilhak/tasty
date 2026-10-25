//! Instance-independent Claude mod assets and their settings path-list entry.
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const MARKER: &str = "tasty-notify-managed-v1\n";
const ENV: &str = "CLAUDE_CODE_PLUGIN_DIRS";
const ASSETS: &[(&str, &str)] = &[
    (
        ".claude-plugin/plugin.json",
        include_str!("../mod/.claude-plugin/plugin.json"),
    ),
    ("hooks/hooks.json", include_str!("../mod/hooks/hooks.json")),
    (
        "hooks/register.ts",
        include_str!("../mod/hooks/register.ts"),
    ),
    ("hooks/reader.ts", include_str!("../mod/hooks/reader.ts")),
    (
        "hooks/messages.ts",
        include_str!("../mod/hooks/messages.ts"),
    ),
    ("types/index.d.ts", include_str!("../mod/types/index.d.ts")),
];

pub(crate) fn directory(settings: &Path) -> Result<PathBuf> {
    Ok(settings
        .parent()
        .context("notify_mod.settings_parent")?
        .join("tasty-managed")
        .join("notify-mod"))
}

fn separator() -> char {
    if cfg!(windows) { ';' } else { ':' }
}

/// Validate before touching files. Never coerce a user's non-string value.
pub(crate) fn configure(root: &mut Value, dir: &Path, sep: char, install: bool) -> Result<bool> {
    let path = dir.to_str().context("notify_mod.non_utf8_path")?;
    if path.contains(sep) {
        bail!("notify_mod.path_separator");
    }
    let obj = root
        .as_object_mut()
        .context("notify_mod.settings_root_type")?;
    if !install && !obj.contains_key("env") {
        return Ok(false);
    }
    let env = obj
        .entry("env")
        .or_insert_with(|| json!({}))
        .as_object_mut()
        .context("notify_mod.settings_env_type")?;
    let old = match env.get(ENV) {
        None => "",
        Some(value) => value.as_str().context("notify_mod.plugin_dirs_type")?,
    };
    // Retain other entries verbatim and in order, including empty path-list fields.
    let mut parts: Vec<&str> = if old.is_empty() {
        vec![]
    } else {
        old.split(sep).collect()
    };
    let before = parts.clone();
    parts.retain(|part| *part != path);
    if install {
        parts.push(path);
    }
    if before == parts {
        return Ok(false);
    }
    if parts.is_empty() {
        env.remove(ENV);
    } else {
        env.insert(ENV.into(), json!(parts.join(&sep.to_string())));
    }
    if env.is_empty() {
        obj.remove("env");
    }
    Ok(true)
}

/// Refuse symlink traversal, including parents. Unknown files are never removed.
fn check_path(path: &Path) -> Result<()> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(meta) if meta.file_type().is_symlink() => {
                bail!("notify_mod.symlink: {}", ancestor.display())
            }
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}

fn check_owner(dir: &Path) -> Result<()> {
    check_path(&dir.join(".tasty-owner"))?;
    if dir.exists() {
        let marker = std::fs::read_to_string(dir.join(".tasty-owner"))
            .context("notify_mod.owner_missing")?;
        if marker != MARKER {
            bail!("notify_mod.owner_mismatch");
        }
    }
    Ok(())
}

pub(crate) fn install(root: &mut Value, settings: &Path) -> Result<bool> {
    let dir = directory(settings)?;
    check_owner(&dir)?;
    let mut changed = configure(root, &dir, separator(), true)?;
    // Preflight every target so a known refusal does not leave half an asset set.
    check_path(settings)?;
    for (name, _) in ASSETS {
        check_path(&dir.join(name))?;
    }
    if !dir.exists() {
        std::fs::create_dir_all(&dir)?;
        std::fs::write(dir.join(".tasty-owner"), MARKER)?;
        changed = true;
    }
    for (name, content) in ASSETS {
        let path = dir.join(name);
        match std::fs::read(&path) {
            Ok(bytes) if bytes == content.as_bytes() => continue,
            Ok(_) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
        std::fs::create_dir_all(path.parent().context("notify_mod.asset_parent")?)?;
        std::fs::write(path, content)?;
        changed = true;
    }
    Ok(changed)
}

pub(crate) fn uninstall(root: &mut Value, settings: &Path) -> Result<bool> {
    let dir = directory(settings)?;
    check_owner(&dir)?;
    let mut changed = configure(root, &dir, separator(), false)?;
    check_path(settings)?;
    for (name, _) in ASSETS {
        check_path(&dir.join(name))?;
    }
    for (name, _) in ASSETS {
        match std::fs::remove_file(dir.join(name)) {
            Ok(()) => changed = true,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    // Claude may generate types here. Do not recursively delete those or user files.
    for name in [".claude-plugin", "hooks", "types"] {
        match std::fs::remove_dir(dir.join(name)) {
            Ok(()) => {}
            Err(e)
                if matches!(
                    e.kind(),
                    std::io::ErrorKind::NotFound | std::io::ErrorKind::DirectoryNotEmpty
                ) => {}
            Err(e) => return Err(e.into()),
        }
    }
    if dir.exists()
        && std::fs::read_dir(&dir)?
            .all(|entry| entry.is_ok_and(|entry| entry.file_name() == ".tasty-owner"))
    {
        std::fs::remove_file(dir.join(".tasty-owner"))?;
        std::fs::remove_dir(&dir)?;
    }
    Ok(changed)
}

#[cfg(test)]
mod tests;
