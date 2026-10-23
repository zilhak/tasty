//! Locked, lossless registry editing. No snapshot from a forgiving load is written here.
use anyhow::{Context, Result, bail};
use std::{
    fs,
    path::{Path, PathBuf},
};
use tasty_utils::file_replace::{self, Fingerprint, WriteLock};
use toml_edit::{ArrayOfTables, Document, Item, Table, value};

pub(crate) struct Registry {
    pub doc: Document,
    path: PathBuf,
    target: PathBuf,
    observed: Fingerprint,
    _guard: WriteLock,
}
impl Registry {
    pub fn open(path: PathBuf) -> Result<Self> {
        fs::create_dir_all(path.parent().context("registry has no parent")?)?;
        let target = file_replace::resolve_link(&path)?;
        let guard = file_replace::lock(&path)?;
        if file_replace::resolve_link(&path)? != target {
            bail!("registry symlink changed while locking");
        }
        let bytes = match fs::read(&target) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e).context("read registry"),
        };
        let observed = bytes
            .as_deref()
            .map(Fingerprint::of_bytes)
            .unwrap_or(Fingerprint::Missing);
        let doc = match bytes {
            Some(bytes) => {
                let text = std::str::from_utf8(&bytes)
                    .map_err(|_| anyhow::anyhow!("registry is not UTF-8"))?;
                // Parser diagnostics can contain source values; never propagate them.
                text.parse::<Document>()
                    .map_err(|_| anyhow::anyhow!("invalid registry TOML; original preserved"))?
            }
            None => Document::new(),
        };
        Ok(Self {
            doc,
            path,
            target,
            observed,
            _guard: guard,
        })
    }
    pub fn entries(&mut self, key: &str) -> Result<&mut ArrayOfTables> {
        if !self.doc.contains_key(key)
            || self
                .doc
                .get(key)
                .and_then(Item::as_array)
                .is_some_and(|a| a.is_empty())
        {
            self.doc[key] = Item::ArrayOfTables(ArrayOfTables::new());
        }
        self.doc[key]
            .as_array_of_tables_mut()
            .context("registry entries must be an array of tables")
    }
    pub fn find(&mut self, key: &str, name: &str) -> Result<Option<usize>> {
        let matches: Vec<_> = self
            .entries(key)?
            .iter()
            .enumerate()
            .filter(|(_, t)| t.get("name").and_then(Item::as_str) == Some(name))
            .map(|(i, _)| i)
            .collect();
        if matches.len() > 1 {
            bail!("ambiguous duplicate registry name");
        }
        Ok(matches.first().copied())
    }
    pub fn reject_alias(&self, other: &Path) -> Result<()> {
        if self.target.try_exists()?
            && other.try_exists()?
            && self.target.canonicalize()? == other.canonicalize()?
        {
            bail!("registry and credential paths alias the same lock; mutation refused");
        }
        Ok(())
    }
    pub fn commit(&mut self) -> Result<()> {
        if file_replace::resolve_link(&self.path)? != self.target
            || Fingerprint::read(&self.target)? != self.observed
        {
            bail!("registry changed during mutation; external edit preserved");
        }
        if !self.doc.contains_key("version") {
            self.doc["version"] = value(1);
        }
        let contents = self.doc.to_string();
        contents
            .parse::<Document>()
            .map_err(|_| anyhow::anyhow!("mutation produced invalid TOML; original preserved"))?;
        file_replace::replace_with(
            &self.target,
            contents.as_bytes(),
            file_replace::Options {
                new_mode: Some(0o600),
                sync: true,
            },
        )?;
        Ok(())
    }
}
pub(crate) fn table_of<T: serde::Serialize>(entry: &T) -> Result<Table> {
    let text = toml::to_string(entry)?;
    let document: Document = text.parse().context("serialize registry entry")?;
    let mut table = document.as_table().clone();
    struct Append;
    impl toml_edit::visit_mut::VisitMut for Append {
        fn visit_table_mut(&mut self, table: &mut Table) {
            table.set_position(usize::MAX);
            toml_edit::visit_mut::visit_table_mut(self, table);
        }
    }
    toml_edit::visit_mut::VisitMut::visit_table_mut(&mut Append, &mut table);
    Ok(table)
}
pub(crate) fn typed<T: serde::de::DeserializeOwned>(table: &Table) -> Result<T> {
    toml::from_str(&Document::from(table.clone()).to_string())
        .map_err(|_| anyhow::anyhow!("invalid target entry; original preserved"))
}
pub(crate) fn managed_path(dir: &Path, name: &str, path: &Path) -> bool {
    crate::is_valid_passkey_name(name)
        && fs::symlink_metadata(dir).is_ok_and(|m| m.is_dir() && !m.file_type().is_symlink())
        && path == dir.join(name)
        && fs::symlink_metadata(path).is_ok_and(|m| !m.file_type().is_symlink())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn external_edit_after_read_rejects_replace_and_keeps_original_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("registry.toml");
        fs::write(&path, "version=1\n").unwrap();
        let mut registry = Registry::open(path.clone()).unwrap();
        registry.doc["version"] = value(2);
        fs::write(&path, "# editor\nversion=3\n").unwrap();
        assert!(registry.commit().is_err());
        assert_eq!(fs::read_to_string(path).unwrap(), "# editor\nversion=3\n");
    }
}
