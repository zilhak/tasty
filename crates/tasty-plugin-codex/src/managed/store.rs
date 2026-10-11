//! Small relay descriptors, not a scheduler or a copy of task results.
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Record {
    pub relay_id: String,
    pub parent_thread: String,
    pub workspace_id: u32,
    pub task_id: String,
    pub cli_path: String,
    pub cli_home: String,
    pub port_file: String,
    pub instance_id: String,
    pub relay_thread: Option<String>,
}

pub fn new_id() -> Result<String> {
    static SEQUENCE: AtomicU64 = AtomicU64::new(0);
    Ok(format!(
        "relay-{:x}-{}-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos(),
        std::process::id(),
        SEQUENCE.fetch_add(1, Ordering::Relaxed)
    ))
}

pub fn path(root: &Path, id: &str) -> Result<PathBuf> {
    if id.is_empty()
        || id.len() > 100
        || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        bail!("invalid_relay_id");
    }
    Ok(root.join("managed-relays").join(format!("{id}.json")))
}

pub fn read(root: &Path, id: &str) -> Result<Record> {
    let path = path(root, id)?;
    if std::fs::metadata(&path)?.len() > 16 * 1024 {
        bail!("relay_record_too_large");
    }
    let record: Record = serde_json::from_slice(&std::fs::read(path)?)?;
    if record.relay_id != id {
        bail!("relay_record_identity_mismatch");
    }
    Ok(record)
}

pub fn write(root: &Path, record: &Record, new: bool) -> Result<()> {
    use std::io::Write;
    let target = path(root, &record.relay_id)?;
    std::fs::create_dir_all(target.parent().context("relay_parent_missing")?)?;
    let temporary = target.with_extension(format!("{}.tmp", new_id()?));
    let result = (|| -> Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temporary)?;
        file.write_all(&serde_json::to_vec(record)?)?;
        file.sync_all()?;
        if new && target.exists() {
            bail!("relay_id_already_exists");
        }
        std::fs::rename(&temporary, &target)?;
        Ok(())
    })();
    if result.is_err()
        && temporary.exists()
        && let Err(error) = std::fs::remove_file(&temporary)
    {
        tracing::warn!("managed relay temporary cleanup: {error}");
    }
    result
}
