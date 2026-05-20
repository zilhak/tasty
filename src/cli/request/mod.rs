mod agent;
mod approval;
mod debug;
mod memory;
mod output;
mod plugin_cmd;
mod presets;
mod telemetry;

use agent::agent_command_to_method_params;
use approval::approval_command_to_method_params;
#[cfg(debug_assertions)]
use debug::debug_command_to_method_params;
use memory::memory_command_to_method_params;
use output::output_command_to_method_params;
use plugin_cmd::plugin_command_to_method_params;
use presets::{
    file_handler_command_to_method_params, preset_command_to_method_params,
    script_command_to_method_params,
};
use telemetry::telemetry_command_to_method_params;

use super::{
    ClipboardCommands, CloseCommands, Commands, ListCommands, MoveCommands, NewCommands,
    ReadCommands, SendCommands, SetCommands, SurfaceMetaCommands, ToolCommands, UnsetCommands,
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
        session_token: std::env::var("TASTY_SESSION_TOKEN")
            .ok()
            .filter(|s| !s.is_empty()),
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
                    ids.iter()
                        .map(|s| serde_json::Value::String(s.clone()))
                        .collect(),
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
