//! Tasty가 실행하는 Codex 세션: 전용 app-server와 `--remote` TUI.
//!
//! launch·spawn·respawn·reboot·복원은 모두 [`command::command_line`]으로 helper를 실행한다.
//! helper는 시작할 때 `codex.session_open`으로 새 세대를 발급받고, 그 세대의 서버 환경에
//! `TASTY_CODEX_GEN`을 넣는다. 설치된 훅은 이 값과 터미널의 surface를 함께 보내며, 플러그인은
//! surface의 현재 세대와 같을 때만 훅을 적용한다.

pub(crate) mod command;
pub(crate) mod helper;
pub(crate) mod registry;

use std::path::Path;

use serde_json::{Value, json};
use tasty_plugin_sdk::{IpcMethodError, i18n::Translator};

/// `codex.session_open`: helper가 자기 터미널의 새 세대를 발급받는다.
pub(crate) fn handle_open(
    root: &Path,
    params: &Value,
    tr: &Translator,
) -> Result<Value, IpcMethodError> {
    let surface = crate::handlers::require_u32(params, "surface", tr)?;
    let resume = match params.get("resume").and_then(Value::as_str) {
        Some(t) if crate::ids::is_uuid(t) => Some(t.to_ascii_lowercase()),
        Some(_) => {
            return Err(IpcMethodError::invalid_params(
                "resume must be a thread UUID",
            ));
        }
        None => None,
    };
    let tui_args: Vec<String> = match params.get("tui_args").and_then(Value::as_str) {
        Some(raw) => serde_json::from_str(raw)
            .map_err(|e| IpcMethodError::invalid_params(&format!("tui_args: {e}")))?,
        None => Vec::new(),
    };
    command::validate_tui_args(&tui_args).map_err(|e| IpcMethodError::invalid_params(&e))?;
    let session = registry::open_session(root, surface, resume.as_deref(), tui_args)
        .map_err(|e| IpcMethodError::new(format!("codex session registry: {e}")))?;
    Ok(json!({
        "gen": session.generation,
        "state_dir": registry::session_dir(root, &session.generation),
        "listen": session.listen,
        "token_file": session.token_file,
    }))
}

/// 터미널에 보낼 helper 명령(제출 문자 포함).
pub(crate) fn submit_line(
    resume: Option<&str>,
    prompt_file: Option<&Path>,
    tui_args: &[String],
) -> std::io::Result<String> {
    Ok(format!(
        "{}\r",
        command::command_line(&command::helper_program()?, resume, prompt_file, tui_args)
    ))
}

/// 복원·reboot가 같은 thread를 같은 정책과 모델로 다시 여는 명령(제출 문자 없음).
pub(crate) fn restore_line(session: &registry::Session, thread: &str) -> std::io::Result<String> {
    let args = command::with_model(session.tui_args.clone(), session.model.as_deref());
    Ok(command::command_line(
        &command::helper_program()?,
        Some(thread),
        None,
        &args,
    ))
}
