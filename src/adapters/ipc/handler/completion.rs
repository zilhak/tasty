//! CLI/plugin surface for the host-owned completion journal.
use crate::core::{CoreState, completion::Binding};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use tasty_ipc::protocol::JsonRpcResponse;

pub(super) fn handle(
    engine: &mut CoreState,
    id: Value,
    params: &Value,
    binding_permission: bool,
) -> JsonRpcResponse {
    if params["action"] == "bind" && !binding_permission {
        return JsonRpcResponse::error(
            id,
            -32001,
            "Use terminal.completion_bind with network permission",
        );
    }
    match execute(engine, params) {
        Ok(value) => JsonRpcResponse::success(id, value),
        Err(error) => JsonRpcResponse::error(id, -32000, format!("completion: {error:#}")),
    }
}
fn number<T: TryFrom<u64>>(params: &Value, name: &str) -> Result<T> {
    super::params::read_int(params, name)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("missing {name}"))
}
fn text<'a>(params: &'a Value, name: &str) -> Result<&'a str> {
    params[name]
        .as_str()
        .filter(|s| !s.is_empty())
        .with_context(|| format!("missing {name}"))
}
fn execute(engine: &mut CoreState, params: &Value) -> Result<Value> {
    let action = text(params, "action")?;
    if matches!(action, "status" | "diagnose") && params["all"] == true {
        let j = engine.completion.snapshot()?;
        return Ok(
            json!({"sessions":j.sessions,"bindings":j.bindings.into_values().collect::<Vec<_>>(),"subscriptions":j.subscriptions.into_values().collect::<Vec<_>>(),"events":j.events.into_values().collect::<Vec<_>>(),"scope":"host journal including closed parents"}),
        );
    }
    let surface = number(params, "surface")?;
    // Existing sessions from before a host/plugin upgrade need positive process evidence.
    if !engine
        .completion
        .snapshot()?
        .sessions
        .contains_key(&surface)
    {
        let kind = engine.foreground_name(surface).and_then(|name| {
            if name.to_ascii_lowercase().contains("claude") {
                Some("claude")
            } else if name.to_ascii_lowercase().contains("codex") {
                Some("codex")
            } else {
                None
            }
        });
        if let Some(kind) = kind {
            let session = crate::surface_meta::SurfaceMetaStore::get(
                &mut *engine
                    .memory
                    .lock()
                    .map_err(|_| anyhow::anyhow!("metadata poisoned"))?,
                surface,
                &format!("{kind}-session-id"),
            );
            if let Some(session) = session {
                engine.completion.session(surface, kind, &session)?;
            }
        }
    }
    let service = &engine.completion;
    match action {
        "session" => {
            service.session(
                surface,
                text(params, "kind")?,
                text(params, "hook_session")?,
            )?;
            Ok(json!({"registered":true}))
        }
        "subscribe" => {
            let target = number(params, "target")?;
            let mode = text(params, "mode")?;
            if mode == "spawn" && engine.child_terminals.parent_of_child(target) != Some(surface) {
                bail!("child_relation_required");
            }
            let id = service.subscribe_wait(
                surface,
                target,
                text(params, "kind")?,
                mode,
                params["await_session"].as_bool().unwrap_or(false),
            )?;
            Ok(json!({"subscription":id}))
        }
        "end_session" => {
            let ended = service.end_execution_session(
                surface,
                "session-end",
                params["hook_session"].as_str(),
            )?;
            Ok(json!({"ended":ended,"ignored_old_session":!ended}))
        }
        "observe" => {
            let recorded = service.observe_session(
                surface,
                text(params, "state")?,
                text(params, "cause")?,
                params["summary"].as_str().unwrap_or(""),
                params["hook_session"].as_str(),
            )?;
            Ok(json!({"recorded":recorded,"ignored_old_session":!recorded}))
        }
        "route" => {
            let snapshot = service.snapshot()?;
            let kind = snapshot
                .sessions
                .get(&surface)
                .map(|s| s.kind.as_str())
                .unwrap_or("unknown");
            Ok(json!({"legacy_log":kind=="claude","parent_kind":kind}))
        }
        "bind" => {
            let register = params["register"].as_bool().unwrap_or(false);
            if register && engine.find_surface_by_id(surface).is_none() {
                bail!("parent_surface_missing");
            }
            let endpoint = text(params, "endpoint")?;
            let thread = text(params, "thread_id")?;
            let key = format!("{endpoint}#{thread}");
            service.bind_register(
                Binding {
                    generation: 0,
                    key: key.clone(),
                    surface,
                    hook_session: text(params, "hook_session")?.into(),
                    endpoint: endpoint.into(),
                    auth_env: params["auth_env"].as_str().map(str::to_string),
                    thread_id: thread.into(),
                    session_id: text(params, "session_id")?.into(),
                    phase: "verifying".into(),
                    diagnostic: "verification_queued".into(),
                    server: String::new(),
                    cli_version: String::new(),
                    codex_home: String::new(),
                    expected_home: params["codex_home"].as_str().map(str::to_string),
                    daemon_pid: None,
                    endpoint_identity: String::new(),
                    connection_generation: 0,
                    probe_attempts: 0,
                    probe_after: 0,
                    history_mode: String::new(),
                },
                register,
            )?;
            Ok(json!({"binding":key,"phase":"verifying"}))
        }
        "resume_context" => {
            let j = service.snapshot()?;
            let found = j
                .bindings
                .values()
                .find(|b| b.surface == surface && b.phase == "verified");
            Ok(json!({"binding":found}))
        }
        "status" | "diagnose" => {
            let j = service.snapshot()?;
            let subscriptions: Vec<_> = j
                .subscriptions
                .values()
                .filter(|s| s.parent == surface)
                .collect();
            let events: Vec<_> = j
                .events
                .values()
                .filter(|e| subscriptions.iter().any(|s| s.id == e.subscription))
                .collect();
            let bindings: Vec<_> = j
                .bindings
                .values()
                .filter(|b| b.surface == surface)
                .collect();
            Ok(
                json!({"session":j.sessions.get(&surface),"bindings":bindings,"subscriptions":subscriptions,"events":events,"transport":"existing endpoint only; no PTY fallback","consumption":"not observable from acceptance alone","capabilities":{"experimentalApi":true,"optOutNotificationMethods":[],"reason":"server/diagnostics identity verification and history recovery","toolOutput":"support determined by each send response; binding alone does not prove support","history":"actual historyMode and page results are retained in event recovery","subscription":"thread/resume on each connection; disconnected gaps require history reconciliation"}}),
            )
        }
        "retry" => {
            service.retry(number(params, "event")?, surface)?;
            Ok(json!({"queued":true}))
        }
        "unsubscribe" => {
            service.unsubscribe(number(params, "subscription")?, surface)?;
            Ok(json!({"unsubscribed":true}))
        }
        _ => bail!("unsupported completion action"),
    }
}
