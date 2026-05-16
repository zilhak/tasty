#[cfg(debug_assertions)]
use super::{DebugCommands, EventBusCommands};
use super::{
    ApprovalCommands, ClipboardCommands, CloseCommands, Commands, ListCommands, MemoryCommands,
    MemorySecretCommands, MoveCommands, NewCommands, OutputCommands, OutputObserveCommands,
    PluginCommands, ReadCommands, SendCommands, SetCommands, SurfaceMetaCommands, ToolCommands,
    UnsetCommands,
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
    };

    JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: method.to_string(),
        params,
        id: Some(serde_json::json!(1)),
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
fn debug_command_to_method_params(command: &DebugCommands) -> (&'static str, serde_json::Value) {
    match command {
        DebugCommands::Info => ("debug.info", serde_json::json!({})),
        DebugCommands::ImeEnable => ("surface.ime_enable", serde_json::json!({})),
        DebugCommands::ImeDisable => ("surface.ime_disable", serde_json::json!({})),
        DebugCommands::ImePreedit { text, cursor } => (
            "surface.ime_preedit",
            serde_json::json!({ "text": text, "cursor": cursor }),
        ),
        DebugCommands::ImeCommit { text } => {
            ("surface.ime_commit", serde_json::json!({ "text": text }))
        }
        DebugCommands::ImeStatus => ("surface.ime_status", serde_json::json!({})),
        DebugCommands::CellInfo { row, col, surface } => (
            "debug.cell_info",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "row": row,
                "col": col,
            }),
        ),
        DebugCommands::ScreenAttrs { row, surface } => (
            "debug.screen_attrs",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "row": row,
            }),
        ),
        DebugCommands::GlyphColor {
            row,
            col,
            surface,
            bg_mode,
        } => (
            "debug.glyph_color",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "row": row,
                "col": col,
                "bg_mode": bg_mode,
            }),
        ),
        DebugCommands::SwitchInputSource { source_id } => (
            "surface.switch_input_source",
            serde_json::json!({ "source_id": source_id }),
        ),
        DebugCommands::RawKey { keycode } => {
            ("surface.raw_key", serde_json::json!({ "keycode": keycode }))
        }
        DebugCommands::EventBus(sub) => event_bus_command_to_method_params(sub),
        DebugCommands::Extension(sub) => extension_debug_command_to_method_params(sub),
        DebugCommands::Tool(sub) => tool_debug_command_to_method_params(sub),
        DebugCommands::Popup(sub) => popup_debug_command_to_method_params(sub),
    }
}

#[cfg(debug_assertions)]
fn popup_debug_command_to_method_params(
    command: &crate::cli::PopupDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::cli::PopupDebugCommands;
    match command {
        PopupDebugCommands::List => ("debug.popup.list", serde_json::json!({})),
        PopupDebugCommands::Open {
            plugin_id,
            popup_id,
            context,
        } => {
            let ctx_value: serde_json::Value = match context {
                Some(s) => serde_json::from_str(s).unwrap_or(serde_json::Value::Null),
                None => serde_json::Value::Null,
            };
            (
                "debug.popup.open",
                serde_json::json!({
                    "plugin_id": plugin_id,
                    "popup_id": popup_id,
                    "context": ctx_value,
                }),
            )
        }
        PopupDebugCommands::Close { instance_id } => (
            "debug.popup.close",
            serde_json::json!({ "instance_id": instance_id }),
        ),
    }
}

#[cfg(debug_assertions)]
fn tool_debug_command_to_method_params(
    command: &crate::cli::ToolDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::cli::ToolDebugCommands;
    match command {
        ToolDebugCommands::List => ("debug.tool.list", serde_json::json!({})),
        ToolDebugCommands::Invoke { key } => {
            ("debug.tool.invoke", serde_json::json!({ "key": key }))
        }
    }
}

#[cfg(debug_assertions)]
fn extension_debug_command_to_method_params(
    command: &crate::cli::ExtensionDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::cli::ExtensionDebugCommands;
    match command {
        ExtensionDebugCommands::InvokeHook {
            extension_id,
            kind,
            phase,
            mode,
            target,
            payload,
        } => {
            let parsed_payload: serde_json::Value =
                serde_json::from_str(payload).unwrap_or(serde_json::Value::Null);
            (
                "debug.extension.invoke_hook",
                serde_json::json!({
                    "extension_id": extension_id,
                    "kind": kind,
                    "phase": phase,
                    "mode": mode,
                    "target": target,
                    "payload": parsed_payload,
                }),
            )
        }
    }
}

#[cfg(debug_assertions)]
fn event_bus_command_to_method_params(
    command: &EventBusCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        EventBusCommands::ListSubscribers { key } => (
            "debug.event_bus.list_subscribers",
            serde_json::json!({ "key": key }),
        ),
        EventBusCommands::Publish {
            key,
            payload,
            scope,
        } => (
            "debug.event_bus.publish",
            serde_json::json!({
                "key": key,
                "payload": payload,
                "scope": scope,
            }),
        ),
        EventBusCommands::Trace { trace_id } => (
            "debug.event_bus.trace",
            serde_json::json!({ "trace_id": trace_id }),
        ),
    }
}

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
        PluginCommands::Extension { command } => match command {
            crate::cli::ExtensionCommands::List => {
                ("plugin.extension.list", serde_json::json!({}))
            }
        },
    }
}

/// Reduce the 5 scope-alias flags + raw `--scope` into a single canonical
/// scope token (`global` / `surface:3` / ...). Returns `None` only if none
/// of the flags were given — caller decides whether that's an error
/// (per-method) or "stats over everything" (memory stats).
fn resolve_scope(
    scope: Option<&str>,
    surface: Option<u32>,
    workspace: Option<u32>,
    window: Option<u64>,
    account: Option<&str>,
    global: bool,
) -> Option<String> {
    if let Some(s) = scope {
        return Some(s.to_string());
    }
    if let Some(id) = surface {
        return Some(format!("surface:{id}"));
    }
    if let Some(id) = workspace {
        return Some(format!("workspace:{id}"));
    }
    if let Some(id) = window {
        return Some(format!("window:{id}"));
    }
    if let Some(u) = account {
        return Some(format!("account:{u}"));
    }
    if global {
        return Some("global".to_string());
    }
    None
}

/// `@path` 접두를 가진 value는 파일에서 UTF-8 텍스트로 읽어온다. 이외에는 그대로.
fn read_value_arg(value: &str) -> std::io::Result<String> {
    if let Some(path) = value.strip_prefix('@') {
        std::fs::read_to_string(path)
    } else {
        Ok(value.to_string())
    }
}

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
    }
}

fn memory_command_to_method_params(
    command: &MemoryCommands,
) -> (&'static str, serde_json::Value) {
    use MemoryCommands::*;
    match command {
        Put {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            key,
            value,
            value_b64,
            content_type,
            ttl,
            expires_at,
            cas,
        } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut params = serde_json::json!({
                "scope": scope_token,
                "key": key,
            });
            if let Some(b64) = value_b64.as_deref() {
                params["value_b64"] = serde_json::json!(b64);
            } else if let Some(v) = value.as_deref() {
                let raw = match read_value_arg(v) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: failed to read value file: {e}");
                        std::process::exit(1);
                    }
                };
                // JSON으로 파싱되면 JSON value, 아니면 string으로 보존.
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    params["value"] = parsed;
                } else {
                    params["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!("Error: 'memory put' requires --value or --value-b64");
                std::process::exit(1);
            }
            if let Some(ct) = content_type.as_deref() {
                params["content_type"] = serde_json::json!(ct);
            }
            if let Some(t) = expires_at {
                params["expires_at"] = serde_json::json!(t);
            } else if let Some(secs) = ttl {
                params["expires_at"] = serde_json::json!(ttl_to_expires_at(*secs));
            }
            if let Some(v) = cas {
                params["cas"] = serde_json::json!(v);
            }
            ("memory.put", params)
        }
        Get { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.get",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        Delete { scope, surface, workspace, window, account, global, key, cas } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token, "key": key });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.delete", p)
        }
        Exists { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.exists",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        List {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            prefix,
            limit,
            since,
            until,
            offset,
        } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::json!(l);
            }
            if let Some(s) = since {
                p["since"] = serde_json::json!(s);
            }
            if let Some(u) = until {
                p["until"] = serde_json::json!(u);
            }
            if let Some(o) = offset {
                p["offset"] = serde_json::json!(o);
            }
            ("memory.list", p)
        }
        Query {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            path,
            equals,
            prefix,
            limit,
            since,
            until,
            offset,
        } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            // `--equals` 는 JSON 리터럴로 파싱; 실패하면 문자열 그대로.
            let equals_val: serde_json::Value = match serde_json::from_str(equals) {
                Ok(v) => v,
                Err(_) => serde_json::Value::String(equals.clone()),
            };
            let mut p = serde_json::json!({
                "scope": scope_token,
                "path": path,
                "equals": equals_val,
            });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::json!(l);
            }
            if let Some(s) = since {
                p["since"] = serde_json::json!(s);
            }
            if let Some(u) = until {
                p["until"] = serde_json::json!(u);
            }
            if let Some(o) = offset {
                p["offset"] = serde_json::json!(o);
            }
            ("memory.query", p)
        }
        Export { scope, surface, workspace, window, account, global } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.export", p)
        }
        Import { file, replace } => {
            let raw = match std::fs::read_to_string(file) {
                Ok(s) => s,
                Err(e) => {
                    eprintln!("Error: failed to read {file}: {e}");
                    std::process::exit(1);
                }
            };
            let parsed: serde_json::Value = match serde_json::from_str(&raw) {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("Error: {file}: not valid JSON: {e}");
                    std::process::exit(1);
                }
            };
            // 입력은 배열이거나 `{ "entries": [...] }` 형태 둘 다 허용.
            let entries = if parsed.is_array() {
                parsed
            } else if let Some(arr) = parsed.get("entries") {
                arr.clone()
            } else {
                eprintln!("Error: {file}: expected JSON array or object with 'entries'");
                std::process::exit(1);
            };
            (
                "memory.import",
                serde_json::json!({ "entries": entries, "replace": replace }),
            )
        }
        Count { scope, surface, workspace, window, account, global, prefix } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            ("memory.count", p)
        }
        Scopes => ("memory.scopes", serde_json::json!({})),
        Stats { scope, surface, workspace, window, account, global } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.stats", p)
        }
        Gc => ("memory.gc", serde_json::json!({})),
        Secret { command } => memory_secret_command_to_method_params(command),
    }
}

/// 상대 TTL(초)을 절대 expires_at(unix ms)으로 환산.
fn ttl_to_expires_at(secs: u64) -> i64 {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let add_ms = secs.saturating_mul(1000).min(i64::MAX as u64) as i64;
    now_ms.saturating_add(add_ms)
}

fn memory_secret_command_to_method_params(
    command: &MemorySecretCommands,
) -> (&'static str, serde_json::Value) {
    use MemorySecretCommands::*;
    match command {
        Put {
            scope,
            surface,
            workspace,
            window,
            account,
            global,
            key,
            value,
            value_b64,
            content_type,
            ttl,
            expires_at,
            cas,
        } => {
            let scope_token = require_scope(
                scope.as_deref(),
                *surface,
                *workspace,
                *window,
                account.as_deref(),
                *global,
            );
            let mut params = serde_json::json!({
                "scope": scope_token,
                "key": key,
            });
            if let Some(b64) = value_b64.as_deref() {
                params["value_b64"] = serde_json::json!(b64);
            } else if let Some(v) = value.as_deref() {
                let raw = match read_value_arg(v) {
                    Ok(s) => s,
                    Err(e) => {
                        eprintln!("Error: failed to read value file: {e}");
                        std::process::exit(1);
                    }
                };
                if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&raw) {
                    params["value"] = parsed;
                } else {
                    params["value"] = serde_json::Value::String(raw);
                }
            } else {
                eprintln!("Error: 'memory secret put' requires --value or --value-b64");
                std::process::exit(1);
            }
            if let Some(ct) = content_type.as_deref() {
                params["content_type"] = serde_json::json!(ct);
            }
            if let Some(t) = expires_at {
                params["expires_at"] = serde_json::json!(t);
            } else if let Some(secs) = ttl {
                params["expires_at"] = serde_json::json!(ttl_to_expires_at(*secs));
            }
            if let Some(v) = cas {
                params["cas"] = serde_json::json!(v);
            }
            ("memory.secret.put", params)
        }
        Get { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.secret.get",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        Delete { scope, surface, workspace, window, account, global, key, cas } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token, "key": key });
            if let Some(c) = cas {
                p["cas"] = serde_json::json!(c);
            }
            ("memory.secret.delete", p)
        }
        Exists { scope, surface, workspace, window, account, global, key } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            (
                "memory.secret.exists",
                serde_json::json!({ "scope": scope_token, "key": key }),
            )
        }
        List { scope, surface, workspace, window, account, global, prefix, limit } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            if let Some(l) = limit {
                p["limit"] = serde_json::json!(l);
            }
            ("memory.secret.list", p)
        }
        Count { scope, surface, workspace, window, account, global, prefix } => {
            let scope_token = require_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global);
            let mut p = serde_json::json!({ "scope": scope_token });
            if let Some(pre) = prefix {
                p["prefix"] = serde_json::json!(pre);
            }
            ("memory.secret.count", p)
        }
        Scopes => ("memory.secret.scopes", serde_json::json!({})),
        Stats { scope, surface, workspace, window, account, global } => {
            let mut p = serde_json::json!({});
            if let Some(tok) = resolve_scope(scope.as_deref(), *surface, *workspace, *window, account.as_deref(), *global) {
                p["scope"] = serde_json::json!(tok);
            }
            ("memory.secret.stats", p)
        }
    }
}

/// scope가 반드시 필요한 메서드용. 없으면 즉시 에러 exit.
fn require_scope(
    scope: Option<&str>,
    surface: Option<u32>,
    workspace: Option<u32>,
    window: Option<u64>,
    account: Option<&str>,
    global: bool,
) -> String {
    match resolve_scope(scope, surface, workspace, window, account, global) {
        Some(s) => s,
        None => {
            eprintln!(
                "Error: must specify a scope. Use --scope <token> or one of \
                 --global / --surface <id> / --workspace <id> / --window <id> / --account <userid>."
            );
            std::process::exit(1);
        }
    }
}
