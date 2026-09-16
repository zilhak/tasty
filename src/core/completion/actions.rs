//! Atomic lifecycle transitions. A release and a worker claim share the journal lock.
use super::*;
use anyhow::{Result, bail};

impl Completion {
    #[cfg(test)]
    pub fn subscribe(&self, parent: u32, child: u32, kind: &str, mode: &str) -> Result<u64> {
        self.subscribe_wait(parent, child, kind, mode, false)
    }
    pub fn subscribe_wait(
        &self,
        parent: u32,
        child: u32,
        kind: &str,
        mode: &str,
        await_session: bool,
    ) -> Result<u64> {
        if !matches!(mode, "spawn" | "tell") || !matches!(kind, "codex" | "claude") {
            bail!("invalid subscription");
        }
        self.change(|j| {
            let id = j.next();
            j.subscriptions.insert(
                id,
                Subscription {
                    id,
                    parent,
                    parent_session: j
                        .sessions
                        .get(&parent)
                        .map(|s| s.hook_session.clone())
                        .unwrap_or_default(),
                    child,
                    child_kind: kind.into(),
                    child_generation: if await_session {
                        None
                    } else {
                        j.sessions.get(&child).map(|s| s.generation)
                    },
                    child_session: if await_session {
                        None
                    } else {
                        j.sessions.get(&child).map(|s| s.hook_session.clone())
                    },
                    await_session,
                    mode: mode.into(),
                    active: true,
                    reason: String::new(),
                },
            );
            Ok(id)
        })
    }
    pub fn release(&self, parent: u32, child: u32) -> Result<()> {
        self.change(|j| {
            let ids: Vec<_> = j
                .subscriptions
                .values()
                .filter(|s| s.active && s.parent == parent && s.child == child && s.mode == "spawn")
                .map(|s| s.id)
                .collect();
            for id in ids {
                j.close_subscription(id, "relation_released");
            }
            Ok(())
        })
    }
    pub fn unsubscribe(&self, id: u64, parent: u32) -> Result<()> {
        self.change(|j| {
            if !j.subscriptions.get(&id).is_some_and(|s| s.parent == parent) {
                bail!("subscription_not_owned");
            }
            j.close_subscription(id, "explicit_unsubscribe");
            Ok(())
        })
    }
    pub fn observe(&self, child: u32, state: &str, cause: &str, summary: &str) -> Result<()> {
        self.change(|j| {
            let session = j.sessions.entry(child).or_insert(Session {
                registration: "unobserved".into(),
                kind: "unknown".into(),
                hook_session: String::new(),
                generation: 0,
                state: "active".into(),
                epoch: 1,
            });
            if state == "active" && session.state != "active" {
                session.epoch += 1;
            }
            session.state = state.into();
            let epoch = session.epoch;
            if state == "active" {
                return Ok(());
            }
            let subscriptions: Vec<_> = j
                .subscriptions
                .values()
                .filter(|s| s.active && !s.await_session && s.child == child)
                .cloned()
                .collect();
            for sub in subscriptions {
                enqueue(j, &sub, epoch, state, cause, summary);
            }
            Ok(())
        })
    }
    pub fn end_execution(&self, surface: u32, cause: &str) -> Result<()> {
        if !self.snapshot()?.sessions.contains_key(&surface) {
            return Ok(());
        }
        self.observe(
            surface,
            "exited",
            cause,
            "Agent execution ended; success is not inferred",
        )?;
        self.change(|j| {
            for sub in j
                .subscriptions
                .values_mut()
                .filter(|s| s.active && s.child == surface && !s.await_session)
            {
                sub.active = false;
                sub.reason = "target_exited".into();
            }
            j.sessions.remove(&surface);
            for b in j.bindings.values_mut().filter(|b| b.surface == surface) {
                b.phase = "unbound".into();
                b.diagnostic = "session_ended: awaiting verified resume".into();
            }
            Ok(())
        })
    }
    pub fn exited(&self, surface: u32, cause: &str) -> Result<()> {
        self.observe(
            surface,
            "exited",
            cause,
            "Process ended; success is not inferred",
        )?;
        self.change(|j| {
            for sub in j
                .subscriptions
                .values_mut()
                .filter(|s| s.active && s.child == surface)
            {
                sub.active = false;
                sub.reason = "target_exited".into();
            }
            let ids: Vec<_> = j
                .subscriptions
                .values()
                .filter(|s| s.parent == surface)
                .map(|s| s.id)
                .collect();
            for id in ids {
                j.close_subscription(id, "parent_closed");
            }
            j.sessions.remove(&surface);
            for b in j.bindings.values_mut().filter(|b| b.surface == surface) {
                b.phase = "unbound".into();
                b.diagnostic = "parent_closed".into();
            }
            Ok(())
        })
    }
    pub fn retry(&self, id: u64, parent: u32) -> Result<()> {
        self.change(|j| {
            let e = j
                .events
                .get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("event_not_found"))?;
            let sub = j
                .subscriptions
                .get(&e.subscription)
                .ok_or_else(|| anyhow::anyhow!("subscription_not_found"))?;
            if sub.parent != parent || !sub.active {
                bail!("subscription_closed_or_not_owned");
            }
            if !matches!(e.phase.as_str(), "blocked" | "pending") {
                bail!("only_definitely_unsent_events_can_retry");
            }
            e.phase = "pending".into();
            e.retry_at = 0;
            e.attempts = 0;
            Ok(())
        })
    }
}
fn enqueue(
    j: &mut Journal,
    sub: &Subscription,
    epoch: u64,
    state: &str,
    cause: &str,
    summary: &str,
) {
    if j.sessions
        .get(&sub.parent)
        .is_some_and(|s| s.kind == "claude")
    {
        return;
    }
    if j.events.values().any(|e| {
        e.subscription == sub.id
            && e.child_generation == sub.child_generation
            && e.epoch == epoch
            && e.state == state
    }) {
        return;
    }
    let binding = j
        .bindings
        .values()
        .find(|b| b.surface == sub.parent && b.hook_session == sub.parent_session)
        .map(|b| b.key.clone());
    let id = j.next();
    j.events.insert(
        id,
        Event {
            child_generation: sub.child_generation,
            recovery: serde_json::Value::Null,
            delivery_id: format!("tasty-{}-{id}", j.instance),
            id,
            subscription: sub.id,
            phase: if binding.is_some() {
                "pending"
            } else {
                "unbound"
            }
            .into(),
            binding,
            epoch,
            state: state.into(),
            cause: cause.into(),
            child_kind: sub.child_kind.clone(),
            child: sub.child,
            diagnostic: String::new(),
            attempts: 0,
            retry_at: 0,
            summary: summary.chars().take(4096).collect(),
        },
    );
}
