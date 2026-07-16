//! `tasty clipboard` CLI → JsonRpcRequest 매핑.

use crate::commands::ClipboardCommands;

pub(super) fn clipboard_command_to_method_params(
    command: &ClipboardCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        ClipboardCommands::SetText { text } => {
            ("clipboard.set_text", serde_json::json!({ "text": text }))
        }
    }
}
