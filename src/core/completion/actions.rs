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
            if j.close_blocks_surface(parent) || j.close_blocks_surface(child) {
                bail!("close_persistence_pending");
            }
            let relation_generation = if mode == "spawn" {
                Some(j.current_relation(parent, child))
            } else {
                None
            };
            let id = j.next();
            j.subscriptions.insert(
                id,
                Subscription {
                    id,
                    relation_generation,
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
            // Identity may arrive after the first restored watch. Reconcile in
            // this same commit, without requiring another subscribe. Explicit
            // spawn/adopt relationships must never import an earlier generation.
            if j.live_relations
                .get(&(parent, child))
                .is_some_and(|relation| relation.restored)
            {
                j.current_relation(parent, child);
            }
            let ids: Vec<_> = j
                .subscriptions
                .values()
                .filter(|s| {
                    s.accepts_pending()
                        && s.parent == parent
                        && s.child == child
                        && s.mode == "spawn"
                        && match j.live_relations.get(&(parent, child)) {
                            Some(relation) => s.relation_generation == Some(relation.generation),
                            // Restored relations require known logical owners. A new
                            // current relation above must never cancel an old generation.
                            None => {
                                j.owns_subscription(s, parent)
                                    && j.sessions
                                        .get(&child)
                                        .or_else(|| j.ended_sessions.get(&child))
                                        .map_or(s.child_session.is_none(), |current| {
                                            s.matches_child(current)
                                        })
                            }
                        }
                })
                .map(|s| s.id)
                .collect();
            for id in ids {
                j.close_subscription(id, "relation_released");
            }
            j.live_relations.remove(&(parent, child));
            Ok(())
        })
    }
    pub fn unsubscribe(&self, id: u64, parent: u32) -> Result<()> {
        self.change(|j| {
            if j.subscriptions
                .get(&id)
                .is_none_or(|s| !j.owns_subscription(s, parent))
            {
                bail!("subscription_not_owned");
            }
            j.close_subscription(id, "explicit_unsubscribe");
            Ok(())
        })
    }
    pub fn observe(&self, child: u32, state: &str, cause: &str, summary: &str) -> Result<()> {
        self.change(|j| record_observation(j, child, state, cause, summary))
    }
    pub fn observe_session(
        &self,
        child: u32,
        state: &str,
        cause: &str,
        summary: &str,
        expected: Option<&str>,
    ) -> Result<bool> {
        self.change(|j| {
            if j.close_blocks_surface(child) {
                return Ok(false);
            }
            // Retired unversioned error callbacks cannot prove an execution identity.
            if cause == "claude-error-stalled" && expected.is_none() {
                return Ok(false);
            }
            if let Some(expected) = expected {
                if j.ended_sessions.contains_key(&child)
                    || j.sessions
                        .get(&child)
                        .is_some_and(|s| !s.hook_session.is_empty() && s.hook_session != expected)
                {
                    return Ok(false);
                }
            }
            record_observation(j, child, state, cause, summary)?;
            Ok(true)
        })
    }
    pub fn end_execution(&self, surface: u32, cause: &str) -> Result<()> {
        self.end_execution_session(surface, cause, None).map(|_| ())
    }
    pub fn end_execution_session(
        &self,
        surface: u32,
        cause: &str,
        expected: Option<&str>,
    ) -> Result<bool> {
        self.change(|j| {
            let identity = j
                .sessions
                .get(&surface)
                .filter(|s| !s.hook_session.is_empty())
                .or_else(|| j.ended_sessions.get(&surface));
            if expected.is_some_and(|expected| {
                identity.is_some_and(|current| current.hook_session != expected)
            }) {
                return Ok(false);
            }
            if !j.sessions.contains_key(&surface) {
                return Ok(true);
            }
            record_observation(
                j,
                surface,
                "exited",
                cause,
                "Agent execution ended; success is not inferred",
            )?;
            let current = observed_identity(j, surface);
            for sub in j.subscriptions.values_mut().filter(|s| {
                s.active && s.child == surface && !s.await_session && s.matches_child(&current)
            }) {
                sub.active = false;
                sub.reason = "target_exited".into();
            }
            j.sessions.remove(&surface);
            j.ended_sessions.insert(surface, current.clone());
            for b in j.bindings.values_mut().filter(|b| {
                b.surface == surface
                    && b.hook_session == current.hook_session
                    && b.phase != "superseded"
            }) {
                b.phase = "unbound".into();
                b.diagnostic = "session_ended: awaiting verified resume".into();
            }
            Ok(true)
        })
    }
    pub fn retry(&self, id: u64, parent: u32) -> Result<()> {
        self.change(|j| {
            if j.events.get(&id).is_some_and(|event| j.close_blocks_subscription(event.subscription)) {
                bail!("close_persistence_pending");
            }
            let e = j
                .events
                .get_mut(&id)
                .ok_or_else(|| anyhow::anyhow!("event_not_found"))?;
            let sub = j
                .subscriptions
                .get(&e.subscription)
                .ok_or_else(|| anyhow::anyhow!("subscription_not_found"))?;
            if sub.parent != parent || !sub.accepts_pending() {
                bail!("subscription_closed_or_not_owned");
            }
            if !matches!(e.phase.as_str(), "blocked" | "pending") {
                bail!("only_definitely_unsent_events_can_retry");
            }
            if !e.binding.as_ref().and_then(|key| j.bindings.get(key)).is_some_and(|b| matches!(b.phase.as_str(), "verified" | "verifying")) {
                bail!("binding_unbound: explicitly bind the recorded parent after correcting its diagnostic");
            }
            e.phase = "pending".into();
            e.retry_at = 0;
            e.attempts = 0;
            Ok(())
        })
    }
}
fn observed_identity(j: &Journal, surface: u32) -> Session {
    let current = &j.sessions[&surface];
    if current.hook_session.is_empty() {
        j.ended_sessions.get(&surface).unwrap_or(current).clone()
    } else {
        current.clone()
    }
}

pub(super) fn record_observation(
    j: &mut Journal,
    child: u32,
    state: &str,
    cause: &str,
    summary: &str,
) -> Result<()> {
    if j.close_blocks_surface(child) {
        return Ok(());
    }
    if !matches!(
        state,
        "active" | "idle" | "needs_input" | "stalled" | "exited"
    ) {
        bail!("invalid_completion_state");
    }
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
    let current = session.clone();
    if state == "active" {
        return Ok(());
    }
    let subscriptions: Vec<_> = j
        .subscriptions
        .values()
        .filter(|s| s.active && !s.await_session && s.child == child && s.matches_child(&current))
        .cloned()
        .collect();
    for sub in subscriptions {
        enqueue(j, &sub, epoch, state, cause, summary);
    }
    Ok(())
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
        .find(|b| {
            b.surface == sub.parent
                && b.hook_session == sub.parent_session
                && b.phase != "superseded"
        })
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
            origin_binding: None,
            delivery_context: serde_json::Value::Null,
            server_response: serde_json::Value::Null,
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

pub(super) fn close_transition(j: &mut Journal, surface: u32, cause: &str) -> Result<()> {
    j.live_relations
        .retain(|(parent, child), _| *parent != surface && *child != surface);
    record_observation(
        j,
        surface,
        "exited",
        cause,
        "Process ended; success is not inferred",
    )?;
    let current = observed_identity(j, surface);
    for sub in j
        .subscriptions
        .values_mut()
        .filter(|s| s.active && s.child == surface && s.matches_child(&current))
    {
        sub.active = false;
        sub.reason = "target_exited".into();
    }
    let ids: Vec<_> = j
        .subscriptions
        .values()
        .filter(|s| s.parent == surface && s.parent_session == current.hook_session)
        .map(|s| s.id)
        .collect();
    for id in ids {
        j.close_subscription(id, "parent_closed");
    }
    j.sessions.remove(&surface);
    j.ended_sessions.remove(&surface);
    for b in j.bindings.values_mut().filter(|b| {
        b.surface == surface && b.hook_session == current.hook_session && b.phase != "superseded"
    }) {
        b.phase = "unbound".into();
        b.diagnostic = "parent_closed".into();
    }
    Ok(())
}
