//! Owned registry commands execute one complete read/merge/replace transaction.
use super::{ChildTerminalRegistry, ParentKey};
use std::path::{Path, PathBuf};
use tasty_utils::file_replace::{self, Fingerprint};

#[derive(Clone)]
pub(super) enum Operation {
    Merge,
    Reserve(ParentKey),
    #[cfg(feature = "gui")]
    Prune(std::collections::HashSet<u32>),
    #[cfg(feature = "gui")]
    Clear,
}

pub(super) struct Command {
    path: PathBuf,
    registry: ChildTerminalRegistry,
    operation: Operation,
}

pub(super) struct Outcome {
    pub(super) registry: ChildTerminalRegistry,
    pub(super) index: Option<u32>,
}

impl Command {
    pub(super) fn new(
        path: PathBuf,
        registry: ChildTerminalRegistry,
        operation: Operation,
    ) -> Self {
        Self {
            path,
            registry,
            operation,
        }
    }

    pub(super) fn execute(mut self) -> Result<Outcome, String> {
        self.execute_inner()
            .map_err(|error| format!("child registry: {error:#}"))
    }

    fn execute_inner(&mut self) -> anyhow::Result<Outcome> {
        if let Some(error) = &self.registry.load_error {
            anyhow::bail!("original load failed: {error}");
        }
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let target = file_replace::resolve_link(&self.path)?;
        let _participant = file_replace::lock(&self.path)?;
        if target != file_replace::resolve_link(&self.path)? {
            anyhow::bail!("registry target changed while acquiring participant lock");
        }
        let (disk, observed) = read_observed(&self.path)?;
        self.registry.merge_from(disk);
        let index = self
            .registry
            .apply_operation(self.operation.clone())
            .map_err(anyhow::Error::msg)?;
        let bytes = serde_json::to_vec_pretty(&self.registry)?;
        if target != file_replace::resolve_link(&self.path)?
            || observed != Fingerprint::read(&self.path)?
        {
            anyhow::bail!("registry changed outside the participant lock; not replaced");
        }
        file_replace::replace(&self.path, &bytes)?;
        Ok(Outcome {
            registry: std::mem::take(&mut self.registry),
            index,
        })
    }
}

pub(super) fn read(path: &Path) -> Result<ChildTerminalRegistry, String> {
    read_observed(path)
        .map(|(registry, _)| registry)
        .map_err(|error| format!("{error:#}"))
}

fn read_observed(path: &Path) -> anyhow::Result<(ChildTerminalRegistry, Fingerprint)> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok((ChildTerminalRegistry::default(), Fingerprint::Missing));
        }
        Err(error) => anyhow::bail!("registry read failed ({:?})", error.kind()),
    };
    // Never include JSON excerpts or registry payloads in diagnostics.
    let registry =
        serde_json::from_slice(&bytes).map_err(|_| anyhow::anyhow!("registry parse failed"))?;
    Ok((registry, Fingerprint::of_bytes(&bytes)))
}

#[cfg(test)]
mod tests;
