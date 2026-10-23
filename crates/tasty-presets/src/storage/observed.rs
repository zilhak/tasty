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

impl Observations {
    pub(super) fn forget(&mut self, path: &Path) {
        self.0.remove(path);
    }
    pub(super) fn read(&mut self, path: &Path) -> io::Result<String> {
        let read: io::Result<_> = (|| {
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
    pub(super) fn replace(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let _guard = file_replace::lock(path)?;
        let previous = self.previous(path)?;
        let target = file_replace::resolve_link(path)?;
        if previous.target != target || previous.content != Fingerprint::read(path)? {
            return Err(io::Error::other(
                "preset changed outside this store; reload before saving",
            ));
        }
        file_replace::replace(path, bytes)?;
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
