//! `events.fetch` — 사건 피드 조회.
//!
//! **서버는 소비자별 상태를 들지 않는다.** 커서는 소비자가 들고 매번 가져온다. 그래서
//! 같은 인자로 두 번 불러도 같은 답이 오고, 느린 소비자가 호스트 쪽에 아무것도 쌓지
//! 않는다. 이 모양의 선례가 `plugin.audit_follow` 다.
//!
//! 이 모듈은 params 를 읽어 버스에 묻고 wire 모양으로 싼다. 링·위치·보존의 규율은
//! 버스가 소유한다(`tasty_host_plugin::event_bus`).

use tasty_ipc::protocol::JsonRpcResponse;

use crate::plugin::event_bus::EventBus;

/// 한 번에 돌려주는 사건 수의 기본값·상한.
///
/// 상한이 필요한 이유는 응답 하나가 링 전체를 실어 나르는 것을 막기 위해서다.
/// 소비자는 `next_offset` 으로 이어 받으면 되므로 상한이 답을 잃게 하지 않는다.
const DEFAULT_MAX: usize = 256;
const MAX_MAX: usize = 1024;

/// `wait_ms` 의 상한. 이 값보다 긴 대기를 주면 여기서 잘린다 — 무한 대기는 응답을
/// 기다리는 쪽의 타임아웃과 어긋나면 조용히 끊긴 연결이 된다.
const MAX_WAIT_MS: u64 = 60_000;

/// `events.fetch` 의 인자. 파싱을 한 자리에 모아 두 dispatch 경로(gui · headless)가
/// 같은 규칙을 쓴다.
pub(crate) struct FetchParams {
    pub offset: u64,
    pub max: usize,
    pub filter: Option<String>,
    pub wait: std::time::Duration,
}

impl FetchParams {
    /// 모든 인자가 **선택**이다. 아무것도 안 주면 처음부터 기다리지 않고 읽는다.
    ///
    /// 다만 "안 왔다" 와 "왔는데 안 읽힌다" 는 가른다. 숫자가 아닌 `offset` 을
    /// 조용히 0 으로 읽으면 소비자가 든 위치를 버리고 **링 전체를 다시** 주고,
    /// 같은 실수가 `max`·`wait_ms` 에서는 호출자가 지정한 값 대신 기본값을 쓴다.
    /// 그래서 공용 관문(`handler::params`)을 지난다.
    pub fn parse(
        params: &serde_json::Value,
        id: &serde_json::Value,
    ) -> Result<Self, JsonRpcResponse> {
        use crate::ipc::handler::params::opt_int;
        let offset = opt_int::<u64>(params, "offset", id)?.unwrap_or(0);
        let max = opt_int::<usize>(params, "max", id)?
            .map(|v| v.clamp(1, MAX_MAX))
            .unwrap_or(DEFAULT_MAX);
        let filter = params
            .get("filter")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        let wait_ms = opt_int::<u64>(params, "wait_ms", id)?
            .unwrap_or(0)
            .min(MAX_WAIT_MS);
        Ok(Self {
            offset,
            max,
            filter,
            wait: std::time::Duration::from_millis(wait_ms),
        })
    }
}

/// 버스에 묻고 wire 모양으로 싼다. `wait` 가 0 이 아니면 **부르는 스레드를 막는다** —
/// 호출자가 워커에서 부른다.
pub(crate) fn fetch(bus: &EventBus, args: &FetchParams, id: serde_json::Value) -> JsonRpcResponse {
    let got = if args.wait.is_zero() {
        bus.fetch(args.offset, args.max, args.filter.as_deref())
    } else {
        bus.fetch_blocking(args.offset, args.max, args.filter.as_deref(), args.wait)
    };
    let events: Vec<serde_json::Value> = got
        .events
        .into_iter()
        .map(|(offset, envelope)| {
            let mut v = serde_json::to_value(&envelope).unwrap_or(serde_json::Value::Null);
            if let Some(obj) = v.as_object_mut() {
                obj.insert("offset".to_string(), serde_json::json!(offset));
            }
            v
        })
        .collect();
    JsonRpcResponse::success(
        id,
        serde_json::json!({
            "events": events,
            "next_offset": got.next_offset,
            "epoch": got.epoch,
            "truncated": got.truncated,
            "skipped": got.skipped,
        }),
    )
}

/// 매니저가 아직 없을 때의 답. 버스는 `PluginManager` 가 소유하므로 그것이 서기
/// 전에는 답할 링 자체가 없다 — **빈 피드로 답하지 않는다.** 빈 답은 "아직 아무 일도
/// 없었다" 로 읽히고, 그것은 이 시점의 사실이 아니다.
pub(crate) fn no_bus(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::error(id, -32000, "plugin manager not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 잘못된 값은 답이 아니라 **거절**이라, 시험은 정상 경로에서 벗겨 쓴다.
    fn ok(params: serde_json::Value) -> FetchParams {
        FetchParams::parse(&params, &serde_json::json!(1)).expect("정상 인자")
    }

    #[test]
    fn every_argument_is_optional_and_the_default_does_not_wait() {
        let p = ok(serde_json::json!({}));
        assert_eq!(p.offset, 0);
        assert_eq!(p.max, DEFAULT_MAX);
        assert!(p.filter.is_none());
        assert!(p.wait.is_zero());
    }

    #[test]
    fn max_is_clamped_at_both_ends() {
        let zero = ok(serde_json::json!({ "max": 0 }));
        assert_eq!(zero.max, 1, "0 을 주면 답이 영원히 비어 진전이 없다");
        let huge = ok(serde_json::json!({ "max": 999_999 }));
        assert_eq!(huge.max, MAX_MAX);
    }

    #[test]
    fn an_empty_filter_is_the_same_as_no_filter() {
        // 빈 문자열을 패턴으로 넘기면 아무 키와도 안 맞아 답이 영원히 빈다.
        let p = ok(serde_json::json!({ "filter": "" }));
        assert!(p.filter.is_none());
    }

    #[test]
    fn the_wait_has_a_ceiling() {
        let p = ok(serde_json::json!({ "wait_ms": 10_000_000u64 }));
        assert_eq!(p.wait, std::time::Duration::from_millis(MAX_WAIT_MS));
    }

    #[test]
    fn a_position_that_is_not_a_number_is_refused_rather_than_read_as_zero() {
        // 0 으로 읽으면 소비자가 든 위치를 버리고 링 전체를 다시 준다 — 그 답은
        // 틀렸다는 표시가 없어 조용하다.
        let r = FetchParams::parse(
            &serde_json::json!({ "offset": "12" }),
            &serde_json::json!(1),
        );
        let Err(resp) = r else {
            panic!("문자열 위치는 거절한다");
        };
        assert!(resp.error.is_some());
    }
}
