use crate::ipc::protocol::JsonRpcRequest;
use super::{Commands, NewCommands, CloseCommands, ListCommands, SetCommands, SetFocusCommands, UnsetCommands, SendCommands, ReadCommands, ClaudeCommands, DebugCommands};

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

/// Get surface_id: explicit value > TASTY_SURFACE_ID env var.
fn resolve_surface_id(explicit: Option<u32>) -> Option<u32> {
    explicit.or_else(|| {
        std::env::var("TASTY_SURFACE_ID").ok()?.parse().ok()
    })
}


pub fn command_to_request(command: &Commands) -> JsonRpcRequest {
    let (method, params) = match command {
        // ── grouped ──
        Commands::New { command } => new_command_to_method_params(command),
        Commands::Close { command } => close_command_to_method_params(command),
        Commands::List { command } => list_command_to_method_params(command),
        Commands::Set { command } => set_command_to_method_params(command),
        Commands::Claude { command } => claude_command_to_method_params(command),
        Commands::Debug { command } => debug_command_to_method_params(command),
        // ── standalone ──
        Commands::Send { command } => send_command_to_method_params(command),
        Commands::Read { command } => read_command_to_method_params(command),
        Commands::Notify { body, title } => (
            "notification.create",
            serde_json::json!({ "title": title, "body": body }),
        ),
        Commands::Unset { command } => unset_command_to_method_params(command),
        Commands::SurfaceMeta { action, surface, key, value } => {
            let method = match action.as_str() {
                "set" => "surface.meta_set",
                "get" => "surface.meta_get",
                "unset" => "surface.meta_unset",
                "list" => "surface.meta_list",
                _ => "surface.meta_list",
            };
            (
                method,
                serde_json::json!({
                    "surface_id": resolve_surface_id(*surface),
                    "key": key,
                    "value": value,
                }),
            )
        }
        Commands::IsTyping { surface } => (
            "surface.is_typing",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
        ),
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
        NewCommands::Workspace { name, cwd } => (
            "workspace.create",
            serde_json::json!({ "name": name.as_deref().unwrap_or(""), "cwd": cwd }),
        ),
        NewCommands::Tab { pane, cwd } => ("tab.create", serde_json::json!({ "pane_id": pane, "cwd": cwd })),
        NewCommands::Split { level, target, direction, meta, cwd } => {
            let resolved_target = resolve_target(target);
            let meta_value = meta
                .as_deref()
                .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok());
            (
                "split",
                serde_json::json!({
                    "level": level,
                    "target": resolved_target,
                    "direction": direction,
                    "meta": meta_value,
                    "cwd": cwd,
                }),
            )
        }
        NewCommands::Markdown { path, pane } => (
            "tab.open_markdown",
            serde_json::json!({ "file_path": path, "pane_id": pane }),
        ),
        NewCommands::Explorer { pane, path } => (
            "tab.open_explorer",
            serde_json::json!({ "pane_id": pane, "path": path }),
        ),
    }
}

fn close_command_to_method_params(command: &CloseCommands) -> (&'static str, serde_json::Value) {
    match command {
        CloseCommands::Tab { pane } => ("tab.close", serde_json::json!({ "pane_id": pane })),
        CloseCommands::Pane { pane } => ("pane.close", serde_json::json!({ "pane_id": pane })),
        CloseCommands::Surface { surface } => ("surface.close", serde_json::json!({ "surface_id": surface })),
    }
}

fn list_command_to_method_params(command: &ListCommands) -> (&'static str, serde_json::Value) {
    match command {
        ListCommands::Workspaces => ("workspace.list", serde_json::json!({})),
        ListCommands::Windows => ("window.list", serde_json::json!({})),
        ListCommands::Tree => ("tree", serde_json::json!({})),
        ListCommands::Surfaces => ("surface.list", serde_json::json!({})),
        ListCommands::Panes => ("pane.list", serde_json::json!({})),
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
            serde_json::json!({ "text": text, "surface_id": resolve_surface_id(*surface) }),
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
        ReadCommands::Mark { surface, strip_ansi } => (
            "surface.read_since_mark",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "strip_ansi": strip_ansi,
            }),
        ),
        ReadCommands::Queue { surface, from, peek, clear } => {
            if *clear {
                ("message.clear", serde_json::json!({ "surface_id": resolve_surface_id(*surface) }))
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
        ReadCommands::Screen { surface } => (
            "surface.screen_text",
            serde_json::json!({ "surface_id": resolve_surface_id(*surface) }),
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
        SetCommands::Workspace { id, name, subtitle, description } => (
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
        SetCommands::Focus { command } => set_focus_command_to_method_params(command),
    }
}

fn set_focus_command_to_method_params(command: &SetFocusCommands) -> (&'static str, serde_json::Value) {
    match command {
        SetFocusCommands::Workspace { index } => (
            "workspace.select",
            serde_json::json!({ "index": index }),
        ),
        SetFocusCommands::Direction { direction } => (
            "focus.direction",
            serde_json::json!({ "direction": direction }),
        ),
    }
}

fn unset_command_to_method_params(command: &UnsetCommands) -> (&'static str, serde_json::Value) {
    match command {
        UnsetCommands::Hook { hook } => (
            "hook.unset",
            serde_json::json!({ "hook_id": hook }),
        ),
        UnsetCommands::GlobalHook { hook } => (
            "global_hook.unset",
            serde_json::json!({ "hook_id": hook }),
        ),
    }
}

fn claude_command_to_method_params(command: &ClaudeCommands) -> (&'static str, serde_json::Value) {
    match command {
        ClaudeCommands::Launch {
            workspace,
            directory,
            task,
        } => (
            "claude.launch",
            serde_json::json!({
                "workspace": workspace,
                "directory": directory,
                "task": task,
            }),
        ),
        ClaudeCommands::Spawn {
            surface,
            direction,
            cwd,
            role,
            nickname,
            prompt,
        } => (
            "claude.spawn",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "direction": direction,
                "cwd": cwd,
                "role": role,
                "nickname": nickname,
                "prompt": prompt,
            }),
        ),
        ClaudeCommands::Children { surface } => ("claude.children", serde_json::json!({ "surface_id": resolve_surface_id(*surface) })),
        ClaudeCommands::Parent { surface } => ("claude.parent", serde_json::json!({ "surface_id": resolve_surface_id(*surface) })),
        ClaudeCommands::Kill { child } => (
            "claude.kill",
            serde_json::json!({ "child_surface_id": child }),
        ),
        ClaudeCommands::Respawn {
            child,
            cwd,
            role,
            nickname,
            prompt,
        } => (
            "claude.respawn",
            serde_json::json!({
                "child_surface_id": child,
                "cwd": cwd,
                "role": role,
                "nickname": nickname,
                "prompt": prompt,
            }),
        ),
        ClaudeCommands::Broadcast { text, surface, role } => (
            "claude.broadcast",
            serde_json::json!({
                "surface_id": resolve_surface_id(*surface),
                "text": text,
                "role": role,
            }),
        ),
        // Hook and Wait are handled separately in run_client
        ClaudeCommands::Hook { .. } => unreachable!("ClaudeHook is handled in run_client"),
        ClaudeCommands::Wait { .. } => unreachable!("ClaudeWait is handled in run_client"),
    }
}

fn debug_command_to_method_params(command: &DebugCommands) -> (&'static str, serde_json::Value) {
    match command {
        DebugCommands::Info => ("debug.info", serde_json::json!({})),
        DebugCommands::ImeEnable => ("surface.ime_enable", serde_json::json!({})),
        DebugCommands::ImeDisable => ("surface.ime_disable", serde_json::json!({})),
        DebugCommands::ImePreedit { text, cursor } => (
            "surface.ime_preedit",
            serde_json::json!({ "text": text, "cursor": cursor }),
        ),
        DebugCommands::ImeCommit { text } => (
            "surface.ime_commit",
            serde_json::json!({ "text": text }),
        ),
        DebugCommands::ImeStatus => ("surface.ime_status", serde_json::json!({})),
        DebugCommands::SwitchInputSource { source_id } => (
            "surface.switch_input_source",
            serde_json::json!({ "source_id": source_id }),
        ),
        DebugCommands::RawKey { keycode } => (
            "surface.raw_key",
            serde_json::json!({ "keycode": keycode }),
        ),
    }
}
