//! Background delivery: durable claim precedes network write; uncertain writes never retry.
use super::{protocol::Client, *};
use anyhow::Result;
use std::time::Duration;

pub fn start(service: Weak<Completion>) -> Result<()> {
    std::thread::Builder::new()
        .name("child-completion".into())
        .spawn(move || {
            loop {
                std::thread::sleep(Duration::from_millis(500));
                let Some(service) = service.upgrade() else {
                    break;
                };
                if let Err(error) = tick(&service) {
                    tracing::warn!("completion worker: {error:#}");
                }
            }
        })?;
    Ok(())
}
pub fn tick(service: &Completion) -> Result<()> {
    let snapshot = service.snapshot()?;
    for binding in snapshot
        .bindings
        .values()
        .filter(|b| (b.phase == "verifying" || b.phase == "verified") && b.probe_after <= now())
    {
        let event = snapshot.events.values().find(|e| {
            e.binding.as_deref() == Some(&binding.key)
                && (e.retry_at <= now()
                    && matches!(e.phase.as_str(), "pending" | "unknown" | "accepted"))
        });
        if event.is_none() && binding.phase == "verified" {
            continue;
        }
        let cli_version = super::diagnostics::cli_version();
        match Client::connect(binding) {
            Ok((mut client, identity)) => {
                let endpoint_identity = super::transport::endpoint_identity(&binding.endpoint)?;
                let still_current = service.change(|j| {
                    let owner_live = j.sessions.get(&binding.surface).is_some_and(|s| {
                        s.kind == "codex" && s.hook_session == binding.hook_session
                    });
                    if let Some(b) = j.bindings.get_mut(&binding.key)
                        && owner_live
                        && matches!(b.phase.as_str(), "verifying" | "verified")
                        && b.generation == binding.generation
                        && b.surface == binding.surface
                        && b.hook_session == binding.hook_session
                    {
                        b.phase = "verified".into();
                        b.server = identity.server;
                        b.codex_home = identity.home;
                        b.daemon_pid = Some(identity.pid);
                        b.history_mode = identity.history;
                        b.diagnostic = String::new();
                        b.endpoint_identity = endpoint_identity;
                        b.connection_generation += 1;
                        b.probe_attempts = 0;
                        b.probe_after = 0;
                        b.cli_version = cli_version;
                        return Ok(true);
                    }
                    Ok(false)
                })?;
                if !still_current {
                    continue;
                }
                if let Some(event) = event {
                    deliver(service, &mut client, binding, event)?;
                }
            }
            Err(error) => {
                let diagnostic = format!("endpoint_verification_failed: {error:#}");
                service.change(|j| {
                    if !j.bindings.get(&binding.key).is_some_and(|b| {
                        b.generation == binding.generation && b.surface == binding.surface
                    }) {
                        return Ok(());
                    }
                    if let Some(b) = j.bindings.get_mut(&binding.key) {
                        b.diagnostic = diagnostic.clone();
                        b.cli_version = cli_version;
                        b.probe_attempts += 1;
                        b.probe_after = now() + 2_u64.pow(b.probe_attempts.min(6)).min(60);
                        if b.probe_attempts >= 8
                            || diagnostic.contains("identity_mismatch")
                            || diagnostic.contains("unsupported_hook_thread_mapping")
                            || diagnostic.contains("unsupported_server_version")
                            || diagnostic.contains("endpoint_replaced")
                            || diagnostic.contains("daemon_replaced")
                            || diagnostic.contains("daemon_home_changed")
                            || diagnostic.contains("codex_home_mismatch")
                            || diagnostic.contains("rpc_error:")
                            || diagnostic.contains("rejected_http_401")
                            || diagnostic.contains("rejected_http_403")
                            || diagnostic.contains("tls_verification_failed")
                        {
                            b.phase = "unbound".into();
                        }
                    }
                    if let Some(event) = event
                        && let Some(e) = j.events.get_mut(&event.id)
                    {
                        if e.phase == "pending" {
                            defer(e, &diagnostic);
                        } else {
                            e.diagnostic = diagnostic;
                            e.retry_at = now() + 10;
                        }
                    }
                    Ok(())
                })?;
            }
        }
    }
    Ok(())
}
fn deliver(
    service: &Completion,
    client: &mut Client,
    binding: &Binding,
    event: &Event,
) -> Result<()> {
    if matches!(event.phase.as_str(), "unknown" | "accepted") {
        let evidence = client.recorded(binding, event);
        return service.change(|j| {
            if let Some(e) = j.events.get_mut(&event.id) {
                e.retry_at = now() + 10;
                match evidence {
                    Ok(scan) if scan.matched => {
                        e.recovery = scan.evidence;
                        e.phase = "recorded".into();
                        e.diagnostic =
                            "persisted functionCallOutput observed; model consumption not inferred"
                                .into();
                    }
                    Ok(scan) => {
                        e.diagnostic = if scan.evidence["complete"] == true {
                            "not_in_history: busy queue acceptance remains unresolved"
                        } else {
                            "history_incomplete: acceptance remains unresolved"
                        }.into();
                        e.recovery = scan.evidence;
                    }
                    Err(error) => {
                        e.diagnostic = format!("history_unavailable: {error:#}");
                        e.recovery = serde_json::json!({"complete":false,"error":e.diagnostic,"absence_proves_rejection":false});
                    }
                }
            }
            Ok(())
        });
    }
    let claimed = service.change(|j| {
        let Some(e) = j.events.get(&event.id) else {
            return Ok(false);
        };
        if e.phase != "pending" {
            return Ok(false);
        }
        let sub = &j.subscriptions[&e.subscription];
        if !sub.accepts_pending() {
            return Ok(false);
        }
        let Some(b) = j.bindings.get(&binding.key) else {
            return Ok(false);
        };
        if b.phase != "verified"
            || b.surface != binding.surface
            || b.generation != binding.generation
        {
            return Ok(false);
        }
        if !j
            .sessions
            .get(&b.surface)
            .is_some_and(|s| s.kind == "codex" && s.hook_session == b.hook_session)
        {
            return Ok(false);
        }
        let context = serde_json::json!({"binding":b.key,"binding_generation":b.generation,"endpoint":b.endpoint,"daemon_pid":b.daemon_pid,"codex_home":b.codex_home,"connection_generation":b.connection_generation,"auth_reference":b.auth_env});
        let e = j.events.get_mut(&event.id).expect("checked above");
        e.delivery_context = context;
        e.phase = "in_flight".into();
        e.attempts += 1;
        Ok(true)
    })?;
    if !claimed {
        return Ok(());
    }
    let (id, wire) = client.prepare_output(binding, event);
    // From this point a partial write or lost response is acceptance-unknown.
    let result = client
        .transport
        .send(&wire)
        .and_then(|()| client.response(id))
        .and_then(|reply| {
            if reply["turn"]["id"].as_str().is_some() && reply["turn"]["status"].as_str().is_some()
            {
                Ok(reply)
            } else {
                anyhow::bail!("turn_response_invalid: acceptance cannot be inferred")
            }
        });
    service.change(|j| {
        let e = j
            .events
            .get_mut(&event.id)
            .expect("journal events are retained");
        match result {
            Ok(_) => {
                e.phase = "accepted".into();
                e.diagnostic = "server returned turn; persistence/consumption not inferred".into();
            }
            Err(error) => {
                let detail = format!("{error:#}");
                e.phase = if detail.starts_with("rpc_error:") {
                    if j.subscriptions
                        .get(&e.subscription)
                        .is_some_and(|s| !s.accepts_pending())
                    {
                        "cancelled"
                    } else {
                        "blocked"
                    }
                } else {
                    "unknown"
                }
                .into();
                e.diagnostic = detail;
            }
        }
        Ok(())
    })
}
fn defer(event: &mut Event, reason: &str) {
    event.attempts += 1;
    event.diagnostic = reason.into();
    if event.attempts >= 8 {
        event.phase = "blocked".into();
    } else {
        event.retry_at = now() + 2_u64.pow(event.attempts).min(60);
    }
}
