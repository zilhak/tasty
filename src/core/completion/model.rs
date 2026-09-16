//! Persisted identities and delivery states. Surface numbers are only live addresses.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Journal {
    pub sequence: u64,
    pub instance: String,
    pub sessions: BTreeMap<u32, Session>,
    pub bindings: BTreeMap<String, Binding>,
    pub subscriptions: BTreeMap<u64, Subscription>,
    pub events: BTreeMap<u64, Event>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Session {
    pub registration: String,
    pub kind: String,
    pub hook_session: String,
    pub generation: u64,
    pub state: String,
    pub epoch: u64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Binding {
    pub generation: u64,
    pub key: String,
    pub surface: u32,
    pub hook_session: String,
    pub endpoint: String,
    pub auth_env: Option<String>,
    pub thread_id: String,
    pub session_id: String,
    pub phase: String,
    pub diagnostic: String,
    pub server: String,
    pub cli_version: String,
    pub codex_home: String,
    pub expected_home: Option<String>,
    pub daemon_pid: Option<u64>,
    pub endpoint_identity: String,
    pub connection_generation: u64,
    pub probe_attempts: u32,
    pub probe_after: u64,
    pub history_mode: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: u64,
    pub parent: u32,
    pub parent_session: String,
    pub child: u32,
    pub child_kind: String,
    pub child_generation: Option<u64>,
    pub child_session: Option<String>,
    pub await_session: bool,
    pub mode: String,
    pub active: bool,
    pub reason: String,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Event {
    pub delivery_id: String,
    pub recovery: serde_json::Value,
    pub child_generation: Option<u64>,
    pub id: u64,
    pub subscription: u64,
    pub binding: Option<String>,
    pub epoch: u64,
    pub state: String,
    pub cause: String,
    pub child_kind: String,
    pub child: u32,
    pub phase: String,
    pub diagnostic: String,
    pub attempts: u32,
    pub retry_at: u64,
    pub summary: String,
}
impl Journal {
    pub fn next(&mut self) -> u64 {
        self.sequence += 1;
        self.sequence
    }
    pub fn close_subscription(&mut self, id: u64, reason: &str) {
        if let Some(sub) = self.subscriptions.get_mut(&id) {
            sub.active = false;
            sub.reason = reason.into();
        }
        for event in self.events.values_mut().filter(|e| e.subscription == id) {
            if matches!(event.phase.as_str(), "pending" | "blocked" | "unbound") {
                event.phase = "cancelled".into();
                event.diagnostic = reason.into();
            }
        }
    }
}
