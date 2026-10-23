//! Builtin theme files are prepared before first render, using participant-safe atomic replacement.
//! Runtime theme application uses cached or embedded colors and never repairs files.

#[cfg(test)]
mod tests;

use std::{
    fs, io,
    path::{Path, PathBuf},
};
use tasty_utils::file_replace::{self, Fingerprint};
use thiserror::Error;

pub const BUILTIN_MOCHA_ID: &str = "mocha";
pub const BUILTIN_LATTE_ID: &str = "latte";

#[derive(Debug, Error)]
pub enum ThemeStoreError {
    #[error("HOME directory unavailable")]
    HomeUnavailable,
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
}

pub fn themes_dir() -> Result<PathBuf, ThemeStoreError> {
    tasty_utils::path::tasty_home()
        .map(|home| home.join("themes"))
        .ok_or(ThemeStoreError::HomeUnavailable)
}

/// Blocking bootstrap backend. Call before first render or from an owned I/O worker.
/// A runtime missing-theme fallback must use embedded colors instead.
pub struct ThemeFiles {
    dir: PathBuf,
}

impl ThemeFiles {
    pub fn new(dir: PathBuf) -> Self {
        Self { dir }
    }
    fn home() -> Result<Self, ThemeStoreError> {
        Ok(Self::new(themes_dir()?))
    }
    fn prepare_dir(&self) -> io::Result<()> {
        fs::create_dir_all(&self.dir)
    }
    fn path(&self, id: &str) -> PathBuf {
        self.dir.join(format!("{id}.toml"))
    }

    /// Seed both builtins only when no TOML exists. This is not a two-file transaction.
    pub fn first_run_init(&self) -> Result<(), ThemeStoreError> {
        self.prepare_dir()?;
        if has_any_toml(&self.dir)? {
            return Ok(());
        }
        self.sync_one(BUILTIN_MOCHA_ID, crate::MOCHA_TOML_TEXT, true)?;
        self.sync_one(BUILTIN_LATTE_ID, crate::LATTE_TOML_TEXT, true)?;
        Ok(())
    }

    /// Restore Mocha drift; sync Latte only while present. Custom files remain untouched.
    pub fn sync_builtin_themes(&self) -> Result<(), ThemeStoreError> {
        self.prepare_dir()?;
        self.sync_one(BUILTIN_MOCHA_ID, crate::MOCHA_TOML_TEXT, true)?;
        self.sync_one(BUILTIN_LATTE_ID, crate::LATTE_TOML_TEXT, false)?;
        Ok(())
    }

    fn sync_one(&self, id: &str, text: &str, create: bool) -> io::Result<()> {
        let (observed, current) = Observed::read(self.path(id))?;
        if current.is_none() && !create {
            return Ok(());
        }
        if current.as_deref() != Some(text) {
            observed.replace(text)?;
        }
        Ok(())
    }

    /// Repair missing/readable corrupt Mocha. Read/UTF-8 errors protect the original.
    pub fn ensure_mocha_exists(&self) -> Result<(), ThemeStoreError> {
        self.prepare_dir()?;
        let (observed, text) = Observed::read(self.path(BUILTIN_MOCHA_ID))?;
        if text
            .as_deref()
            .is_none_or(|s| crate::file::ThemeFile::parse(s).is_err())
        {
            observed.replace(crate::MOCHA_TOML_TEXT)?;
        }
        Ok(())
    }

    /// Explicit bootstrap repair, preserving unreadable originals and concurrent edits.
    pub fn rewrite_mocha_fallback(&self) -> Result<(), ThemeStoreError> {
        self.prepare_dir()?;
        let (observed, _) = Observed::read(self.path(BUILTIN_MOCHA_ID))?;
        observed.replace(crate::MOCHA_TOML_TEXT)?;
        Ok(())
    }
}

struct Observed {
    path: PathBuf,
    target: PathBuf,
    fingerprint: Fingerprint,
}
impl Observed {
    fn read(path: PathBuf) -> io::Result<(Self, Option<String>)> {
        let target = file_replace::resolve_link(&path)?;
        match fs::metadata(&target) {
            Ok(m) if !m.is_file() => return Err(io::Error::other("theme is not a regular file")),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
        let bytes = match fs::read(&target) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        let fingerprint = bytes
            .as_deref()
            .map_or(Fingerprint::Missing, Fingerprint::of_bytes);
        let text = bytes
            .map(String::from_utf8)
            .transpose()
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "theme is not UTF-8"))?;
        Ok((
            Self {
                path,
                target,
                fingerprint,
            },
            text,
        ))
    }
    fn replace(&self, text: &str) -> io::Result<()> {
        let _lock = file_replace::lock(&self.path)?;
        if file_replace::resolve_link(&self.path)? != self.target
            || Fingerprint::read(&self.target)? != self.fingerprint
        {
            return Err(io::Error::other(
                "theme changed outside the observed snapshot",
            ));
        }
        file_replace::replace(&self.path, text.as_bytes())
    }
}

fn has_any_toml(dir: &Path) -> io::Result<bool> {
    for entry in fs::read_dir(dir)? {
        if entry?
            .path()
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("toml"))
        {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Blocking bootstrap preparation; see ThemeFiles. Runtime apply does not call this.
pub fn first_run_init() -> Result<(), ThemeStoreError> {
    ThemeFiles::home()?.first_run_init()
}
pub fn sync_builtin_themes() -> Result<(), ThemeStoreError> {
    ThemeFiles::home()?.sync_builtin_themes()
}
pub fn ensure_mocha_exists() -> Result<(), ThemeStoreError> {
    ThemeFiles::home()?.ensure_mocha_exists()
}
pub fn rewrite_mocha_fallback() -> Result<(), ThemeStoreError> {
    ThemeFiles::home()?.rewrite_mocha_fallback()
}
