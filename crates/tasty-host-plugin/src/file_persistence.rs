//! Participating read/merge/replace operations. Execute these owned updates on the file worker.
mod config;
mod document;
mod formatted;
mod intent;
#[cfg(test)]
mod tests;

pub use config::ConfigUpdate;
pub(crate) use config::save as save_config;
pub(crate) use document::Origin;
pub(crate) use document::{decode, encode, read};

use crate::known_plugins::KnownPlugins;
use std::path::Path;

/// Read the latest trust document while holding the same lock used by all save participants.
/// The callback runs without a manager/registry guard and must only edit this owned document.
pub fn update_known<T>(
    path: &Path,
    edit: impl FnOnce(&mut KnownPlugins) -> std::io::Result<T>,
) -> std::io::Result<T> {
    document::mutate(path, |table| {
        let mut known: KnownPlugins = document::decode(table)?;
        let before = document::encode(&known)?;
        let output = edit(&mut known)?;
        let after = document::encode(&known)?;
        document::patch_maps(table, &before, &after, &["plugins", "unverified"])?;
        Ok(output)
    })
}

pub(crate) fn save_known(known: &KnownPlugins) -> std::io::Result<()> {
    let path = known
        .origin
        .as_ref()
        .map(|o| o.path.clone())
        .or_else(KnownPlugins::path)
        .ok_or_else(|| std::io::Error::other("known-plugins.toml path unavailable"))?;
    let after = document::encode(known)?;
    let before = match &known.origin {
        Some(origin) => origin.baseline()?,
        None => document::encode(&KnownPlugins::default())?,
    };
    document::mutate(&path, |table| {
        document::patch_maps(table, &before, &after, &["plugins", "unverified"])
    })?;
    if let Some(origin) = &known.origin {
        origin.advance(after)?;
    }
    Ok(())
}
