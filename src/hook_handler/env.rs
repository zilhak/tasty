//! 셸 명령에 전달할 TASTY_HOOK_* 값을 만든다. command·args 자체는 치환하지 않는다.
//! 값별 NUL 제거·길이 제한은 있지만 변수 개수나 전체 환경 크기를 제한하지는 않는다.

use serde_json::Value;

/// 각 값의 바이트 상한. UTF-8 문자 경계에서 자른다.
const MAX_ENV_VALUE_BYTES: usize = 4096;

#[derive(Debug, Clone)]
pub struct HookShellEnv {
    /// 등록 이벤트의 표시 문자열 또는 수동 실행의 핸들러 ID.
    pub event: String,
    /// hook 또는 수동 dispatch. 이 함수 자체는 문자열 값을 검증하지 않는다.
    pub source: &'static str,
    /// Some이면 예약 TASTY_HOOK_SURFACE_ID를 먼저 만든다.
    pub surface_id: Option<u32>,
    /// 객체의 최상위 키만 환경변수로 만든다. 다른 JSON 타입은 무시한다.
    pub payload: Value,
}

/// ASCII 영숫자를 대문자, 다른 문자를 _로 바꿔 이름을 만든다. 영숫자가 없으면 제외한다.
/// 충돌 시 먼저 넣은 값을 유지한다. 예약값과 충돌은 조용히, payload끼리 충돌은 로그로 처리한다.
/// surface_id가 None이면 예약 변수가 없어 payload의 surface_id가 그 이름을 사용할 수 있다.
pub fn build_env(ctx: &HookShellEnv) -> Vec<(String, String)> {
    let mut vars: Vec<(String, String)> = vec![
        ("TASTY_HOOK_EVENT".into(), sanitize_value(ctx.event.clone())),
        ("TASTY_HOOK_SOURCE".into(), ctx.source.to_string()),
    ];
    if let Some(sid) = ctx.surface_id {
        vars.push(("TASTY_HOOK_SURFACE_ID".into(), sid.to_string()));
    }
    let reserved_count = vars.len();
    if let Value::Object(map) = &ctx.payload {
        for (key, value) in map {
            let Some(fragment) = env_key_fragment(key) else {
                tracing::warn!(
                    "hook shell env: payload key '{key}' has no ASCII alphanumerics — skipped"
                );
                continue;
            };
            let name = format!("TASTY_HOOK_{fragment}");
            if let Some(idx) = vars.iter().position(|(existing, _)| *existing == name) {
                if idx >= reserved_count {
                    tracing::warn!(
                        "hook shell env: payload key '{key}' collides with existing '{name}' — first value kept"
                    );
                }
                continue;
            }
            vars.push((name, sanitize_value(render_value(value))));
        }
    }
    vars
}

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

/// 문자열은 그대로, 나머지는 JSON 문자열로 바꾼다.
fn render_value(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

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
        assert_eq!(get(&vars, "TASTY_HOOK__PO_3"), Some("x"));
    }

    #[test]
    fn colliding_normalized_keys_first_wins() {
        // 이 검사 구성의 JSON 맵 순서에서 pr-id가 먼저 온다.
        let vars = build_env(&ctx(None, json!({"pr-id": "a", "pr_id": "b"})));
        let hits: Vec<_> = vars
            .iter()
            .filter(|(n, _)| n == "TASTY_HOOK_PR_ID")
            .collect();
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
    fn payload_surface_id_matches_reserved_is_routine_not_exceptional() {
        let without_payload = build_env(&ctx(Some(7), Value::Null));
        let with_matching_payload = build_env(&ctx(Some(7), json!({"surface_id": 7})));
        assert_eq!(without_payload.len(), with_matching_payload.len());
        assert_eq!(
            get(&with_matching_payload, "TASTY_HOOK_SURFACE_ID"),
            Some("7")
        );
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
        let s = format!("{}한", "a".repeat(MAX_ENV_VALUE_BYTES - 1));
        let vars = build_env(&ctx(None, json!({"k": s})));
        assert_eq!(
            get(&vars, "TASTY_HOOK_K").map(str::len),
            Some(MAX_ENV_VALUE_BYTES - 1)
        );
    }
}
