//! `tasty settings` CLI → JsonRpcRequest 매핑 (07 원격 전송 저장 정책).

use crate::commands::SettingsCommands;

pub(super) fn settings_command_to_method_params(
    command: &SettingsCommands,
) -> (&'static str, serde_json::Value) {
    match command {
        SettingsCommands::GetRemoteTransfer => {
            ("settings.get_remote_transfer", serde_json::json!({}))
        }
        SettingsCommands::SetRemoteTransfer { dir, max_mb } => {
            // 지정된 필드만 실어 보낸다(핸들러가 부분 patch 로 현재 설정 위에 덮음).
            let mut params = serde_json::Map::new();
            if let Some(dir) = dir {
                params.insert("dir".to_string(), serde_json::json!(dir));
            }
            if let Some(max_mb) = max_mb {
                params.insert("max_mb".to_string(), serde_json::json!(max_mb));
            }
            (
                "settings.set_remote_transfer",
                serde_json::Value::Object(params),
            )
        }
    }
}
