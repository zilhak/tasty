//! Bind the signed-manifest scope and the selected plugin's trust observation across retirement.
use crate::known_plugins::{KnownPluginEntry, KnownPlugins, UnverifiedChoice};
use sha2::{Digest, Sha256};
use std::{
    io,
    path::{Path, PathBuf},
};

pub(super) struct PreparedSource {
    canonical: PathBuf,
    known_path: PathBuf,
    manifest: [u8; 32],
    signature: Option<[u8; 32]>,
    key: Option<KnownPluginEntry>,
    choice: Option<UnverifiedChoice>,
}
impl PreparedSource {
    pub(super) fn capture(
        source: &Path,
        known_path: &Path,
        known: &KnownPlugins,
        id: &str,
    ) -> io::Result<Self> {
        Ok(Self {
            canonical: source.canonicalize()?,
            known_path: known_path.to_path_buf(),
            manifest: required_digest(&source.join("tasty-plugin.toml"))?,
            signature: optional_digest(&source.join("tasty-plugin.toml.sig"))?,
            key: known.lookup(id).cloned(),
            choice: known.unverified_choice(id).cloned(),
        })
    }
    pub(super) fn validate(&self, source: &Path, id: &str) -> io::Result<KnownPlugins> {
        if source.canonicalize()? != self.canonical {
            return Err(changed());
        }
        self.validate_manifest(source)?;
        let known = KnownPlugins::load_from(self.known_path.clone())?;
        if known.lookup(id) != self.key.as_ref()
            || known.unverified_choice(id) != self.choice.as_ref()
        {
            return Err(changed());
        }
        Ok(known)
    }
    pub(super) fn validate_manifest(&self, directory: &Path) -> io::Result<()> {
        if required_digest(&directory.join("tasty-plugin.toml"))? != self.manifest
            || optional_digest(&directory.join("tasty-plugin.toml.sig"))? != self.signature
        {
            return Err(changed());
        }
        Ok(())
    }
    pub(super) fn retained_bytes(&self) -> usize {
        self.canonical
            .as_os_str()
            .len()
            .saturating_add(self.known_path.as_os_str().len())
            .saturating_add(
                serde_json::to_vec(&(&self.key, &self.choice)).map_or(usize::MAX, |v| v.len()),
            )
            .saturating_add(std::mem::size_of::<Self>())
    }
}
fn changed() -> io::Error {
    io::Error::other("prepared builtin source or trust changed; prepare the upgrade again")
}
fn required_digest(path: &Path) -> io::Result<[u8; 32]> {
    optional_digest(path)?.ok_or_else(|| io::Error::other("prepared builtin manifest unavailable"))
}
fn optional_digest(path: &Path) -> io::Result<Option<[u8; 32]>> {
    match std::fs::read(path) {
        Ok(bytes) => Ok(Some(Sha256::digest(bytes).into())),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io::Error::new(
            error.kind(),
            "prepared builtin source read failed",
        )),
    }
}
