//! 요청 params 를 호스트 `terminal.*` 로 넘기기 전 다듬는 헬퍼.

use serde_json::{Map, Value};

/// 요청 params 에서 지정한 키들을 **존재할 때만** 그대로 새 Map 에 복사한다. CLI
/// 인자를 호스트 `terminal.*` 로 pass-through 하는 용도 — 없는 키를 `null` 로 채워
/// 보내면 호스트가 "값을 명시했다" 로 읽는 자리가 있어 존재 여부를 보존한다.
pub fn forward(params: &Value, keys: &[&str]) -> Map<String, Value> {
    let mut out = Map::new();
    for k in keys {
        if let Some(v) = params.get(*k) {
            out.insert((*k).to_string(), v.clone());
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn absent_keys_stay_absent_and_null_is_preserved() {
        let p = json!({ "a": 1, "b": null });
        let out = forward(&p, &["a", "b", "c"]);
        assert_eq!(out.get("a"), Some(&json!(1)));
        assert_eq!(out.get("b"), Some(&json!(null)), "명시된 null 은 보존한다");
        assert!(!out.contains_key("c"), "없는 키를 만들어내지 않는다");
    }
}
