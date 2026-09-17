//! Captured close effects refer to logical subscriptions, never a later reused address.
use super::*;
use serde::{Deserialize, Serialize};

#[derive(Clone, Serialize, Deserialize)]
pub struct CloseTask {
    pub id: String,
    pub surface: u32,
    pub identity: Option<Session>,
    pub children: Vec<u64>,
    pub parents: Vec<u64>,
    pub events: Vec<Event>,
    pub bindings: Vec<Binding>,
    pub relations: Vec<(u32, u32, u64)>,
    pub attempts: u32,
    pub retry_at: u64,
    pub error: String,
    pub durable: bool,
}
impl CloseTask {
    pub fn capture(j: &Journal, surface: u32, cause: &str, id: String) -> anyhow::Result<Self> {
        let mut next = j.clone();
        actions::close_transition(&mut next, surface, cause)?;
        let changed = |sub: &&Subscription| {
            j.subscriptions
                .get(&sub.id)
                .is_some_and(|old| old.active != sub.active || old.reason != sub.reason)
        };
        Ok(Self {
            id,
            surface,
            identity: j
                .sessions
                .get(&surface)
                .or_else(|| j.ended_sessions.get(&surface))
                .cloned(),
            children: next
                .subscriptions
                .values()
                .filter(changed)
                .filter(|s| s.reason == "target_exited")
                .map(|s| s.id)
                .collect(),
            parents: next
                .subscriptions
                .values()
                .filter(changed)
                .filter(|s| s.reason == "parent_closed")
                .map(|s| s.id)
                .collect(),
            events: next
                .events
                .values()
                .filter(|e| !j.events.contains_key(&e.id))
                .cloned()
                .collect(),
            bindings: j
                .bindings
                .values()
                .filter(|b| {
                    next.bindings.get(&b.key).is_some_and(|n| {
                        n.diagnostic == "parent_closed"
                            && (n.phase != b.phase || n.diagnostic != b.diagnostic)
                    })
                })
                .cloned()
                .collect(),
            relations: j
                .live_relations
                .iter()
                .filter(|(key, _)| !next.live_relations.contains_key(key))
                .map(|((p, c), r)| (*p, *c, r.generation))
                .collect(),
            attempts: 0,
            retry_at: 0,
            error: String::new(),
            durable: false,
        })
    }
    fn same_identity(&self, current: Option<&Session>) -> bool {
        match (&self.identity, current) {
            (Some(old), Some(current)) => {
                old.generation == current.generation
                    && old.hook_session == current.hook_session
                    && old.kind == current.kind
            }
            (None, None) => true,
            _ => false,
        }
    }
    pub fn blocks_surface(&self, j: &Journal, surface: u32) -> bool {
        self.surface == surface
            && (j.sessions.get(&surface).is_none() || self.same_identity(j.sessions.get(&surface)))
    }
    fn clear_addresses(&self, j: &mut Journal) {
        if self.same_identity(j.sessions.get(&self.surface)) {
            j.sessions.remove(&self.surface);
        }
        if self.same_identity(j.ended_sessions.get(&self.surface)) {
            j.ended_sessions.remove(&self.surface);
        }
        for (p, c, generation) in &self.relations {
            if j.live_relations
                .get(&(*p, *c))
                .is_some_and(|r| r.generation == *generation)
            {
                j.live_relations.remove(&(*p, *c));
            }
        }
        for binding in &self.bindings {
            if let Some(current) = j.bindings.get_mut(&binding.key)
                && current.generation == binding.generation
            {
                current.phase = "unbound".into();
                current.diagnostic = "parent_closed".into();
            }
        }
    }
    pub fn apply(&self, j: &mut Journal) {
        for id in &self.children {
            if let Some(sub) = j.subscriptions.get_mut(id)
                && sub.active
            {
                sub.active = false;
                sub.reason = "target_exited".into();
            }
        }
        for id in &self.parents {
            j.close_subscription(*id, "parent_closed");
        }
        for prototype in &self.events {
            if !j
                .subscriptions
                .get(&prototype.subscription)
                .is_some_and(Subscription::accepts_pending)
            {
                continue;
            }
            let delivery_id = format!(
                "tasty-{}-close-{}-{}",
                j.instance, self.id, prototype.subscription
            );
            if j.events.values().any(|e| e.delivery_id == delivery_id) {
                continue;
            }
            let mut event = prototype.clone();
            event.id = j.next();
            event.delivery_id = delivery_id;
            if let Some(sub) = j.subscriptions.get(&event.subscription)
                && let Some(binding) = j.bindings.values().find(|b| {
                    b.surface == sub.parent
                        && b.hook_session == sub.parent_session
                        && b.phase != "superseded"
                })
            {
                event.binding = Some(binding.key.clone());
                // Only this newly captured, never-sent exit is being enqueued.
                // A binding may have arrived while its close intent was pending.
                event.phase = "pending".into();
            }
            j.events.insert(event.id, event);
        }
        self.clear_addresses(j);
    }
    pub fn project(&self, j: &mut Journal) {
        for id in self.children.iter().chain(&self.parents) {
            if let Some(sub) = j.subscriptions.get_mut(id)
                && sub.active
            {
                sub.active = false;
                sub.reason = "close_persistence_pending".into();
            }
            for event in j.events.values_mut().filter(|e| e.subscription == *id) {
                if matches!(event.phase.as_str(), "pending" | "unbound" | "blocked") {
                    event.diagnostic = format!("close_persistence_pending: {}", self.error);
                }
            }
        }
        self.clear_addresses(j);
    }
}
impl Journal {
    pub fn close_blocks_surface(&self, surface: u32) -> bool {
        self.pending_closes
            .values()
            .any(|t| t.blocks_surface(self, surface))
    }
    pub fn close_blocks_subscription(&self, id: u64) -> bool {
        self.pending_closes
            .values()
            .any(|t| t.children.contains(&id) || t.parents.contains(&id))
    }
}
