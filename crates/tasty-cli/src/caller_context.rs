//! CLI를 실행한 호출자의 실행 환경을 읽는다.
//!
//! Codex 도구 호출 안에서는 `CODEX_THREAD_ID`가 호출한 thread를 가리킨다. 같은 환경의
//! `TASTY_SURFACE_ID`는 Codex 서버가 시작될 때 물려받은 값일 수 있어 호출자의 surface라고
//! 볼 수 없다. 그래서 Codex 호출자가 있으면 dynamic plugin 요청의 surface 자동 채움을 하지 않는다.
//! 호스트 명령의 대상 해석과 호출자 자기 보호(`caller_surface_id`)는 이 모듈과 무관하게 기존대로 동작한다.
//! 이 값들은 환경 변수일 뿐 인증된 신원이 아니다. 받는 쪽은 혼선을 막는 데만 쓴다.

use std::ffi::OsString;

use serde_json::{Map, Value};

/// dynamic plugin 요청에 CLI가 붙이는 예약 키. 사용자 인자로는 받지 않는다.
pub const CONTEXT_KEY: &str = "_tasty_context";

/// 호출 환경의 Codex 정보.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CodexCaller {
    /// `CODEX_THREAD_ID`가 없거나 비어 있다. 기존 셸·Claude 동작을 그대로 쓴다.
    Absent,
    Thread {
        thread_id: String,
        /// 호출자 환경의 `CODEX_HOME`(절대 경로). 없으면 받는 쪽의 기본 경로 규칙을 쓴다.
        codex_home: Option<String>,
    },
    /// 값이 있으나 쓸 수 없다. 낡은 surface로 대신하지 않고 명령을 거절한다. 사용자에게 보일 문구를 담는다.
    Invalid(String),
}

/// Codex thread ID 형식(하이픈 위치가 고정된 36자 16진수 UUID).
pub fn is_thread_uuid(value: &str) -> bool {
    value.len() == 36
        && value.bytes().enumerate().all(|(i, b)| {
            if [8, 13, 18, 23].contains(&i) {
                b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
}

/// 환경 값으로 Codex 호출자를 판정한다. 환경을 직접 읽지 않아 시험에서 그대로 쓴다.
pub fn codex_caller_from(thread: Option<OsString>, codex_home: Option<OsString>) -> CodexCaller {
    let Some(thread) = thread.filter(|v| !v.is_empty()) else {
        return CodexCaller::Absent;
    };
    let Some(thread) = thread.to_str().filter(|v| is_thread_uuid(v)) else {
        return CodexCaller::Invalid(tasty_i18n::t_fmt(
            "cli.caller_context.invalid_codex_thread",
            &thread.to_string_lossy(),
        ));
    };
    let codex_home = match codex_home.filter(|v| !v.is_empty()) {
        None => None,
        Some(home) => match std::path::absolute(&home)
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
        {
            Some(home) => Some(home),
            None => {
                return CodexCaller::Invalid(tasty_i18n::t_fmt(
                    "cli.caller_context.invalid_codex_home",
                    &home.to_string_lossy(),
                ));
            }
        },
    };
    CodexCaller::Thread {
        thread_id: thread.to_owned(),
        codex_home,
    }
}

/// 현재 프로세스 환경의 Codex 호출자.
pub fn codex_caller() -> CodexCaller {
    codex_caller_from(
        std::env::var_os("CODEX_THREAD_ID"),
        std::env::var_os("CODEX_HOME"),
    )
}

/// plugin 인자를 채울 때 `TASTY_SURFACE_ID`를 써도 되는지 판정한다.
/// Codex 호출자가 있으면 쓰지 않는다(`Ok(None)`). Invalid면 오류다.
fn env_surface(codex: &CodexCaller, tasty_surface: Option<&str>) -> Result<Option<u32>, String> {
    match codex {
        CodexCaller::Absent => Ok(tasty_surface.and_then(|s| s.parse().ok())),
        CodexCaller::Thread { .. } => Ok(None),
        CodexCaller::Invalid(message) => Err(message.clone()),
    }
}

/// dynamic plugin 요청의 surface 자동 채움과 `_tasty_context`를 한 번에 정한다.
/// `declares` 는 plugin 인자 그룹이 해당 이름의 u32 인자를 선언했는지 알려 준다.
/// 명시한 값은 바꾸지 않는다. 채운 키는 `implicit` 에 남겨 받는 쪽이 출처를 구분하게 한다.
pub fn apply_to_plugin_params(
    params: &mut Map<String, Value>,
    declares: impl Fn(&str) -> bool,
    codex: &CodexCaller,
    tasty_surface: Option<&str>,
) -> Result<(), String> {
    if params.contains_key(CONTEXT_KEY) {
        return Err(tasty_i18n::t_fmt(
            "cli.caller_context.reserved_param",
            CONTEXT_KEY,
        ));
    }
    let env = env_surface(codex, tasty_surface)?;
    let mut implicit: Vec<&str> = Vec::new();
    if declares("surface")
        && !params.contains_key("surface")
        && !params.contains_key("surface_id")
        && let Some(sid) = env
    {
        params.insert("surface".into(), Value::from(sid));
        params.insert("surface_id".into(), Value::from(sid));
        implicit.extend(["surface", "surface_id"]);
    }
    // 명시한 --surface 도 surface_id 키로 맞춘다. IPC 처리기가 surface_id 만 볼 수 있다.
    if let Some(v) = params.get("surface").cloned() {
        params.entry(String::from("surface_id")).or_insert(v);
    }
    // 호출자 surface는 대상 surface와 구분하며 호스트 키로 복제하지 않는다.
    if declares("caller_surface")
        && !params.contains_key("caller_surface")
        && let Some(sid) = env
    {
        params.insert("caller_surface".into(), Value::from(sid));
        implicit.push("caller_surface");
    }

    let mut context = Map::new();
    if let CodexCaller::Thread {
        thread_id,
        codex_home,
    } = codex
    {
        context.insert("provider".into(), Value::from("codex"));
        context.insert("thread_id".into(), Value::from(thread_id.as_str()));
        if let Some(home) = codex_home {
            context.insert("codex_home".into(), Value::from(home.as_str()));
        }
    }
    if !implicit.is_empty() {
        context.insert("implicit".into(), Value::from(implicit));
    }
    if !context.is_empty() {
        params.insert(CONTEXT_KEY.into(), Value::Object(context));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
