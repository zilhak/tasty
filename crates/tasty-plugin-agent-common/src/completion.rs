//! Host completion adapter shared by Codex and Claude producers.
use crate::host_call::HostCall;
use serde_json::{Value, json};
use tasty_plugin_sdk::PluginError;

pub fn subscribe<H: HostCall>(
    host: &H,
    parent: u32,
    target: u32,
    kind: &str,
    mode: &str,
) -> Result<Value, PluginError> {
    host.call(
        "terminal.completion",
        json!({"action":"subscribe","surface":parent,"target":target,"kind":kind,"mode":mode}),
    )
}
pub fn session<H: HostCall>(
    host: &H,
    surface: u32,
    kind: &str,
    session: &str,
) -> Result<Value, PluginError> {
    host.call(
        "terminal.completion",
        json!({"action":"session","surface":surface,"kind":kind,"hook_session":session}),
    )
}
pub fn observe<H: HostCall>(
    host: &H,
    surface: u32,
    state: &str,
    cause: &str,
    summary: &str,
) -> Result<Value, PluginError> {
    host.call(
        "terminal.completion",
        json!({"action":"observe","surface":surface,"state":state,"cause":cause,"summary":summary}),
    )
}
/// Only a positively registered Claude parent uses the legacy log channel.
pub fn legacy_log<H: HostCall>(host: &H, parent: u32) -> bool {
    match host.call(
        "terminal.completion",
        json!({"action":"route","surface":parent}),
    ) {
        Ok(value) => value["legacy_log"].as_bool().unwrap_or(false),
        Err(error) => {
            tracing::warn!("completion routing unavailable: {error}");
            false
        }
    }
}
