//! Session identity and endpoint binding committed as one journal transition.
use super::*;
use anyhow::{Result, bail};
impl Completion {
    pub fn session(&self, surface: u32, kind: &str, hook_session: &str) -> Result<()> {
        self.change(|j| register_session(j, surface, kind, hook_session))
    }
    pub fn hook_session(&self, surface: u32, kind: &str, hook_session: &str) -> Result<bool> {
        self.change(|j| {
            if kind == "codex" {
                let current = j.sessions.get(&surface).map(|s| s.hook_session.clone());
                let mut requires_binding = false;
                for b in j.bindings.values_mut().filter(|b| b.phase != "superseded" && (b.hook_session == hook_session || (b.surface == surface && current.as_deref() == Some(b.hook_session.as_str())))) {
                    b.phase = "unbound".into();
                    b.diagnostic = "session_start_requires_binding: hook identity alone cannot prove the current TUI endpoint".into();
                    requires_binding = true;
                }
                if requires_binding { return Ok(false); }
            }
            register_session(j, surface, kind, hook_session)?;
            Ok(true)
        })
    }
    #[cfg(test)]
    pub fn bind(&self, binding: Binding) -> Result<()> {
        self.bind_register(binding, false)
    }
    pub fn bind_register(&self, mut binding: Binding, register: bool) -> Result<()> {
        transport::validate_endpoint(&binding.endpoint)?;
        self.change(|j| {
            if register {
                register_session(j, binding.surface, "codex", &binding.hook_session)?;
                j.sessions
                    .get_mut(&binding.surface)
                    .expect("registered above")
                    .registration = "explicit_bind: hook identity supplied by caller".into();
            }

            let session = j
                .sessions
                .get(&binding.surface)
                .ok_or_else(|| anyhow::anyhow!("session_start_required"))?;
            if session.kind != "codex" || session.hook_session != binding.hook_session {
                bail!("hook_session_mismatch");
            }
            if binding.thread_id.is_empty() || binding.session_id.is_empty() {
                bail!("thread_and_session_required");
            }
            if !binding
                .thread_id
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
            {
                bail!("invalid_thread_identifier");
            }
            // Remapping is an explicit binding to the same endpoint/thread, never a surface guess.
            if let Some(previous) = j.bindings.get(&binding.key) {
                if previous.hook_session != binding.hook_session {
                    bail!("binding_session_mismatch");
                }
                let old_surface = previous.surface;
                if old_surface != binding.surface
                    && j.sessions.get(&old_surface).is_some_and(|old| {
                        old.kind == "codex" && old.hook_session == binding.hook_session
                    })
                {
                    bail!("ambiguous_live_parent_surface");
                }
                for sub in j
                    .subscriptions
                    .values_mut()
                    .filter(|s| s.parent == old_surface && s.parent_session == binding.hook_session)
                {
                    sub.parent = binding.surface;
                }
            }
            for sub in j
                .subscriptions
                .values_mut()
                .filter(|s| s.parent == binding.surface && s.parent_session.is_empty())
            {
                sub.parent_session = binding.hook_session.clone();
            }
            let predecessors: Vec<_> = j
                .bindings
                .values()
                .filter(|b| {
                    b.surface == binding.surface
                        && b.key != binding.key
                        && b.hook_session == binding.hook_session
                        && b.phase != "superseded"
                })
                .cloned()
                .collect();
            for old in predecessors {
                if old.thread_id != binding.thread_id || old.session_id != binding.session_id {
                    bail!("explicit_rebind_must_preserve_logical_thread");
                }
                if !old.codex_home.is_empty() {
                    binding.codex_home = old.codex_home.clone();
                }
                let generation = j.next();
                let retired = j.bindings.get_mut(&old.key).expect("collected above");
                retired.phase = "superseded".into();
                retired.generation = generation;
                retired.diagnostic =
                    "explicit endpoint replacement; previous identity retained".into();
                for event in j.events.values_mut().filter(|e| {
                    e.binding.as_deref() == Some(&old.key)
                        && matches!(
                            e.phase.as_str(),
                            "pending"
                                | "blocked"
                                | "unbound"
                                | "accepted"
                                | "unknown"
                                | "in_flight"
                        )
                }) {
                    if event.origin_binding.is_none() {
                        event.origin_binding = Some(old.key.clone());
                    }
                    event.binding = Some(binding.key.clone());
                    // A request already claimed on the old connection may still be accepted.
                    if event.phase == "in_flight" {
                        event.phase = "unknown".into();
                    }
                }
            }
            for event in j.events.values_mut().filter(|e| e.phase == "unbound") {
                if j.subscriptions.get(&event.subscription).is_some_and(|s| {
                    s.parent == binding.surface && s.parent_session == binding.hook_session
                }) {
                    event.binding = Some(binding.key.clone());
                    event.phase = "pending".into();
                }
            }
            binding.generation = j.next();
            j.bindings.insert(binding.key.clone(), binding);
            Ok(())
        })
    }
}
fn register_session(j: &mut Journal, surface: u32, kind: &str, hook_session: &str) -> Result<()> {
    if !matches!(kind, "codex" | "claude") || hook_session.is_empty() {
        bail!("invalid agent session");
    }

    if j.sessions
        .get(&surface)
        .is_some_and(|s| s.kind == kind && s.hook_session == hook_session)
    {
        return Ok(());
    }
    let previous = j
        .sessions
        .get(&surface)
        .or_else(|| j.ended_sessions.get(&surface));
    let stale: Vec<_> = j
        .subscriptions
        .values()
        .filter(|s| {
            s.active
                && ((s.parent == surface
                    && !s.parent_session.is_empty()
                    && previous.is_some_and(|old| {
                        old.hook_session == s.parent_session
                            && (old.hook_session != hook_session || old.kind != kind)
                    }))
                    || (s.child == surface
                        && s.child_session.as_deref().is_some_and(|old| {
                            previous.is_some_and(|prior| {
                                prior.hook_session == old
                                    && (old != hook_session || prior.kind != kind)
                            })
                        })))
        })
        .map(|s| s.id)
        .collect();
    for id in stale {
        j.close_subscription(id, "session_replaced");
    }
    j.ended_sessions.remove(&surface);
    let generation = j.next();
    for sub in j.subscriptions.values_mut().filter(|s| s.active) {
        if sub.parent == surface && sub.parent_session.is_empty() {
            sub.parent_session = hook_session.into();
        }
        let restoring = sub.child_session.as_deref() == Some(hook_session)
            && sub.child_kind == kind
            && j.sessions
                .get(&sub.child)
                .is_none_or(|old| old.kind != kind || old.hook_session != hook_session);
        if restoring || (sub.child == surface && sub.child_session.is_none()) {
            sub.child = surface;
            sub.child_generation = Some(generation);
            sub.child_session = Some(hook_session.into());
            sub.await_session = false;
        }
    }
    if kind == "codex" {
        let ambiguous = j
            .sessions
            .iter()
            .any(|(sid, s)| *sid != surface && s.kind == kind && s.hook_session == hook_session);
        for b in j
            .bindings
            .values_mut()
            .filter(|b| b.hook_session == hook_session && b.phase != "superseded")
        {
            if ambiguous {
                b.phase = "unbound".into();
                b.diagnostic = "ambiguous_live_session".into();
                continue;
            }
            let old = b.surface;
            b.surface = surface;
            b.generation = generation;
            b.phase = "unbound".into();
            b.diagnostic =
                "session_registered: explicitly bind the current remote TUI endpoint".into();
            b.probe_attempts = 0;
            b.probe_after = 0;
            // Revalidate the recorded endpoint/thread after a positively observed session start.
            b.endpoint_identity.clear();
            b.daemon_pid = None;
            for sub in j
                .subscriptions
                .values_mut()
                .filter(|s| s.parent == old && s.parent_session == hook_session)
            {
                sub.parent = surface;
            }
        }
    }
    j.sessions.insert(
        surface,
        Session {
            registration: "hook".into(),
            kind: kind.into(),
            hook_session: hook_session.into(),
            generation,
            state: "active".into(),
            epoch: 1,
        },
    );
    Ok(())
}
