mod agent;
mod debug;
mod memory;
mod telemetry;

use agent::agent_command_to_method_params;
#[cfg(debug_assertions)]
use debug::debug_command_to_method_params;
use memory::memory_command_to_method_params;
use telemetry::telemetry_command_to_method_params;

use super::{
    ApprovalCommands, ClipboardCommands, CloseCommands, Commands, FileHandlerCommands,
    ListCommands, MoveCommands, NewCommands, OutputCommands, OutputObserveCommands,
    PluginCommands, PresetCommands, ReadCommands, ScriptCommands, SendCommands, SetCommands,
    SurfaceMetaCommands, ToolCommands, UnsetCommands,
};
use crate::ipc::protocol::JsonRpcRequest;

/// Resolve a target string for split/other commands.
/// - "this" → numeric surface ID from TASTY_SURFACE_ID env var
/// - numeric string → passed through as-is
/// - other string → passed through as-is (server resolves as nickname)
fn resolve_target(target: &str) -> String {
    if target == "this" {
        std::env::var("TASTY_SURFACE_ID").unwrap_or_else(|_| target.to_string())
    } else {
        target.to_string()
    }
}

/// Interpret C-style escape sequences: \r \n \t \\ \0
fn unescape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('r') => out.push('\r'),
                Some('n') => out.push('\n'),
                Some('t') => out.push('\t'),
                Some('\\') => out.push('\\'),
                Some('0') => out.push('\0'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Get surface_id: explicit value > TASTY_SURFACE_ID env var.
pub(super) fn resolve_surface_id(explicit: Option<u32>) -> Option<u32> {
    explicit.or_else(|| std::env::var("TASTY_SURFACE_ID").ok()?.parse().ok())
}

pub fn command_to_request(command: &Commands) -> JsonRpcRequest {
    let (method, params) = match command {
        // ── grouped ──
        Commands::New { command } => new_command_to_method_params(command),
        Commands::Close { command } => close_command_to_method_params(command),
        Commands::List { command } => list_command_to_method_params(command),
        Commands::Set { command } => set_command_to_method_params(command),
        Commands::Move { command } => move_command_to_method_params(command),
        #[cfg(debug_assertions)]
        Commands::Debug { command } => debug_command_to_method_params(command),
        // ── standalone ──
        Commands::Split {
            level,
            target_surface,
            target_pane,
            direction,
            r#type,
            meta,
            cwd,
            file,
            path,
            url,
        } => {
            let meta_value = meta
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());

            let ts = target_surface
                .as_ref()
                .map(|s| serde_json::Value::String(resolve_target(s)));
            let tp = target_pane.map(|p| serde_json::Value::Number(p.into()));

            (
                "split",
                serde_json::json!({
                    "level": level,
                    "target_surface": ts,
                    "target_pane": tp,
                    "direction": direction,
                    "type": r#type,
                    "meta": meta_value,
                    "cwd": cwd,
                    "file": file,
                    "path": path,
                    "url": url,
                }),
            )
        }
        Commands::Send { command } => send_command_to_method_params(command),
        Commands::Read { command } => read_command_to_method_params(command),
        Commands::Notify { body, title } => (
            "notification.create",
            serde_json::json!({ "title": title, "body": body }),
        ),
        Commands::Unset { command } => unset_command_to_method_params(command),
        Commands::SurfaceMeta { command } => surface_meta_command_to_method_params(command),
        Commands::IsTyping { surface } => (
            "surface.is_typing",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
        ),
        Commands::Wake { surface } => (
            "surface.wake",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
        ),
        Commands::Tool { command } => tool_command_to_method_params(command),
        Commands::Plugin { command } => plugin_command_to_method_params(command),
        Commands::Memory { command } => memory_command_to_method_params(command),
        Commands::Output { command } => output_command_to_method_params(command),
        Commands::Approval { command } => approval_command_to_method_params(command),
        Commands::Telemetry { command } => telemetry_command_to_method_params(command),
        Commands::Agent { command } => agent_command_to_method_params(command),
        Commands::FileHandler { command } => file_handler_command_to_method_params(command),
        Commands::Script { command } => script_command_to_method_params(command),
        Commands::Preset { command } => preset_command_to_method_params(command),
    };

    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
        // Phase 6.2 — 자식 agent 가 띄운 CLI 가 env 로 받은 session token 을
        // envelope 에 자동 첨부. 호스트는 이를 검증해 CallerContext::Agent 로 분기.
        session_token: std::env::var("TASTY_SESSION_TOKEN").ok().filter(|s| !s.is_empty()),
    }
}

fn new_command_to_method_params(command: &NewCommands) -> (&'static str, serde_json::Value) {
    match command {
        NewCommands::Window => ("window.create", serde_json::json!({})),
        NewCommands::Workspace {
            name,
            cwd,
            r#type,
            file,
            path,
            url,
        } => (
            "workspace.create",
            serde_json::json!({
                "name": name.as_deref().unwrap_or(""),
                "cwd": cwd,
                "type": r#type,
                "file": file,
                "path": path,
                "url": url,
            }),
        ),
        NewCommands::Tab {
            pane,
            r#type,
            cwd,
            file,
            path,
            url,
        } => (
            "tab.create",
            serde_json::json!({
                "pane_id": pane,
                "type": r#type,
                "cwd": cwd,
                "file": file,
                "path": path,
                "url": url,
            }),
        ),
    }
}

fn close_command_to_method_params(command: &CloseCommands) -> (&'static str, serde_json::Value) {
    let caller = resolve_surface_id(None); // TASTY_SURFACE_ID
    match command {
        CloseCommands::Tab { tab } => (
            "tab.close",
            serde_json::json!({ "tab_id": tab, "caller_surface_id": caller }),
        ),
        CloseCommands::Pane { pane } => (
            "pane.close",
            serde_json::json!({ "pane_id": pane, "caller_surface_id": caller }),
        ),
        CloseCommands::Surface { surface } => (
            "surface.close",
            serde_json::json!({ "surface_id": surface, "caller_surface_id": caller }),
        ),
        CloseCommands::CloseSelf => match caller {
            Some(sid) => (
                "surface.close_self",
                serde_json::json!({ "surface_id": sid }),
            ),
            None => {
                eprintln!(
                    "Error: TASTY_SURFACE_ID not set. 'close self' can only be used inside a tasty terminal."
                );
                std::process::exit(1);
            }
        },
    }
}

fn list_command_to_method_params(command: &ListCommands) -> (&'static str, serde_json::Value) {
    match command {
        ListCommands::Workspaces => ("workspace.list", serde_json::json!({})),
        ListCommands::Windows => ("window.list", serde_json::json!({})),
        ListCommands::Tree => ("tree", serde_json::json!({})),
        ListCommands::Surfaces => ("surface.list", serde_json::json!({})),
        ListCommands::Panes => ("pane.list", serde_json::json!({})),
        ListCommands::Tabs { pane } => ("tab.list", serde_json::json!({ "pane_id": pane })),
        ListCommands::Info => ("system.info", serde_json::json!({})),
        ListCommands::Notifications => ("notification.list", serde_json::json!({})),
        ListCommands::Hooks { surface } => (
            "hook.list",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
        ),
        ListCommands::GlobalHooks => ("global_hook.list", serde_json::json!({})),
        ListCommands::Queue { surface } => (
            "message.count",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
        ),
    }
}

fn send_command_to_method_params(command: &SendCommands) -> (&'static str, serde_json::Value) {
    match command {
        SendCommands::Text { text, surface } => (
            "surface.send",
            serde_json::json!({ "text": unescape(text), "surface_id": resolve_surface_id(*surface) }),
        ),
        SendCommands::Key { key, surface } => (
            "surface.send_key",
            serde_json::json!({ "key": key, "surface_id": resolve_surface_id(*surface) }),
        ),
        SendCommands::Queue { to, content, from } => (
            "message.send",
            serde_json::json!({
                "to_surface_id": to,
                "content": content,
                "from_surface_id": resolve_surface_id(*from),
            }),
        ),
    }
}

fn read_command_to_method_params(command: &ReadCommands) -> (&'static str, serde_json::Value) {
    match command {
        ReadCommands::SinceMark {
            surface,
            strip_ansi,
        } => (
            "surface.read_since_mark",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "strip_ansi": strip_ansi,
            }),
        ),
        ReadCommands::ParseSinceMark { surface, parsers } => {
            let mut params = serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
            });
            if let Some(ids) = parsers {
                params["parsers"] = serde_json::Value::Array(
                    ids.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
                );
            }
            ("surface.parse_since_mark", params)
        }
        ReadCommands::Queue {
            surface,
            from,
            peek,
            clear,
        } => {
            if *clear {
                (
                    "message.clear",
                    serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
                )
            } else {
                (
                    "message.read",
                    serde_json::json!({
                        "surface_id": resolve_surface_id(*surface),
                        "from_surface_id": from,
                        "peek": peek,
                    }),
                )
            }
        }
        ReadCommands::Screen { surface, lines } => (
            "surface.screen_text",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "lines": lines,
            }),
        ),
        ReadCommands::Commands {
            surface,
            limit,
            since,
        } => {
            let mut params = serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
            });
            if let Some(n) = limit {
                params["limit"] = serde_json::Value::from(*n);
            }
            if let Some(ts) = since {
                params["since"] = serde_json::Value::from(*ts);
            }
            ("surface.commands", params)
        }
        ReadCommands::LastCommand { surface } => (
            "surface.last_command",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
            }),
        ),
        ReadCommands::CommandAt { surface, index } => (
            "surface.command_at",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "index": index,
            }),
        ),
    }
}

fn set_command_to_method_params(command: &SetCommands) -> (&'static str, serde_json::Value) {
    match command {
        SetCommands::Hook {
            surface,
            event,
            command,
            once,
        } => (
            "hook.set",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "event": event,
                "command": command,
                "once": once,
            }),
        ),
        SetCommands::Mark { surface } => (
            "surface.set_mark",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
        ),
        SetCommands::Workspace {
            id,
            name,
            subtitle,
            description,
        } => (
            "workspace.update",
            serde_json::json!({
                "id": id,
                "name": name,
                "subtitle": subtitle,
                "description": description,
            }),
        ),
        SetCommands::GlobalHook {
            condition,
            command,
            label,
        } => (
            "global_hook.set",
            serde_json::json!({
                "condition": condition,
                "command": command,
                "label": label,
            }),
        ),
    }
}

fn move_command_to_method_params(command: &MoveCommands) -> (&'static str, serde_json::Value) {
    match command {
        MoveCommands::Tab { pane, from, to } => (
            "tab.move",
            serde_json::json!({
                "pane_id": pane,
                "from_index": from,
                "to_index": to,
            }),
        ),
        MoveCommands::Workspace { from, to } => (
            "workspace.move",
            serde_json::json!({
                "from_index": from,
                "to_index": to,
            }),
        ),
    }
}

fn unset_command_to_method_params(command: &UnsetCommands) -> (&'static str, serde_json::Value) {
    match command {
        UnsetCommands::Hook { hook } => ("hook.unset", serde_json::json!({ "hook_id": hook })),
        UnsetCommands::GlobalHook { hook } => {
            ("global_hook.unset", serde_json::json!({ "hook_id": hook }))
        }
    }
}

#[cfg(debug_assertions)]
fn surface_meta_command_to_method_params(
    command: &SurfaceMetaCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        SurfaceMetaCommands::Set {
            key,
            value,
            surface,
        } => (
            "surface.meta.set",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "key": key,
                "value": value,
            }),
        ),
        SurfaceMetaCommands::Get { key, surface } => (
            "surface.meta.get",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "key": key,
            }),
        ),
        SurfaceMetaCommands::Unset { key, surface } => (
            "surface.meta.unset",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "key": key,
            }),
        ),
        SurfaceMetaCommands::List { surface } => (
            "surface.meta.list",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
            }),
        ),
    }
}

fn tool_command_to_method_params(command: &ToolCommands) -> (&'static str, serde_json::Value) {
    match command {
        ToolCommands::Clipboard { command } => match command {
            ClipboardCommands::List { limit } => {
                ("tool.clipboard.list", serde_json::json!({ "limit": limit }))
            }
            ClipboardCommands::Get { index } => {
                ("tool.clipboard.get", serde_json::json!({ "index": index }))
            }
            ClipboardCommands::Paste { index } => (
                "tool.clipboard.paste",
                serde_json::json!({ "index": index }),
            ),
            ClipboardCommands::Remove { index } => (
                "tool.clipboard.remove",
                serde_json::json!({ "index": index }),
            ),
            ClipboardCommands::Clear => ("tool.clipboard.clear", serde_json::json!({})),
        },
    }
}

fn plugin_command_to_method_params(command: &PluginCommands) -> (&'static str, serde_json::Value) {
    match command {
        PluginCommands::List => ("plugin.list", serde_json::json!({})),
        PluginCommands::Show { id } => ("plugin.show", serde_json::json!({ "id": id })),
        PluginCommands::Install { path } => (
            "plugin.install",
            serde_json::json!({ "path": path }),
        ),
        PluginCommands::Remove { id } => ("plugin.remove", serde_json::json!({ "id": id })),
        PluginCommands::Enable { id } => ("plugin.enable", serde_json::json!({ "id": id })),
        PluginCommands::Disable { id } => ("plugin.disable", serde_json::json!({ "id": id })),
        // Logs는 IPC를 거치지 않음 — run_client에서 special-case로 처리.
        PluginCommands::Logs { .. } => ("plugin.list", serde_json::json!({})),
        // Doctor도 IPC를 거치지 않음 — manifest 를 로컬에서 직접 읽는다.
        PluginCommands::Doctor { .. } => ("plugin.list", serde_json::json!({})),
        PluginCommands::Permissions { id } => (
            "plugin.permissions",
            serde_json::json!({ "id": id }),
        ),
        PluginCommands::Grant { id, permission } => (
            "plugin.grant",
            serde_json::json!({ "id": id, "permission": permission }),
        ),
        PluginCommands::Revoke { id, permission } => (
            "plugin.revoke",
            serde_json::json!({ "id": id, "permission": permission }),
        ),
        PluginCommands::GrantAgentPermission {
            agent,
            permission,
            ttl,
        } => (
            "plugin.grant_agent_permission",
            serde_json::json!({
                "agent_id": agent,
                "permission": permission,
                "ttl_secs": ttl,
            }),
        ),
        PluginCommands::RevokeAgentPermission { agent, permission } => (
            "plugin.revoke_agent_permission",
            serde_json::json!({
                "agent_id": agent,
                "permission": permission,
            }),
        ),
        PluginCommands::ListAgentPermissions { agent } => (
            "plugin.list_agent_permissions",
            serde_json::json!({ "agent_id": agent }),
        ),
        PluginCommands::RequestPermission {
            agent,
            permission,
            reason,
        } => (
            "plugin.request_permission",
            serde_json::json!({
                "agent_id": agent,
                "permission": permission,
                "reason": reason,
            }),
        ),
        PluginCommands::Extension { command } => match command {
            crate::cli::ExtensionCommands::List => {
                ("plugin.extension.list", serde_json::json!({}))
            }
        },
        PluginCommands::AuditQuery {
            caller_kind,
            caller_id,
            method_prefix,
            decision,
            since_ms,
            until_ms,
            limit,
        } => {
            let mut p = serde_json::Map::new();
            if let Some(v) = caller_kind {
                p.insert("caller_kind".into(), serde_json::json!(v));
            }
            if let Some(v) = caller_id {
                p.insert("caller_id".into(), serde_json::json!(v));
            }
            if let Some(v) = method_prefix {
                p.insert("method_prefix".into(), serde_json::json!(v));
            }
            if let Some(v) = decision {
                p.insert("decision".into(), serde_json::json!(v));
            }
            if let Some(v) = since_ms {
                p.insert("since_ms".into(), serde_json::json!(v));
            }
            if let Some(v) = until_ms {
                p.insert("until_ms".into(), serde_json::json!(v));
            }
            if let Some(v) = limit {
                p.insert("limit".into(), serde_json::json!(v));
            }
            ("plugin.audit_query", serde_json::Value::Object(p))
        }
        PluginCommands::AuditSummary {
            caller_kind,
            caller_id,
            method_prefix,
            decision,
            since_ms,
            until_ms,
            top_n,
        } => {
            let mut p = serde_json::Map::new();
            if let Some(v) = caller_kind {
                p.insert("caller_kind".into(), serde_json::json!(v));
            }
            if let Some(v) = caller_id {
                p.insert("caller_id".into(), serde_json::json!(v));
            }
            if let Some(v) = method_prefix {
                p.insert("method_prefix".into(), serde_json::json!(v));
            }
            if let Some(v) = decision {
                p.insert("decision".into(), serde_json::json!(v));
            }
            if let Some(v) = since_ms {
                p.insert("since_ms".into(), serde_json::json!(v));
            }
            if let Some(v) = until_ms {
                p.insert("until_ms".into(), serde_json::json!(v));
            }
            if let Some(v) = top_n {
                p.insert("top_n".into(), serde_json::json!(v));
            }
            ("plugin.audit_summary", serde_json::Value::Object(p))
        }
        PluginCommands::AuditClear { before_ms } => {
            let mut p = serde_json::Map::new();
            if let Some(v) = before_ms {
                p.insert("before_ms".into(), serde_json::json!(v));
            }
            ("plugin.audit_clear", serde_json::Value::Object(p))
        }
        // AuditFollow는 IPC를 거치지 않음 — run_client에서 special-case로 처리.
        PluginCommands::AuditFollow { .. } => ("plugin.list", serde_json::json!({})),
    }
}

/// Reduce the 5 scope-alias flags + raw `--scope` into a single canonical
/// scope token (`global` / `surface:3` / ...). Returns `None` only if none
/// of the flags were given — caller decides whether that's an error
/// (per-method) or "stats over everything" (memory stats).

fn output_command_to_method_params(
    command: &OutputCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        OutputCommands::Observe { command } => match command {
            OutputObserveCommands::Start {
                surface,
                parsers,
                kinds,
                sink,
                path,
                max_records,
            } => {
                let mut sink_obj = serde_json::json!({ "type": sink });
                if sink == "file" {
                    if let Some(p) = path {
                        sink_obj["path"] = serde_json::Value::String(p.clone());
                    }
                } else if sink == "memory" {
                    sink_obj["max_records"] = serde_json::Value::from(*max_records);
                }
                let mut params = serde_json::json!({ "sink": sink_obj });
                if let Some(s) = surface {
                    params["surface_id"] = serde_json::Value::from(*s);
                }
                if let Some(p) = parsers {
                    params["parsers"] = serde_json::Value::Array(
                        p.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
                    );
                }
                if let Some(k) = kinds {
                    params["kinds"] = serde_json::Value::Array(
                        k.iter().map(|s| serde_json::Value::String(s.clone())).collect(),
                    );
                }
                ("output.observe_start", params)
            }
            OutputObserveCommands::Stop { observer } => (
                "output.observe_stop",
                serde_json::json!({ "observer_id": observer }),
            ),
            OutputObserveCommands::List => ("output.observe_list", serde_json::json!({})),
            OutputObserveCommands::Info { observer } => (
                "output.observe_info",
                serde_json::json!({ "observer_id": observer }),
            ),
        },
    }
}

fn approval_command_to_method_params(
    command: &ApprovalCommands,
) -> (&'static str, serde_json::Value) {
    use ApprovalCommands::*;
    match command {
        Request {
            title,
            body,
            choices,
            default_choice,
            timeout_ms,
            severity,
            workspace_id,
            surface_id,
            metadata,
        } => {
            let mut p = serde_json::json!({ "title": title });
            if let Some(b) = body {
                p["body"] = serde_json::Value::String(b.clone());
            }
            if let Some(raw) = choices {
                let arr: Vec<serde_json::Value> = raw
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(|spec| {
                        let mut parts = spec.split(':');
                        let key = parts.next().unwrap_or("").to_string();
                        let label = parts.next().map(str::to_string).unwrap_or_else(|| key.clone());
                        let destructive = matches!(parts.next(), Some("1") | Some("true"));
                        serde_json::json!({
                            "key": key,
                            "label": label,
                            "destructive": destructive,
                        })
                    })
                    .collect();
                p["choices"] = serde_json::Value::Array(arr);
            }
            if let Some(d) = default_choice {
                p["default_choice"] = serde_json::Value::String(d.clone());
            }
            if let Some(t) = timeout_ms {
                p["timeout_ms"] = serde_json::Value::from(*t);
            }
            if let Some(s) = severity {
                p["severity"] = serde_json::Value::String(s.clone());
            }
            if let Some(w) = workspace_id {
                p["workspace_id"] = serde_json::Value::from(*w);
            }
            if let Some(s) = surface_id {
                p["surface_id"] = serde_json::Value::from(*s);
            }
            if let Some(m) = metadata {
                match serde_json::from_str::<serde_json::Value>(m) {
                    Ok(v) => p["metadata"] = v,
                    Err(e) => {
                        eprintln!("Error: --metadata must be valid JSON: {e}");
                        std::process::exit(2);
                    }
                }
            }
            ("approval.request", p)
        }
        Respond { id, choice, comment } => {
            let mut p = serde_json::json!({ "id": id, "choice": choice });
            if let Some(c) = comment {
                p["comment"] = serde_json::Value::String(c.clone());
            }
            ("approval.respond", p)
        }
        Cancel { id } => ("approval.cancel", serde_json::json!({ "id": id })),
        Await { id, timeout_ms } => {
            let mut p = serde_json::json!({ "id": id });
            if let Some(t) = timeout_ms {
                p["timeout_ms"] = serde_json::Value::from(*t);
            }
            ("approval.await", p)
        }
        Get { id } => ("approval.get", serde_json::json!({ "id": id })),
        List {
            state,
            workspace_id,
        } => {
            let mut p = serde_json::json!({});
            if let Some(s) = state {
                p["state"] = serde_json::Value::String(s.clone());
            }
            if let Some(w) = workspace_id {
                p["workspace_id"] = serde_json::Value::from(*w);
            }
            ("approval.list", p)
        }
        History {
            since,
            until,
            workspace_id,
            requester_id,
            decision,
            state,
            limit,
        } => {
            let mut p = serde_json::json!({});
            if let Some(s) = since {
                p["since"] = serde_json::Value::from(*s);
            }
            if let Some(u) = until {
                p["until"] = serde_json::Value::from(*u);
            }
            if let Some(w) = workspace_id {
                p["workspace_id"] = serde_json::Value::from(*w);
            }
            if let Some(r) = requester_id {
                p["requester_id"] = serde_json::Value::String(r.clone());
            }
            if let Some(d) = decision {
                p["decision"] = serde_json::Value::String(d.clone());
            }
            if let Some(s) = state {
                p["state"] = serde_json::Value::String(s.clone());
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::Value::from(*l);
            }
            ("approval.history", p)
        }
        Summary { command } => {
            use crate::cli::ApprovalSummaryCommands::*;
            match command {
                Set { workspace_id, content } => {
                    let resolved = if let Some(path) = content.strip_prefix('@') {
                        match std::fs::read_to_string(path) {
                            Ok(s) => s,
                            Err(e) => {
                                eprintln!("Error: failed to read --content file '{path}': {e}");
                                std::process::exit(2);
                            }
                        }
                    } else {
                        content.clone()
                    };
                    (
                        "approval.summary.set",
                        serde_json::json!({ "workspace_id": *workspace_id, "content": resolved }),
                    )
                }
                Get { workspace_id } => (
                    "approval.summary.get",
                    serde_json::json!({ "workspace_id": *workspace_id }),
                ),
            }
        }
    }
}

fn file_handler_command_to_method_params(
    command: &FileHandlerCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        FileHandlerCommands::Reload => ("file_handler.reload", serde_json::Value::Null),
    }
}

fn script_command_to_method_params(
    command: &ScriptCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        ScriptCommands::Reload => ("script.reload", serde_json::Value::Null),
    }
}

/// Read --file (or "-" for stdin) and parse as JSON.
fn read_json_file_or_stdin(path: &str) -> Result<serde_json::Value, String> {
    use std::io::Read;
    let raw = if path == "-" {
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .map_err(|e| format!("stdin read failed: {e}"))?;
        buf
    } else {
        std::fs::read_to_string(path).map_err(|e| format!("file read failed: {e}"))?
    };
    serde_json::from_str(&raw).map_err(|e| format!("invalid JSON: {e}"))
}

fn preset_command_to_method_params(
    command: &PresetCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        PresetCommands::List { kind } => (
            "preset.list",
            serde_json::json!({ "kind": kind }),
        ),
        PresetCommands::Get { kind, name } => (
            "preset.get",
            serde_json::json!({ "kind": kind, "name": name }),
        ),
        PresetCommands::Save {
            kind,
            name,
            file,
            overwrite,
        } => {
            let data = match read_json_file_or_stdin(file) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {e}");
                    std::process::exit(1);
                }
            };
            (
                "preset.save",
                serde_json::json!({
                    "kind": kind,
                    "name": name,
                    "data": data,
                    "overwrite": overwrite,
                }),
            )
        }
        PresetCommands::Delete { kind, name } => (
            "preset.delete",
            serde_json::json!({ "kind": kind, "name": name }),
        ),
        PresetCommands::Rename { kind, from, to } => (
            "preset.rename",
            serde_json::json!({ "kind": kind, "from": from, "to": to }),
        ),
        PresetCommands::Capture {
            kind,
            source_id,
            name,
        } => (
            "preset.capture",
            serde_json::json!({
                "kind": kind,
                "source_id": source_id,
                "name": name,
            }),
        ),
        PresetCommands::Apply {
            kind,
            name,
            target_pane,
            target_workspace,
        } => (
            "preset.apply",
            serde_json::json!({
                "kind": kind,
                "name": name,
                "target_pane_id": target_pane,
                "target_workspace_id": target_workspace,
            }),
        ),
    }
}
