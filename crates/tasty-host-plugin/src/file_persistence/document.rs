use serde::{Serialize, de::DeserializeOwned};
use std::{
    io,
    path::{Path, PathBuf},
    sync::Mutex,
};
use tasty_utils::file_replace::{self, Fingerprint, Options};

pub(crate) struct Origin {
    pub(crate) path: PathBuf,
    baseline: Mutex<Result<toml::Table, String>>,
}
impl Clone for Origin {
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            baseline: Mutex::new(self.baseline.lock().unwrap().clone()),
        }
    }
}
impl std::fmt::Debug for Origin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ObservedPluginFile")
    }
}
impl Origin {
    pub(crate) fn new(path: PathBuf, baseline: Result<toml::Table, String>) -> Self {
        Self {
            path,
            baseline: Mutex::new(baseline),
        }
    }
    pub(crate) fn baseline(&self) -> io::Result<toml::Table> {
        self.baseline
            .lock()
            .unwrap()
            .clone()
            .map_err(io::Error::other)
    }
    pub(crate) fn advance(&self, table: toml::Table) -> io::Result<()> {
        *self
            .baseline
            .lock()
            .map_err(|_| io::Error::other("plugin file observation poisoned"))? = Ok(table);
        Ok(())
    }
}
pub(crate) fn encode(value: &impl Serialize) -> io::Result<toml::Table> {
    toml::Table::try_from(value).map_err(|_| io::Error::other("plugin file serialization failed"))
}
pub(crate) fn decode<T: DeserializeOwned>(value: &toml::Table) -> io::Result<T> {
    toml::Value::Table(value.clone())
        .try_into()
        .map_err(|_| io::Error::other("plugin file parse failed"))
}
pub(crate) fn read(path: &Path) -> io::Result<toml::Table> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(toml::Table::new()),
        Err(e) => return Err(io::Error::new(e.kind(), "plugin file read failed")),
    };
    let text =
        std::str::from_utf8(&bytes).map_err(|_| io::Error::other("plugin file is not UTF-8"))?;
    toml::from_str(text).map_err(|_| io::Error::other("plugin file parse failed"))
}
pub(crate) fn mutate<T>(
    path: &Path,
    edit: impl FnOnce(&mut toml::Table) -> io::Result<T>,
) -> io::Result<T> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let target = resolved_target(path)?;
    let _guard = file_replace::lock(path)?;
    if target != resolved_target(path)? {
        return Err(io::Error::other(
            "plugin file target changed; reload before saving",
        ));
    }
    let original = Fingerprint::read(path)?;
    let mut table = read(path)?;
    let before = table.clone();
    let output = edit(&mut table)?;
    if target != resolved_target(path)? || original != Fingerprint::read(path)? {
        return Err(io::Error::other(
            "plugin file changed outside this update; reload before saving",
        ));
    }
    if table == before {
        return Ok(output);
    }
    let original_text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => String::new(),
        Err(error) => return Err(io::Error::new(error.kind(), "plugin file read failed")),
    };
    let bytes = super::formatted::merge(&original_text, &before, &table)?;
    if target != resolved_target(path)? || original != Fingerprint::read(path)? {
        return Err(io::Error::other(
            "plugin file changed outside this update; reload before saving",
        ));
    }
    file_replace::replace_with(
        path,
        bytes.as_bytes(),
        Options {
            new_mode: Some(0o600),
            sync: true,
        },
    )?;
    Ok(output)
}
fn resolved_target(path: &Path) -> io::Result<PathBuf> {
    let target = file_replace::resolve_link(path)?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    Ok(parent.canonicalize()?.join(
        target
            .file_name()
            .ok_or_else(|| io::Error::other("plugin file has no name"))?,
    ))
}

pub(crate) fn at<'a>(table: &'a toml::Table, path: &[&str]) -> Option<&'a toml::Value> {
    let (first, rest) = path.split_first()?;
    let value = table.get(*first)?;
    if rest.is_empty() {
        Some(value)
    } else {
        at(value.as_table()?, rest)
    }
}
pub(crate) fn patch(
    table: &mut toml::Table,
    path: &[&str],
    before: Option<&toml::Value>,
    after: Option<&toml::Value>,
) -> io::Result<()> {
    if before == after {
        return Ok(());
    }
    let current = at(table, path);
    if current != before && current != after {
        return Err(io::Error::other(
            "plugin file field changed; reload before saving",
        ));
    }
    let (last, parents) = path
        .split_last()
        .ok_or_else(|| io::Error::other("empty plugin field"))?;
    let mut destination = table;
    for parent in parents {
        destination = destination
            .entry((*parent).to_owned())
            .or_insert_with(|| toml::Value::Table(Default::default()))
            .as_table_mut()
            .ok_or_else(|| io::Error::other("plugin file section is not a table"))?;
    }
    match after {
        Some(value) => {
            destination.insert((*last).to_owned(), value.clone());
        }
        None => {
            destination.remove(*last);
        }
    }
    Ok(())
}
pub(crate) fn patch_maps(
    table: &mut toml::Table,
    before: &toml::Table,
    after: &toml::Table,
    roots: &[&str],
) -> io::Result<()> {
    for root in roots {
        let empty = toml::Table::new();
        let a = before
            .get(*root)
            .and_then(toml::Value::as_table)
            .unwrap_or(&empty);
        let b = after
            .get(*root)
            .and_then(toml::Value::as_table)
            .unwrap_or(&empty);
        let keys: std::collections::BTreeSet<_> = a.keys().chain(b.keys()).collect();
        for key in keys {
            patch_trust_entry(table, root, key, a.get(key), b.get(key))?;
        }
    }
    Ok(())
}

fn normalized_entry(root: &str, entry: Option<&toml::Value>) -> io::Result<Option<toml::Value>> {
    entry
        .map(|entry| {
            let table = entry
                .as_table()
                .ok_or_else(|| io::Error::other("plugin trust record is not a table"))?;
            let normalized = match root {
                "plugins" => encode(&decode::<crate::known_plugins::KnownPluginEntry>(table)?)?,
                "unverified" => encode(&decode::<crate::known_plugins::UnverifiedChoice>(table)?)?,
                _ => return Err(io::Error::other("unknown plugin trust section")),
            };
            Ok(toml::Value::Table(normalized))
        })
        .transpose()
}
fn patch_trust_entry(
    table: &mut toml::Table,
    root: &str,
    key: &str,
    before: Option<&toml::Value>,
    after: Option<&toml::Value>,
) -> io::Result<()> {
    if before == after {
        return Ok(());
    }
    let path = [root, key];
    let raw = at(table, &path).cloned();
    let current = normalized_entry(root, raw.as_ref())?;
    if current.as_ref() != before && current.as_ref() != after {
        return Err(io::Error::other(
            "plugin trust record changed; reload before saving",
        ));
    }
    let merged = after.map(|after| {
        let mut fields = raw
            .as_ref()
            .and_then(toml::Value::as_table)
            .cloned()
            .unwrap_or_default();
        if let Some(before) = before.and_then(toml::Value::as_table) {
            for field in before.keys() {
                fields.remove(field);
            }
        }
        if let Some(after) = after.as_table() {
            fields.extend(after.clone());
        }
        toml::Value::Table(fields)
    });
    patch(table, &path, raw.as_ref(), merged.as_ref())
}
