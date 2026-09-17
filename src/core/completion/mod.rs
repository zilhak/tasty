//! Host-owned durable child-completion delivery. No terminal input fallback.
mod actions;
#[cfg(test)]
mod callback_tests;
mod callbacks;
#[cfg(test)]
mod close_binding_tests;
mod close_effects;
mod close_recovery;
#[cfg(test)]
mod close_tests;
mod deadline;
mod diagnostics;
mod history;
mod identity;
#[cfg(test)]
mod late_identity_tests;
mod model;
mod observation;
mod ownership;
mod protocol;
#[cfg(test)]
mod relation_tests;
mod relations;
#[cfg(test)]
mod review_tests;
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
    live_owners:
        Mutex<std::collections::BTreeMap<usize, (Weak<()>, std::collections::HashSet<u32>)>>,
}
impl Completion {
    pub fn memory() -> Result<Arc<Self>> {
        Self::open_connection(Connection::open_in_memory()?)
    }
    fn open_connection(connection: Connection) -> Result<Arc<Self>> {
        connection.execute_batch("PRAGMA locking_mode=EXCLUSIVE; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS completion_journal (id INTEGER PRIMARY KEY CHECK(id=1), body TEXT NOT NULL);")?;
        connection.execute_batch("CREATE TABLE IF NOT EXISTS completion_closures (id TEXT PRIMARY KEY, body TEXT NOT NULL);")?;
        let body: Option<String> = connection
            .query_row("SELECT body FROM completion_journal WHERE id=1", [], |r| {
                r.get(0)
            })
            .optional()?;
        let mut journal: Journal = body
            .map(|s| serde_json::from_str(&s))
            .transpose()?
            .unwrap_or_default();
        {
            let mut statement = connection.prepare("SELECT body FROM completion_closures")?;
            for body in statement.query_map([], |row| row.get::<_, String>(0))? {
                let mut task: close_effects::CloseTask = serde_json::from_str(&body?)?;
                task.durable = true;
                task.retry_at = 0;
                journal.pending_closes.insert(task.id.clone(), task);
            }
        }
        if journal.instance.is_empty() {
            journal.instance =
                connection.query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))?;
        }
        for binding in journal
            .bindings
            .values_mut()
            .filter(|b| b.phase != "superseded")
        {
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
        // Never associate an unregistered execution with a reused numeric address.
        let unresolved: Vec<_> = journal
            .subscriptions
            .values()
            .filter(|s| s.active && (s.parent_session.is_empty() || s.child_session.is_none()))
            .map(|s| s.id)
            .collect();
        for id in unresolved {
            if journal.subscriptions[&id].parent_session.is_empty() {
                journal.close_subscription(id, "restart_parent_identity_unresolved");
            } else {
                let sub = journal.subscriptions.get_mut(&id).expect("collected above");
                sub.active = false;
                sub.reason = "target_identity_unavailable".into();
                // Previously persisted facts can still reach their known logical parent.
            }
        }
        // A restarted host cannot treat persisted surface numbers as live identities.
        journal.sessions.clear();
        journal.ended_sessions.clear();
        journal.error_observers.clear();
        let service = Arc::new(Self {
            inner: Mutex::new((connection, journal)),
            live_owners: Mutex::default(),
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
        let guard = self
            .inner
            .lock()
            .map_err(|_| anyhow::anyhow!("completion journal poisoned"))?;
        let mut snapshot = guard.1.clone();
        for task in guard.1.pending_closes.values() {
            task.project(&mut snapshot);
        }
        Ok(snapshot)
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
