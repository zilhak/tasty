//! Protect the exact document read at load, including originals that could not be parsed.
use std::{
    collections::BTreeMap,
    io,
    path::{Path, PathBuf},
};
use tasty_utils::file_replace::{self, Fingerprint};

#[derive(Debug, Clone)]
struct Original {
    target: PathBuf,
    content: Fingerprint,
}
#[derive(Debug, Default, Clone)]
pub(super) struct Observations(BTreeMap<PathBuf, Result<Original, String>>);

pub(super) fn check_target(path: &Path) -> io::Result<()> {
    let target = file_replace::resolve_link(path)?;
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::other("preset target has no parent"))?;
    let name = target
        .file_name()
        .ok_or_else(|| io::Error::other("preset target has no filename"))?;
    let target = parent.canonicalize()?.join(name);
    let root = path
        .parent()
        .and_then(Path::parent)
        .ok_or_else(|| io::Error::other("preset root missing"))?
        .canonicalize()?;
    if target == root.join(".preset-mutations") || target == root.join(".preset-mutations.lock") {
        return Err(io::Error::other(
            "preset target aliases the participant lock namespace",
        ));
    }
    Ok(())
}

impl Observations {
    pub(super) fn forget(&mut self, path: &Path) {
        self.0.remove(path);
    }
    pub(super) fn read(&mut self, path: &Path) -> io::Result<String> {
        let read: io::Result<_> = (|| {
            check_target(path)?;
            let target = file_replace::resolve_link(path)?;
            let text = std::fs::read_to_string(path)?;
            let original = Original {
                target,
                content: Fingerprint::of_bytes(text.as_bytes()),
            };
            Ok((text, original))
        })();
        match read {
            Ok((text, original)) => {
                self.0.insert(path.into(), Ok(original));
                Ok(text)
            }
            Err(error) => {
                self.0.insert(
                    path.into(),
                    Err(format!("read failed ({:?})", error.kind())),
                );
                Err(error)
            }
        }
    }
    pub(super) fn reject_parse(&mut self, path: &Path) {
        self.0
            .insert(path.into(), Err("preset parse failed".into()));
    }
    fn previous(&self, path: &Path) -> io::Result<Original> {
        match self.0.get(path) {
            Some(Ok(original)) => Ok(original.clone()),
            Some(Err(error)) => Err(io::Error::other(format!(
                "original preset was not readable and valid: {error}"
            ))),
            None => Ok(Original {
                target: file_replace::resolve_link(path)?,
                content: Fingerprint::Missing,
            }),
        }
    }
    pub(super) fn check(&self, path: &Path) -> io::Result<()> {
        let previous = self.previous(path)?;
        if previous.target != file_replace::resolve_link(path)?
            || previous.content != Fingerprint::read(path)?
        {
            return Err(io::Error::other(
                "preset changed outside this store; reload before mutation",
            ));
        }
        Ok(())
    }
    pub(super) fn replace(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.replace_from(path, bytes, None)
    }
    pub(super) fn replace_from(
        &mut self,
        path: &Path,
        bytes: &[u8],
        source: Option<&Path>,
    ) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        check_target(path)?;
        let _guard = file_replace::lock(path)?;
        let previous = self.previous(path)?;
        let target = file_replace::resolve_link(path)?;
        if previous.target != target || previous.content != Fingerprint::read(path)? {
            return Err(io::Error::other(
                "preset changed outside this store; reload before saving",
            ));
        }
        match source {
            Some(source) => file_replace::replace_preserving(path, bytes, source)?,
            None => file_replace::replace(path, bytes)?,
        }
        self.0.insert(
            path.into(),
            Ok(Original {
                target,
                content: Fingerprint::of_bytes(bytes),
            }),
        );
        Ok(())
    }
}
