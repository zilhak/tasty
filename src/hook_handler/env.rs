//! 셸 핸들러 `TASTY_HOOK_*` 환경변수 조립 (순수 함수).
//!
//! 훅 트리거/수동 발화 컨텍스트를 `ShellCommand` 자식 프로세스의 환경변수로
//! 변환한다. IpcSequence 가 `${body.*}` 치환(값슬롯)으로 받는 payload 값을 셸은
//! env 로 받는다 — 두 action 의 의미 대칭.
//!
//! ## 불변식 준수
//! - **데이터/흐름 분리**: env 는 **값 전달 전용**이다. 실행할 명령(command/args)은
//!   레지스트리 owner 가 등록 시 고정하므로 payload 가 실행 대상을 바꿀 수 없다.
//! - **플랫폼 env 제약**: NUL 문자는 양 플랫폼 모두 env 값에 올 수 없어 제거한다
//!   (Unix 는 `CString` 변환 실패로 spawn 자체가 죽고, Windows env 블록은
//!   NUL-구분이라 값이 잘린다). 값당 [`MAX_ENV_VALUE_BYTES`] 초과분은 절단 —
//!   Windows 프로세스 env 블록 전체 상한(~32KiB)을 한 payload 값이 잠식해 spawn 이
//!   깨지는 것을 막는다.

use serde_json::Value;

/// env 한 값의 상한(바이트). 초과분은 char 경계에서 절단하고 warn.
const MAX_ENV_VALUE_BYTES: usize = 4096;

/// 셸 핸들러 spawn 에 노출할 트리거 컨텍스트.
#[derive(Debug, Clone)]
pub struct HookShellEnv {
    /// `TASTY_HOOK_EVENT` — 훅 트리거는 등록 이벤트 표시 문자열(`bell` /
    /// `process-exit` / `output-match:<pattern>` / 플러그인 커스텀 키 등),
    /// `hook_handler.dispatch` 수동 발화는 핸들러 id.
    pub event: String,
    /// `TASTY_HOOK_SOURCE` — `"hook"`(내부 이벤트 트리거) | `"dispatch"`
    /// (`hook_handler.dispatch` 수동 발화). 셸은 webhook 바인딩이 구조적으로
    /// 불가하므로 `"webhook"` 은 존재하지 않는다.
    pub source: &'static str,
    /// `TASTY_HOOK_SURFACE_ID` — 훅 트리거의 발생 surface. 수동 발화는
    /// surface 무관이라 `None`(변수 미설정).
    pub surface_id: Option<u32>,
    /// payload — object 면 최상위 key 각각이 `TASTY_HOOK_<UPPER_SNAKE_KEY>` 로
    /// 노출된다(IpcSequence `${body.<key>}` 의 셸 대칭). object 외 값은 무시.
    pub payload: Value,
}

/// 컨텍스트를 `(이름, 값)` env 목록으로 조립한다.
///
/// key 정규화: ASCII 영숫자는 대문자로, 그 외 문자는 `_` 로. 정규화 결과가 이미
/// 있는 변수(예약 3종 포함)와 겹치면 **먼저 온 값이 이기고** 나머지는 warn 후
/// 무시한다(정규화 접힘 충돌 방침). 영숫자가 하나도 없는 key 는 건너뛴다.
pub fn build_env(ctx: &HookShellEnv) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = vec![
        ("TASTY_HOOK_EVENT".into(), sanitize_value(ctx.event.clone())),
        ("TASTY_HOOK_SOURCE".into(), ctx.source.to_string()),
    ];
    if let Some(sid) = ctx.surface_id {
        vars.push(("TASTY_HOOK_SURFACE_ID".into(), sid.to_string()));
    }
    if let Value::Object(map) = &ctx.payload {
        for (key, value) in map {
            let Some(fragment) = env_key_fragment(key) else {
                tracing::warn!("hook shell env: payload key '{key}' has no ASCII alphanumerics — skipped");
                continue;
            };
            let name = format!("TASTY_HOOK_{fragment}");
            if vars.iter().any(|(existing, _)| *existing == name) {
                tracing::warn!(
                    "hook shell env: payload key '{key}' collides with existing '{name}' — first value kept"
                );
                continue;
            }
            vars.push((name, sanitize_value(render_value(value))));
        }
    }
    vars
}

/// payload key → env 이름 조각. 영숫자는 대문자, 그 외는 `_`. 영숫자 0개면 `None`.
fn env_key_fragment(key: &str) -> Option<String> {
    let mut out = String::with_capacity(key.len());
    let mut has_alnum = false;
    for c in key.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_uppercase());
            has_alnum = true;
        } else {
            out.push('_');
        }
    }
    has_alnum.then_some(out)
}

/// payload 값을 env 문자열로 렌더. 문자열은 그대로, 그 외 JSON 은 compact 표현
/// (IpcSequence 치환의 `render_embedded` 와 동일 규약).
fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// env 값 정화 — NUL 제거 + [`MAX_ENV_VALUE_BYTES`] 절단(char 경계).
fn sanitize_value(mut s: String) -> String {
    if s.contains('\0') {
        tracing::warn!("hook shell env: NUL characters stripped from value");
        s = s.replace('\0', "");
    }
    if s.len() > MAX_ENV_VALUE_BYTES {
        let mut end = MAX_ENV_VALUE_BYTES;
        while !s.is_char_boundary(end) {
            end -= 1;
        }
        tracing::warn!(
            "hook shell env: value truncated from {} to {end} bytes",
            s.len()
        );
        s.truncate(end);
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ctx(surface_id: Option<u32>, payload: Value) -> HookShellEnv {
        HookShellEnv {
            event: "bell".into(),
            source: "hook",
            surface_id,
            payload,
        }
    }

    fn get<'a>(vars: &'a [(String, String)], name: &str) -> Option<&'a str> {
        vars.iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn base_vars_without_surface_or_payload() {
        let vars = build_env(&ctx(None, Value::Null));
        assert_eq!(vars.len(), 2);
        assert_eq!(get(&vars, "TASTY_HOOK_EVENT"), Some("bell"));
        assert_eq!(get(&vars, "TASTY_HOOK_SOURCE"), Some("hook"));
        assert_eq!(get(&vars, "TASTY_HOOK_SURFACE_ID"), None);
    }

    #[test]
    fn surface_id_present_when_hook_trigger() {
        let vars = build_env(&ctx(Some(7), Value::Null));
        assert_eq!(get(&vars, "TASTY_HOOK_SURFACE_ID"), Some("7"));
    }

    #[test]
    fn payload_top_level_keys_become_vars() {
        let vars = build_env(&ctx(None, json!({"repo": "tasty", "count": 3})));
        assert_eq!(get(&vars, "TASTY_HOOK_REPO"), Some("tasty"));
        // 비문자열 값은 compact JSON 표현.
        assert_eq!(get(&vars, "TASTY_HOOK_COUNT"), Some("3"));
    }

    #[test]
    fn nested_values_render_as_json() {
        let vars = build_env(&ctx(None, json!({"pr": {"id": 1}})));
        assert_eq!(get(&vars, "TASTY_HOOK_PR"), Some(r#"{"id":1}"#));
    }

    #[test]
    fn key_normalization_upper_snake() {
        let vars = build_env(&ctx(None, json!({"pr-id": "42", "리poß3": "x"})));
        assert_eq!(get(&vars, "TASTY_HOOK_PR_ID"), Some("42"));
        // 비ASCII 는 `_`, ASCII 영숫자만 승격.
        assert_eq!(get(&vars, "TASTY_HOOK__PO_3"), Some("x"));
    }

    #[test]
    fn colliding_normalized_keys_first_wins() {
        // BTreeMap 순서상 "pr-id" < "pr_id" — 먼저 온 값 유지, 뒤는 무시.
        let vars = build_env(&ctx(None, json!({"pr-id": "a", "pr_id": "b"})));
        let hits: Vec<_> = vars.iter().filter(|(n, _)| n == "TASTY_HOOK_PR_ID").collect();
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].1, "a");
    }

    #[test]
    fn payload_key_cannot_shadow_reserved_vars() {
        let vars = build_env(&ctx(Some(1), json!({"event": "spoof", "surface_id": "9"})));
        assert_eq!(get(&vars, "TASTY_HOOK_EVENT"), Some("bell"));
        assert_eq!(get(&vars, "TASTY_HOOK_SURFACE_ID"), Some("1"));
    }

    #[test]
    fn key_without_alphanumerics_is_skipped() {
        let vars = build_env(&ctx(None, json!({"---": "x"})));
        assert_eq!(vars.len(), 2);
    }

    #[test]
    fn non_object_payload_adds_nothing() {
        assert_eq!(build_env(&ctx(None, json!("str"))).len(), 2);
        assert_eq!(build_env(&ctx(None, json!([1, 2]))).len(), 2);
    }

    #[test]
    fn nul_stripped_and_long_value_truncated() {
        let long = "a".repeat(MAX_ENV_VALUE_BYTES + 100);
        let vars = build_env(&ctx(None, json!({"nul": "a\0b", "big": long})));
        assert_eq!(get(&vars, "TASTY_HOOK_NUL"), Some("ab"));
        assert_eq!(
            get(&vars, "TASTY_HOOK_BIG").map(str::len),
            Some(MAX_ENV_VALUE_BYTES)
        );
    }

    #[test]
    fn truncation_respects_char_boundary() {
        // 경계에 멀티바이트 문자가 걸리면 그 문자 전에서 자른다(패닉 없음).
        let s = format!("{}한", "a".repeat(MAX_ENV_VALUE_BYTES - 1));
        let vars = build_env(&ctx(None, json!({"k": s})));
        assert_eq!(
            get(&vars, "TASTY_HOOK_K").map(str::len),
            Some(MAX_ENV_VALUE_BYTES - 1)
        );
    }
}
