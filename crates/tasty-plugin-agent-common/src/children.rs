//! 호스트 `terminal.children` 응답을 읽는 헬퍼.

use serde_json::{Value, json};

use crate::host_call::HostCall;

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

/// spawn 경고의 문턱값 기본치 — 설정(`spawn_child_warn_threshold`)이 없을 때.
pub const DEFAULT_SPAWN_CHILD_WARN_THRESHOLD: f64 = 6.0;

/// spawn 경고를 지을 때 쓰는 자식 인구. **문구는 안 만든다** — 두 plugin 의 i18n
/// namespace 와 placeholder 형태가 달라서, 문장은 부르는 쪽이 짓는다.
#[derive(Debug, Clone, PartialEq)]
pub struct SpawnCensus {
    pub total: usize,
    pub idle: Vec<u64>,
    /// **확정된 stale 만** 담는다 — 아래 [`spawn_census`] 의 근거 참조.
    pub stale: Vec<u64>,
    pub threshold: f64,
}

/// parent surface 의 자식들을 세어 spawn 경고의 재료를 만든다.
///
/// 이 스무 줄이 짝의 두 plugin 에 **주석까지 글자 그대로** 두 벌 있었고, 둘을 같게
/// 유지하는 것은 아무것도 없었다(그 사이 인자 순서만 갈려 있었다 —
/// `(host, parent, tr)` vs `(host, tr, parent)`).
///
/// `stale` 은 확정(`foreground_is_shell`)인 것만 센다 — `heuristic` stale 은
/// SIGSTOP·긴 추론·무출력 명령과 관측상 구별되지 않아, 그것까지 "respawn 후보" 로
/// 부르면 일하는 자식을 재시작하라고 권하게 된다. `docs/dev-guide/api-conventions.md`
/// 가 같은 이유로 `stale` 을 기본 terminal state 집합에서 뺀 것과 같은 판단이다.
///
/// 자식 목록을 못 읽으면 `None` — "자식이 0" 과 "못 물어봤다" 를 안 섞는다.
pub fn spawn_census<H: HostCall>(host: &H, parent_surface_id: u32) -> Option<SpawnCensus> {
    let resp = host
        .call("terminal.children", json!({ "surface": parent_surface_id }))
        .ok()?;
    let children = resp.get("children")?.as_array()?;
    let threshold = host
        .call(
            "settings.get_plugin_setting",
            json!({ "storage_key": "spawn_child_warn_threshold" }),
        )
        .ok()
        .and_then(|v| v.get("value").and_then(|v| v.as_f64()))
        .unwrap_or(DEFAULT_SPAWN_CHILD_WARN_THRESHOLD);
    Some(SpawnCensus {
        total: children.len(),
        idle: indices_with(children, |c| state_of(c) == Some("idle")),
        stale: indices_with(children, |c| {
            state_of(c) == Some("stale")
                && c.get("confidence").and_then(|v| v.as_str()) == Some("confirmed")
        }),
        threshold,
    })
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

    struct FakeHost(Vec<(String, Value)>);
    impl HostCall for FakeHost {
        fn call(
            &self,
            method: &str,
            _params: Value,
        ) -> Result<Value, tasty_plugin_sdk::PluginError> {
            self.0
                .iter()
                .find(|(m, _)| m == method)
                .map(|(_, v)| v.clone())
                .ok_or_else(|| tasty_plugin_sdk::PluginError::HostCall {
                    method: method.to_string(),
                    message: "mock: no response set".to_string(),
                    code: None,
                })
        }
    }

    /// 휴리스틱 stale 은 respawn 후보가 아니다 — 이 규칙이 두 plugin 에 주석까지
    /// 똑같이 두 벌 있었고, 그것을 지키는 시험은 **어느 쪽에도 없었다**.
    #[test]
    fn only_confirmed_stale_children_count_as_respawn_candidates() {
        let host = FakeHost(vec![(
            "terminal.children".into(),
            json!({ "children": [
                { "index": 0, "state": "idle" },
                { "index": 1, "state": "stale", "confidence": "confirmed" },
                { "index": 2, "state": "stale", "confidence": "heuristic" },
                // `confidence` 를 안 싣는 옛 호스트 응답 — 안 센다. 경고가 과하게
                // 나가는 것보다 안전한 쪽으로 실패한다.
                { "index": 4, "state": "stale" },
                { "index": 3, "state": "running" },
            ]}),
        )]);
        let c = spawn_census(&host, 1).expect("자식 목록을 읽었다");
        assert_eq!(c.total, 5);
        assert_eq!(c.idle, vec![0]);
        assert_eq!(c.stale, vec![1], "휴리스틱 stale 을 respawn 후보로 셌다");
        assert_eq!(
            c.threshold, DEFAULT_SPAWN_CHILD_WARN_THRESHOLD,
            "설정을 못 읽으면 기본 문턱값이다"
        );
    }

    /// 자식 목록을 못 읽으면 `None` 이다 — 그것을 "자식 0" 으로 읽으면 경고가
    /// 조용히 사라진다.
    #[test]
    fn a_failed_children_call_is_none_not_zero() {
        assert_eq!(spawn_census(&FakeHost(vec![]), 1), None);
    }
}
