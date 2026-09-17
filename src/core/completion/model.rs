//! Persisted identities and delivery states. Surface numbers are only live addresses.
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Default, Serialize, Deserialize)]
pub struct Journal {
    pub sequence: u64,
    #[serde(skip)]
    pub pending_closes: BTreeMap<String, super::close_effects::CloseTask>,
    // Only relationships established in this host lifetime can authorize an
    // unregistered parent. Never restore this address map from the journal.
    #[serde(skip)]
    pub live_relations: BTreeMap<(u32, u32), LiveRelation>,
    pub instance: String,
    pub sessions: BTreeMap<u32, Session>,
    #[serde(default)]
    pub ended_sessions: BTreeMap<u32, Session>,
    pub bindings: BTreeMap<String, Binding>,
    pub subscriptions: BTreeMap<u64, Subscription>,
    pub events: BTreeMap<u64, Event>,
    #[serde(default)]
    pub error_observers: BTreeMap<u64, ErrorObserver>,
}
#[derive(Clone)]
pub struct LiveRelation {
    pub generation: u64,
    pub restored: bool,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ErrorObserver {
    pub subscription: u64,
    pub generation: Option<u64>,
    #[serde(default)]
    pub notified_epoch: Option<u64>,
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
    #[serde(default)]
    pub relation_generation: Option<u64>,
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
    #[serde(default)]
    pub origin_binding: Option<String>,
    #[serde(default)]
    pub delivery_context: serde_json::Value,
    #[serde(default)]
    pub server_response: serde_json::Value,
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
impl Subscription {
    pub fn accepts_pending(&self) -> bool {
        self.active
            || matches!(
                self.reason.as_str(),
                "target_exited" | "target_identity_unavailable"
            )
    }

    pub fn matches_child(&self, session: &Session) -> bool {
        self.child_generation
            .is_none_or(|generation| generation == session.generation)
            && self
                .child_session
                .as_deref()
                .is_none_or(|id| id == session.hook_session)
    }
}
impl Journal {
    pub fn owns_subscription(&self, sub: &Subscription, parent: u32) -> bool {
        sub.parent == parent
            && self
                .sessions
                .get(&parent)
                .or_else(|| self.ended_sessions.get(&parent))
                .is_some_and(|current| {
                    !sub.parent_session.is_empty() && current.hook_session == sub.parent_session
                })
    }

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
