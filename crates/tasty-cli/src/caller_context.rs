//! CLI를 실행한 호출자의 실행 환경을 플러그인 선언(`contributes.caller_context`)으로 읽는다.
//!
//! 다른 프로그램의 도구 호출 안에서 실행된 CLI는 그 프로그램이 넣은 환경변수로 호출한 대화를
//! 알 수 있다. 무엇을 읽고 어떤 형식을 요구하는지는 선언한 플러그인이 정하고, 이 모듈은 선언을
//! 해석해 dynamic plugin 요청의 `_tasty_context`에 싣기만 한다. 호스트 명령의 대상 해석과
//! 호출자 자기 보호(`caller_surface_id`)는 이 모듈과 무관하게 기존대로 동작한다.
//! 이 값들은 환경 변수일 뿐 인증된 신원이 아니다. 받는 쪽은 수신 대화와 실행 환경을 고를 때 쓴다.

use std::ffi::OsString;

use serde_json::{Map, Value};
use tasty_plugin_manifest::CallerContextDecl;
use tasty_utils::execution::ExecutionContext;

/// dynamic plugin 요청에 CLI가 붙이는 예약 키. 사용자 인자로는 받지 않는다.
pub const CONTEXT_KEY: &str = "_tasty_context";

/// 설치된 플러그인의 호출자 문맥 선언 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallerContextSource {
    pub plugin_id: String,
    pub decl: CallerContextDecl,
}

/// 선언 하나를 호출 환경에 적용한 결과.
#[derive(Debug, Clone, PartialEq)]
pub enum Caller {
    /// 선언한 ID 변수가 없거나 비어 있다.
    Absent,
    /// `_tasty_context`에 실을 필드(`provider` 포함).
    Provided {
        provider: String,
        suppress_env_surface: bool,
        fields: Map<String, Value>,
    },
    /// 값이 있으나 쓸 수 없다. 엄격 대상 명령은 다른 surface로 대신하지 않고 거절한다.
    Invalid { provider: String, message: String },
}

/// 환경 조회 함수로 선언 하나를 판정한다. 실행 파일 탐색은 하지 않아 시험에서 그대로 쓴다.
pub fn caller_from(decl: &CallerContextDecl, env: impl Fn(&str) -> Option<OsString>) -> Caller {
    let Some(id) = env(&decl.id_env).filter(|v| !v.is_empty()) else {
        return Caller::Absent;
    };
    let invalid = |message: String| Caller::Invalid {
        provider: decl.provider.clone(),
        message,
    };
    let Some(id) = id.to_str().filter(|v| decl.id_format.accepts(v)) else {
        return invalid(tasty_i18n::t_args(
            "cli.caller_context.invalid_id",
            &[&decl.id_env, &id.to_string_lossy(), &decl.provider],
        ));
    };
    let mut fields = Map::new();
    fields.insert("provider".into(), Value::from(decl.provider.as_str()));
    fields.insert(decl.id_field.clone(), Value::from(id));
    for path in &decl.path_env {
        let Some(raw) = env(&path.env).filter(|v| !v.is_empty()) else {
            continue;
        };
        let Some(absolute) = std::path::absolute(&raw)
            .ok()
            .and_then(|p| p.to_str().map(str::to_owned))
        else {
            return invalid(tasty_i18n::t_fmt2(
                "cli.caller_context.invalid_path",
                &path.env,
                &raw.to_string_lossy(),
            ));
        };
        fields.insert(path.field.clone(), Value::from(absolute));
    }
    Caller::Provided {
        provider: decl.provider.clone(),
        suppress_env_surface: decl.suppress_env_surface,
        fields,
    }
}

/// 선언 순서대로 판정하고 실행 파일을 찾는다. `Absent`는 빼고 돌려준다.
pub fn resolve(
    sources: &[CallerContextSource],
    env: impl Fn(&str) -> Option<OsString>,
    capture: impl Fn(&str) -> Option<ExecutionContext>,
) -> Vec<Caller> {
    let mut out = Vec::new();
    for source in sources {
        let mut caller = caller_from(&source.decl, &env);
        if let (Caller::Provided { fields, .. }, Some(program)) =
            (&mut caller, source.decl.executable.as_deref())
            && let Some(execution) = capture(program)
        {
            match (
                execution.executable.to_str(),
                execution.search_path.to_str(),
            ) {
                (Some(executable), Some(search_path)) => {
                    fields.insert(
                        "execution".into(),
                        serde_json::json!({
                            "executable": executable,
                            "search_path": search_path,
                        }),
                    );
                }
                _ => {
                    caller = Caller::Invalid {
                        provider: source.decl.provider.clone(),
                        message: tasty_i18n::t_fmt("cli.caller_context.invalid_execution", program),
                    }
                }
            }
        }
        if caller != Caller::Absent {
            out.push(caller);
        }
    }
    out
}

/// 현재 프로세스 환경으로 [`resolve`]한다.
pub fn current(sources: &[CallerContextSource]) -> Vec<Caller> {
    resolve(
        sources,
        |name| std::env::var_os(name),
        |program| match ExecutionContext::capture(program) {
            Ok(found) => Some(found),
            Err(error) => {
                // 조회 전용 명령은 실행 정보 없이도 쓸 수 있다.
                tracing::debug!("{program} execution context unavailable: {error}");
                None
            }
        },
    )
}

/// dynamic plugin 요청의 surface 자동 채움과 `_tasty_context`를 한 번에 정한다.
/// `declares` 는 plugin 인자 그룹이 해당 이름의 u32 인자를 선언했는지 알려 준다.
/// `strict` 는 이 명령이 형식 오류를 거절할 provider 목록이다.
/// 명시한 값은 바꾸지 않는다. 채운 키는 `implicit` 에 남겨 받는 쪽이 출처를 구분하게 한다.
pub fn apply_to_plugin_params(
    params: &mut Map<String, Value>,
    declares: impl Fn(&str) -> bool,
    callers: &[Caller],
    strict: &[String],
    tasty_surface: Option<&str>,
) -> Result<(), String> {
    if params.contains_key(CONTEXT_KEY) {
        return Err(tasty_i18n::t_fmt(
            "cli.caller_context.reserved_param",
            CONTEXT_KEY,
        ));
    }
    for caller in callers {
        if let Caller::Invalid { provider, message } = caller
            && strict.contains(provider)
        {
            return Err(message.clone());
        }
    }
    // 여러 호출자가 함께 있으면 정렬된 선언 순서의 첫 번째를 쓴다.
    let provided = callers.iter().find_map(|caller| match caller {
        Caller::Provided {
            suppress_env_surface,
            fields,
            ..
        } => Some((*suppress_env_surface, fields)),
        _ => None,
    });
    let env = match provided {
        Some((true, _)) => None,
        _ => tasty_surface.and_then(|s| s.parse::<u32>().ok()),
    };
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

    let mut context = provided.map(|(_, f)| f.clone()).unwrap_or_default();
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
