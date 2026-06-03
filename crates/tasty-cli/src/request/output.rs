//! `tasty output ...` CLI → JsonRpcRequest 매핑.

use crate::commands::{OutputCommands, OutputObserveCommands};

pub(super) fn output_command_to_method_params(
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
                        p.iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
                    );
                }
                if let Some(k) = kinds {
                    params["kinds"] = serde_json::Value::Array(
                        k.iter()
                            .map(|s| serde_json::Value::String(s.clone()))
                            .collect(),
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
