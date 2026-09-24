//! 사건 피드 조회. 소비자가 커서를 보관하고 매 요청에 보낸다.
//! 보관 범위와 위치 처리는 tasty_host_plugin::event_bus가 담당한다.

use tasty_ipc::protocol::JsonRpcResponse;

use crate::plugin::event_bus::{EVENT_RING_CAPACITY, EventBus};

/// 한 번에 돌려주는 사건 수의 기본값.
const DEFAULT_MAX: usize = 256;

/// 반환 건수의 상한은 링 용량과 같다. 나머지는 next_offset으로 이어 받는다.
const MAX_MAX: usize = EVENT_RING_CAPACITY;

/// wait_ms의 상한. 더 긴 대기는 이 값으로 제한한다.
const MAX_WAIT_MS: u64 = 60_000;

/// GUI와 헤드리스가 공유하는 사건 조회 인자.
pub(crate) struct FetchParams {
    pub offset: u64,
    pub max: usize,
    pub filter: Option<String>,
    pub wait: std::time::Duration,
}

impl FetchParams {
    /// 생략한 인자는 기본값을 쓰지만 잘못된 타입은 거절한다.
    /// 잘못된 offset을 0으로 바꾸면 이미 읽은 사건을 다시 반환하게 된다.
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

/// wait가 0보다 크면 호출 스레드가 대기하므로 워커에서 호출해야 한다.
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
            "ahead_of_stream": got.ahead_of_stream,
            "stream_end": got.stream_end,
        }),
    )
}

/// 버스가 준비되지 않은 상태를 사건이 없는 빈 피드와 구분한다.
pub(crate) fn no_bus(id: serde_json::Value) -> JsonRpcResponse {
    JsonRpcResponse::error(id, -32000, "plugin manager not initialized")
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(zero.max, 1, "max=0은 최소 1건으로 보정해야 한다");
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
        let r = FetchParams::parse(
            &serde_json::json!({ "offset": "12" }),
            &serde_json::json!(1),
        );
        let Err(resp) = r else {
            panic!("문자열 위치는 거절한다");
        };
        assert!(resp.error.is_some());
    }

    // 미래 위치 응답에서도 요청 위치와 빈 결과를 유지한다.
    #[test]
    fn the_answer_carries_the_ahead_marker_next_to_the_old_fields() {
        let bus = EventBus::new();
        let resp = fetch(
            &bus,
            &ok(serde_json::json!({ "offset": 7 })),
            serde_json::json!(1),
        );
        let r = resp.result.expect("정상 답");
        assert_eq!(r["ahead_of_stream"], serde_json::json!(true));
        assert_eq!(r["stream_end"], serde_json::json!(0));
        assert_eq!(r["next_offset"], serde_json::json!(7));
        assert_eq!(r["truncated"], serde_json::json!(false));
        assert_eq!(r["skipped"], serde_json::json!(0));
        assert_eq!(r["events"], serde_json::json!([]));
        assert!(r["epoch"].is_u64());
    }
}
