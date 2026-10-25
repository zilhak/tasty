//! Connection-owned HTTP models. Captured local authority fences every access and retirement.
mod transport;
use crate::{
    connection::ConnectionEpoch,
    html_http_body::BodyBudget,
    html_http_client::{ClientDocument, ClientError, ClientLimits, DocumentKey, PendingBudget},
};
use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};
use tasty_ipc::{html_http::HttpReply, html_transfer::TransferId};
pub use transport::HttpAction;

pub struct CapturedDocument {
    entry: TransferId,
    connection: ConnectionEpoch,
    wake: Arc<dyn Fn() + Send + Sync>,
    model: Mutex<ClientDocument>,
    live: AtomicBool,
    current: Arc<dyn Fn() -> bool + Send + Sync>,
    poisoned: AtomicBool,
}
impl CapturedDocument {
    /// The integrating host supplies its actual source/grant/native-document predicate.
    /// It must perform no I/O, wait or reentry into this model; release source locks before retire.
    pub fn new(
        key: DocumentKey,
        epoch: ConnectionEpoch,
        limits: ClientLimits,
        pending: PendingBudget,
        bodies: BodyBudget,
        current: Arc<dyn Fn() -> bool + Send + Sync>,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Arc<Self>, ClientError> {
        if !epoch.is_active() || !current() {
            return Err(ClientError::Stale);
        }
        let connection = epoch.clone();
        let entry = key.entry;
        let model = ClientDocument::new(key, epoch, limits, pending, bodies, wake.clone())?;
        if !current() {
            return Err(ClientError::Stale);
        }
        Ok(Arc::new(Self {
            entry,
            connection: connection.clone(),
            wake,
            model: Mutex::new(model),
            live: AtomicBool::new(true),
            current: Arc::new(move || connection.is_active() && current()),
            poisoned: AtomicBool::new(false),
        }))
    }
    pub fn entry(&self) -> TransferId {
        self.entry
    }
    pub fn is_current(&self) -> bool {
        self.live.load(Ordering::Acquire) && (self.current)()
    }
    fn poison(&self) {
        self.live.store(false, Ordering::Release);
        if !self.poisoned.swap(true, Ordering::Relaxed) {
            tracing::error!("HTTP client model poisoned; captured document retired");
        }
    }
    /// Callbacks only update bounded model state or enqueue an already prepared command.
    /// Never hold this guard through file/network I/O, decode/scan, native calls or waiting joins.
    pub fn with_current<T>(&self, f: impl FnOnce(&mut ClientDocument) -> T) -> Option<T> {
        if !self.is_current() {
            self.retire();
            return None;
        }
        let Ok(mut model) = self.model.lock() else {
            self.poison();
            return None;
        };
        if !self.is_current() {
            self.live.store(false, Ordering::Release);
            model.send_failed();
            return None;
        }
        let result = f(&mut model);
        if self.is_current() {
            Some(result)
        } else {
            self.live.store(false, Ordering::Release);
            model.send_failed();
            None
        }
    }
    pub fn receive(&self, reply: HttpReply) -> bool {
        self.with_current(|model| model.receive(reply))
            .unwrap_or(false)
    }
    pub fn retire(&self) {
        self.live.store(false, Ordering::Release);
        match self.model.lock() {
            Ok(mut model) => model.send_failed(),
            Err(_) => self.poison(),
        }
    }
}
/// A finite connection owner. Replacing/removing one entry revokes all of its cloned handles.
/// Dropping the connection table also retires native-held clones, without retiring another epoch.
#[derive(Default)]
pub struct HttpDocuments(HashMap<u32, Arc<CapturedDocument>>);
impl HttpDocuments {
    pub fn insert(&mut self, document: Arc<CapturedDocument>) -> Result<(), ClientError> {
        if !document.is_current() {
            document.retire();
            return Err(ClientError::Stale);
        }
        let remote = document.entry.surface_id;
        if let Some(prior) = self.0.get(&remote) {
            if Arc::ptr_eq(prior, &document) {
                return Ok(());
            }
            if prior.is_current() {
                let old = prior.entry;
                let new = document.entry;
                if (old.epoch, old.binding) != (new.epoch, new.binding)
                    || new.document <= old.document
                {
                    document.retire();
                    return Err(ClientError::AlreadyUsed);
                }
            }
        }
        if !self.0.contains_key(&remote) && self.0.len() >= 8 {
            document.retire();
            return Err(ClientError::Quota);
        }
        if let Some(prior) = self.0.insert(remote, document) {
            prior.retire();
        }
        Ok(())
    }
    pub fn get(&self, remote: u32) -> Option<&Arc<CapturedDocument>> {
        self.0.get(&remote)
    }
    pub fn snapshot(&self) -> Vec<(u32, Arc<CapturedDocument>)> {
        self.0
            .iter()
            .map(|(id, document)| (*id, document.clone()))
            .collect()
    }
    pub fn receive(&self, reply: HttpReply) -> bool {
        self.0
            .get(&reply.id().surface_id)
            .is_some_and(|document| document.receive(reply))
    }
    pub fn retain(&mut self, mut keep: impl FnMut(u32) -> bool) {
        self.0.retain(|remote, document| {
            let keep = keep(*remote) && document.is_current();
            if !keep {
                document.retire();
            }
            keep
        });
    }
    pub fn clear(&mut self) {
        for document in self.0.drain().map(|(_, value)| value) {
            document.retire();
        }
    }
}
impl Drop for HttpDocuments {
    fn drop(&mut self) {
        self.clear();
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod transport_tests;
