//! Host-owned durable child-completion delivery. No terminal input fallback.
mod actions;
mod diagnostics;
mod history;
mod identity;
mod model;
mod observation;
mod protocol;
#[cfg(test)]
mod tests;
mod transport;
mod worker;

use anyhow::Result;
pub use model::*;
use rusqlite::Connection;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, Weak};

pub struct Completion {
    inner: Mutex<(Connection, Journal)>,
}
impl Completion {
    pub fn memory() -> Result<Arc<Self>> {
        Self::open_connection(Connection::open_in_memory()?)
    }
    fn open_connection(connection: Connection) -> Result<Arc<Self>> {
        connection.execute_batch("PRAGMA locking_mode=EXCLUSIVE; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS completion_journal (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);")?;
        let body: Option<String> = connection
            .query_row("SELECT body FROM completion_journal WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let mut journal: Journal = body
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        if journal.instance.is_empty() {
            journal.instance =
                connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        }
        for binding in journal.bindings.values_mut() {
            binding.phase = "unbound".into();
            binding.diagnostic =
                "host_restarted: session registration and endpoint verification required".into();
        }
        for event in journal.events.values_mut() {
            if event.phase == "in_flight" {
                event.phase = "unknown".into();
                event.diagnostic = "host_restarted_during_send".into();
            }
        }
        // A restarted host cannot treat persisted surface numbers as live identities.
        journal.sessions.clear();
        let service = Arc::new(Self {
            inner: Mutex::new((connection, journal)),
        });
        service.change(|_| Ok(()))?;
        Ok(service)
    }
    pub fn open(path: &Path) -> Result<Arc<Self>> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        Self::open_connection(Connection::open(path)?)
    }
    pub fn production() -> Result<Arc<Self>> {
        static SERVICES: OnceLock<Mutex<std::collections::HashMap<PathBuf, Weak<Completion>>>> =
            OnceLock::new();
        let path = tasty_utils::path::tasty_home()
            .ok_or_else(|| anyhow::anyhow!("completion home unavailable"))?
            .join("completion.sqlite3");
        let mut services = SERVICES
            .get_or_init(Mutex::default)
            .lock()
            .map_err(|_| anyhow::anyhow!("completion service registry poisoned"))?;
        if let Some(service) = services.get(&path).and_then(Weak::upgrade) {
            return Ok(service);
        }
        let service = Self::open(&path)?;
        worker::start(Arc::downgrade(&service))?;
        services.insert(path, Arc::downgrade(&service));
        Ok(service)
    }
    pub fn snapshot(&self) -> Result<Journal> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("completion journal poisoned"))?
            .1
            .clone())
    }
    pub fn change<T>(&self, f: impl FnOnce(&mut Journal) -> Result<T>) -> Result<T> {
        let mut guard = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("completion journal poisoned"))?;
        let mut next = guard.1.clone();
        let result = f(&mut next)?;
        let body = serde_json::to_string(&next)?;
        guard.0.execute("INSERT INTO completion_journal VALUES(1,?1) ON CONFLICT(id) DO UPDATE SET body=excluded.body", [&body])?;
        guard.1 = next;
        Ok(result)
    }
}
use rusqlite::OptionalExtension;
pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
