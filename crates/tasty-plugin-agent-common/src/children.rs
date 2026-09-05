//! 호스트 `terminal.children` 응답을 읽는 헬퍼.

use serde_json::Value;

/// 자식 항목의 `state` 문자열. 없으면 `None` — 상태를 모르는 것과 특정 상태인 것을
/// 호출자가 가를 수 있게 남긴다.
pub fn state_of(child: &Value) -> Option<&str> {
    child.get("state").and_then(|s| s.as_str())
}

/// 조건을 만족하는 자식들의 `index` 목록.
pub fn indices_with(children: &[Value], pred: impl Fn(&Value) -> bool) -> Vec<u64> {
    children
        .iter()
        .filter(|c| pred(c))
        .filter_map(|c| c.get("index").and_then(|i| i.as_u64()))
        .collect()
}

/// index 목록을 사람이 읽는 한 줄로.
pub fn join_indices(indices: &[u64]) -> String {
    indices
        .iter()
        .map(u64::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn index_less_children_are_dropped_not_defaulted() {
        let children = vec![
            json!({ "index": 1, "state": "idle" }),
            json!({ "state": "idle" }),
            json!({ "index": 3, "state": "running" }),
        ];
        let idle = indices_with(&children, |c| state_of(c) == Some("idle"));
        assert_eq!(idle, vec![1], "index 없는 항목이 0 으로 둔갑하지 않는다");
        assert_eq!(join_indices(&[1, 3]), "1, 3");
        assert_eq!(join_indices(&[]), "");
    }
}
