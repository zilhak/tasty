//! IpcSequence 실행 코어 — owner 가 고정한 IPC 호출들을 페이로드 값으로 채워
//! 순차 실행한다.
//!
//! ## 불변식 강제
//! - **데이터/흐름 분리**: 치환은 [`substitute_params`] 로 **params 값 노드에만**
//!   적용된다. `IpcCall::method` 는 이 모듈의 어떤 함수에도 인자로 넘어가지 않으므로
//!   페이로드가 method 자리에 도달할 코드 경로가 없다. 객체 key 위치도 치환하지
//!   않는다(값 leaf string 만).
//! - **단방향(fire-and-forget)**: [`execute_sequence`] 는 `()` 를 반환한다. 각 IPC
//!   호출 결과는 내부 로깅에만 쓰이고 호출자(웹훅 ACK 빌더)로 되돌아가지 않는다 —
//!   시그니처상 실행 결과가 ACK 경로로 샐 수 없다.

use std::collections::BTreeMap;
use std::sync::OnceLock;
use std::time::Duration;

use regex::Regex;
use serde_json::Value;

use super::types::IpcCall;
use crate::adapters::ipc::host_call::HostIpcInjector;

/// IpcSequence 한 스텝의 응답 대기 상한. 메인루프 tick + 핸들러 처리 시간 포함.
const STEP_TIMEOUT: Duration = Duration::from_secs(10);

/// 치환 컨텍스트 — HTTP 요청(또는 이벤트)에서 추출한 값들.
///
/// `body` 는 파싱된 JSON(파싱 실패/비-JSON 이면 `Null`), `headers` 는 소문자 정규화
/// 이름→값, `query` 는 쿼리 파라미터 이름→값.
#[derive(Debug, Clone, Default)]
pub struct SubstitutionContext {
    pub body: Value,
    pub headers: BTreeMap<String, String>,
    pub query: BTreeMap<String, String>,
}

/// `${scope.path}` 플레이스홀더 매처. `scope` ∈ {body, header, query}.
fn placeholder_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\$\{([^}]+)\}").expect("valid placeholder regex"))
}

/// 문자열 전체가 정확히 하나의 `${...}` 인 경우 내부 참조를 반환.
fn whole_placeholder(s: &str) -> Option<&str> {
    let inner = s.strip_prefix("${")?.strip_suffix('}')?;
    // 내부에 추가 `${` 나 `}` 가 있으면 "전체가 단일 플레이스홀더" 가 아니다.
    if inner.contains("${") || inner.contains('}') {
        return None;
    }
    Some(inner)
}

/// JSON path (`a.b.0.c`) 를 따라 값을 찾는다. 객체 key + 배열 인덱스 지원.
fn resolve_json_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut cur = root;
    for seg in path.split('.') {
        cur = match cur {
            Value::Object(map) => map.get(seg)?,
            Value::Array(arr) => {
                let idx: usize = seg.parse().ok()?;
                arr.get(idx)?
            }
            _ => return None,
        };
    }
    Some(cur)
}

/// `${scope.path}` 참조를 해소한다. 미해소 시 `None`.
fn resolve_ref(reference: &str, ctx: &SubstitutionContext) -> Option<Value> {
    let (scope, path) = reference.split_once('.')?;
    match scope {
        "body" => resolve_json_path(&ctx.body, path).cloned(),
        // HTTP 헤더 이름은 대소문자 무시 — 소문자 정규화 후 조회.
        "header" => ctx
            .headers
            .get(&path.to_ascii_lowercase())
            .map(|s| Value::String(s.clone())),
        "query" => ctx.query.get(path).map(|s| Value::String(s.clone())),
        _ => None,
    }
}

/// 해소된 값을 문자열 임베드용으로 렌더. 문자열이면 그대로, 그 외는 JSON 표현.
fn render_embedded(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// 문자열 leaf 치환. 전체가 단일 `${...}` 이면 해소된 **JSON 타입을 보존**하고,
/// 임베드(`"hello ${body.name}"`)면 텍스트로 치환한다.
fn substitute_string(s: &str, ctx: &SubstitutionContext) -> Value {
    if let Some(inner) = whole_placeholder(s) {
        return resolve_ref(inner, ctx).unwrap_or(Value::Null);
    }
    let replaced = placeholder_re().replace_all(s, |caps: &regex::Captures| {
        resolve_ref(&caps[1], ctx)
            .as_ref()
            .map(render_embedded)
            .unwrap_or_default()
    });
    Value::String(replaced.into_owned())
}

/// params 템플릿에 페이로드를 치환한다.
///
/// **값 노드(leaf string)에만** `${...}` 를 해석한다. 객체 key 는 절대 건드리지
/// 않으며, method 는 이 함수에 인자로 넘어오지 않는다(데이터/흐름 분리 불변식).
pub fn substitute_params(template: &Value, ctx: &SubstitutionContext) -> Value {
    match template {
        Value::String(s) => substitute_string(s, ctx),
        Value::Array(a) => Value::Array(a.iter().map(|v| substitute_params(v, ctx)).collect()),
        Value::Object(o) => Value::Object(
            o.iter()
                .map(|(k, v)| (k.clone(), substitute_params(v, ctx)))
                .collect(),
        ),
        other => other.clone(),
    }
}

/// IpcSequence 를 **fire-and-forget** 로 실행한다.
///
/// 각 스텝의 `method` 는 owner 가 고정한 리터럴(치환 대상 아님)이고, `params` 만
/// 페이로드로 치환한다. IPC 응답은 내부 로깅에만 쓰이고 반환하지 않는다 — 이
/// 함수가 `()` 를 반환하므로 실행 결과가 웹훅 ACK 로 샐 수 없다(단방향 불변식).
///
/// MVP: 한 스텝이 실패해도 다음 스텝을 계속 진행한다(관측만). 조건분기는 후속.
pub fn execute_sequence(injector: &HostIpcInjector, calls: &[IpcCall], ctx: &SubstitutionContext) {
    for (i, call) in calls.iter().enumerate() {
        let params = substitute_params(&call.params, ctx);
        match injector.dispatch(&call.method, params, STEP_TIMEOUT) {
            Ok(_result) => {
                tracing::debug!("webhook IpcSequence step {i} ({}) ok", call.method);
            }
            Err(e) => {
                tracing::warn!(
                    "webhook IpcSequence step {i} ({}) failed: {e}",
                    call.method
                );
            }
        }
    }
}

/// `ShellCommand` action 을 **fire-and-forget** 로 실행한다(worker thread spawn).
///
/// `hook_handler.dispatch` 가 셸 핸들러를 수동 발화할 때 쓴다. `tasty-hooks` 의
/// background spawn 을 미러링하되, 구조화된 `command` + `args` 를 셸 경유 없이 직접
/// exec 한다(인젝션 표면 축소). 실행 결과는 로깅에만 쓰이고 반환하지 않는다 —
/// 응답 경로(ACK/JSON)로 실행 결과가 새지 않는다(단방향 불변식).
pub fn spawn_shell(command: String, args: Vec<String>) {
    if let Err(e) = std::thread::Builder::new()
        .name("hook-shell".into())
        .spawn(move || {
            let mut cmd = std::process::Command::new(&command);
            cmd.args(&args);
            match tasty_utils::process::hide_console(&mut cmd).output() {
                Ok(_) => tracing::debug!("hook shell '{command}' ran"),
                Err(e) => tracing::warn!("hook shell '{command}' spawn failed: {e}"),
            }
        })
    {
        tracing::warn!("hook shell thread spawn failed: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx() -> SubstitutionContext {
        let mut headers = BTreeMap::new();
        headers.insert("x-signature".to_string(), "abc123".to_string());
        let mut query = BTreeMap::new();
        query.insert("token".to_string(), "qtok".to_string());
        SubstitutionContext {
            body: json!({"message": "hi", "nested": {"n": 42}, "arr": [1, 2, 3]}),
            headers,
            query,
        }
    }

    #[test]
    fn whole_placeholder_preserves_type() {
        // 전체가 단일 플레이스홀더 → JSON 타입 보존(숫자는 숫자로).
        let out = substitute_params(&json!("${body.nested.n}"), &ctx());
        assert_eq!(out, json!(42));
    }

    #[test]
    fn whole_placeholder_string_value() {
        let out = substitute_params(&json!("${body.message}"), &ctx());
        assert_eq!(out, json!("hi"));
    }

    #[test]
    fn embedded_placeholder_is_textual() {
        let out = substitute_params(&json!("msg: ${body.message}!"), &ctx());
        assert_eq!(out, json!("msg: hi!"));
    }

    #[test]
    fn header_and_query_refs() {
        let out = substitute_params(
            &json!({"sig": "${header.X-Signature}", "tok": "${query.token}"}),
            &ctx(),
        );
        assert_eq!(out, json!({"sig": "abc123", "tok": "qtok"}));
    }

    #[test]
    fn array_index_path() {
        let out = substitute_params(&json!("${body.arr.1}"), &ctx());
        assert_eq!(out, json!(2));
    }

    #[test]
    fn unresolved_ref_is_null_or_empty() {
        assert_eq!(substitute_params(&json!("${body.missing}"), &ctx()), json!(null));
        assert_eq!(
            substitute_params(&json!("x=${body.missing}"), &ctx()),
            json!("x=")
        );
    }

    #[test]
    fn object_keys_are_never_substituted() {
        // key 위치의 `${...}` 는 치환하지 않는다(데이터/흐름 분리).
        let out = substitute_params(&json!({"${body.message}": "v"}), &ctx());
        assert_eq!(out, json!({"${body.message}": "v"}));
    }

    #[test]
    fn nested_object_and_array_recurse() {
        let out = substitute_params(
            &json!({"a": ["${body.message}", {"b": "${body.nested.n}"}]}),
            &ctx(),
        );
        assert_eq!(out, json!({"a": ["hi", {"b": 42}]}));
    }
}
