//! `tasty debug ...` CLI → JsonRpcRequest 매핑 (debug + popup + tool + extension + event_bus).

#![cfg(debug_assertions)]

use crate::commands::{DebugCommands, EventBusCommands};

use super::resolve_surface_id;

pub(super) fn debug_command_to_method_params(
    command: &DebugCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        // 로컬 attach 의 non-force 경로는 run_client 에서 raw 스트림으로 선처리된다.
        // 여기 도달하는 건 `--force-detach` 뿐 → workspace 면 force_detach_workspace,
        // 아니면 surface force_detach IPC.
        DebugCommands::Attach {
            surface,
            workspace,
            force_detach,
            ..
        } => {
            debug_assert!(
                *force_detach,
                "non-force debug attach is dispatched before request mapping"
            );
            if let Some(ws) = workspace {
                (
                    "attach.force_detach_workspace",
                    serde_json::json!({ "workspace_id": ws }),
                )
            } else {
                (
                    "attach.force_detach",
                    serde_json::json!({ "surface_id": surface }),
                )
            }
        }
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
        DebugCommands::HostPopup(sub) => host_popup_debug_command_to_method_params(sub),
        DebugCommands::Banner(sub) => banner_debug_command_to_method_params(sub),
        DebugCommands::Settings(sub) => settings_debug_command_to_method_params(sub),
        // stream-echo is a raw framed exchange, not a JSON-RPC request — it is
        // handled directly in `run_client` before request mapping is reached.
        DebugCommands::StreamEcho { .. } => {
            unreachable!("debug stream-echo is dispatched before request mapping")
        }
        // sim emits raw VTE locally — handled directly in `run_client` before
        // request mapping is reached.
        DebugCommands::Sim { .. } => {
            unreachable!("debug sim is dispatched before request mapping")
        }
    }
}

#[cfg(debug_assertions)]
pub(super) fn popup_debug_command_to_method_params(
    command: &crate::PopupDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::PopupDebugCommands;
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
pub(super) fn host_popup_debug_command_to_method_params(
    command: &crate::HostPopupDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::HostPopupDebugCommands;
    match command {
        HostPopupDebugCommands::List => ("debug.host_popup.list", serde_json::json!({})),
        HostPopupDebugCommands::Open { popup_id } => (
            "debug.host_popup.open",
            serde_json::json!({ "popup_id": popup_id }),
        ),
        HostPopupDebugCommands::Close { popup_id } => (
            "debug.host_popup.close",
            serde_json::json!({ "popup_id": popup_id }),
        ),
    }
}

#[cfg(debug_assertions)]
pub(super) fn banner_debug_command_to_method_params(
    command: &crate::BannerDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::BannerDebugCommands;
    match command {
        BannerDebugCommands::List => ("debug.banner.list", serde_json::json!({})),
        BannerDebugCommands::Show { banner_id, scope } => (
            "debug.banner.show",
            serde_json::json!({ "banner_id": banner_id, "scope": scope }),
        ),
        BannerDebugCommands::Close { banner_id } => (
            "debug.banner.close",
            serde_json::json!({ "banner_id": banner_id }),
        ),
        BannerDebugCommands::SetCountdown { scope, seconds } => (
            "debug.banner.set_countdown",
            serde_json::json!({ "scope": scope, "seconds": seconds }),
        ),
    }
}

#[cfg(debug_assertions)]
pub(super) fn settings_debug_command_to_method_params(
    command: &crate::SettingsDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::SettingsDebugCommands;
    match command {
        SettingsDebugCommands::Open { tab } => {
            ("debug.settings.open", serde_json::json!({ "tab": tab }))
        }
    }
}

#[cfg(debug_assertions)]
pub(super) fn tool_debug_command_to_method_params(
    command: &crate::ToolDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::ToolDebugCommands;
    match command {
        ToolDebugCommands::List => ("debug.tool.list", serde_json::json!({})),
        ToolDebugCommands::Invoke { key } => {
            ("debug.tool.invoke", serde_json::json!({ "key": key }))
        }
    }
}

#[cfg(debug_assertions)]
pub(super) fn extension_debug_command_to_method_params(
    command: &crate::ExtensionDebugCommands,
) -> (&'static str, serde_json::Value) {
    use crate::ExtensionDebugCommands;
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
pub(super) fn event_bus_command_to_method_params(
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
