//! Owned file snapshots and storage-neutral receipts. Only the worker calls `write`.
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::de::DeserializeOwned;
use tasty_utils::file_replace::{self, Fingerprint};

pub(crate) trait PendingSave: Send {
    fn poll(&self) -> Option<Result<(), String>>;
}

pub(crate) type Submit =
    Arc<dyn Fn(Snapshot) -> Result<Box<dyn PendingSave>, String> + Send + Sync>;

struct Observation {
    target: PathBuf,
    content: Fingerprint,
}

/// An unreadable or unparseable original remains protected for this load's lifetime.
pub(crate) struct Document {
    path: PathBuf,
    observed: Mutex<Result<Observation, String>>,
}

impl Document {
    pub(crate) fn load<T: DeserializeOwned + Default>(path: PathBuf) -> (T, Arc<Self>) {
        let loaded = (|| {
            let target = file_replace::resolve_link(&path).map_err(|e| e.to_string())?;
            let bytes = match std::fs::read(&path) {
                Ok(bytes) => Some(bytes),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.to_string()),
            };
            let (value, content) = match bytes {
                Some(bytes) => {
                    let text = std::str::from_utf8(&bytes).map_err(|e| e.to_string())?;
                    let value = toml::from_str(text).map_err(|e| e.to_string())?;
                    (value, Fingerprint::of_bytes(&bytes))
                }
                None => (T::default(), Fingerprint::Missing),
            };
            Ok((value, Observation { target, content }))
        })();
        let (value, observed) = match loaded {
            Ok((value, observed)) => (value, Ok(observed)),
            Err(error) => {
                tracing::warn!(path = %path.display(), %error, "file load failed; protecting original");
                (T::default(), Err(error))
            }
        };
        (
            value,
            Arc::new(Self {
                path,
                observed: Mutex::new(observed),
            }),
        )
    }

    fn write(&self, bytes: &[u8]) -> Result<(), String> {
        let run = || -> anyhow::Result<()> {
            if let Some(parent) = self.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            let _guard = file_replace::lock(&self.path)?;
            let mut observation = self
                .observed
                .lock()
                .map_err(|_| anyhow::anyhow!("file observation poisoned"))?;
            let previous = observation
                .as_mut()
                .map_err(|e| anyhow::anyhow!("original file was not readable and valid: {e}"))?;
            if previous.target != file_replace::resolve_link(&self.path)?
                || previous.content != Fingerprint::read(&self.path)?
            {
                anyhow::bail!("file changed outside this snapshot; reload before saving");
            }
            file_replace::replace(&self.path, bytes)?;
            previous.content = Fingerprint::of_bytes(bytes);
            Ok(())
        };
        run().map_err(|e| format!("{}: {e:#}", self.path.display()))
    }
}

pub(crate) struct Snapshot {
    pub(crate) key: &'static str,
    pub(crate) document: Arc<Document>,
    pub(crate) bytes: Vec<u8>,
}

impl Snapshot {
    pub(crate) fn write(&self) -> Result<(), String> {
        self.document.write(&self.bytes)
    }
}
