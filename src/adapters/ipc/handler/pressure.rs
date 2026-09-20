//! `system.pressure` — 요청 압력 게이지 조회.
//!
//! 재는 자리는 [ADR-0305](../../../../docs/adr/0305-request-pressure-is-a-process-gauge-not-a-per-caller-observation.md)
//! 가 정했고 이 모듈은 그것을 **밖에서 읽을 수 있게** 한다. 그 ADR 이 "잃은 것" 으로
//! 적어 둔 것이 바로 노출 경로의 부재였다 — 값은 프로세스 안에만 있었고 읽는 것은
//! 시험뿐이었다.
//!
//! ## 덩어리는 **모수마다 하나**다
//!
//! 응답은 재는 모수마다 한 덩어리로 갈린다 — 오늘 넷이고, 아래에 그 넷이 한 절씩
//! 있다. **덩어리 이름 자체에 그 모수의 경계를 넣는다**: 응답을 그대로 덤프해도
//! 어느 수가 무엇을 센 것인지 갈린다.
//!
//! ★ 이 수를 세는 문장은 이 파일에만 두고 절 제목에는 서수를 쓰지 않는다. 한때
//! "두 덩어리" · "세 번째 덩어리" 로 적혀 있었고, 덩어리를 더한 커밋이 **두 번
//! 연속** 다른 문서만 고치고 여기와 CLI 도움말을 지나쳤다. 서수를 쓰면 덩어리를
//! 더할 때마다 고쳐야 할 자리가 절 수만큼 늘어난다.
//!
//! ## `queue_before_gate` / `handler_after_gate` — 게이트가 가르는 둘
//!
//! 두 값의 **모수가 다르다.** 큐 대기·깊이는 명령이 큐에서 나온 직후, 게이트보다
//! **앞**에서 재므로 뒤에 거부될 요청도 센다. handler 실행 시간은 게이트를 통과한
//! 뒤 `handle_checked_request` 에서 재므로 통과한 것만 센다. 둘을 같은 이름 아래
//! 묶으면 운영자가 "큐에 34 건이 앉았는데 handler 는 30 번 돌았다" 를 읽고도 그 차이가
//! 무엇인지 고를 수 없다.
//!
//! ## `plugin_round_trip` — **남을 기다린** 시간이다
//!
//! 위 둘은 호스트가 자기 큐와 자기 handler 에서 보낸 시간이다. `plugin_round_trip` 은
//! 호스트가 **plugin 프로세스의 답을 기다린** 시간이고, 그래서 이것이 같은 응답에
//! 있어야 운영자가 원인을 고를 수 있다 — 큐도 handler 도 빠른데 응답이 느리면 그
//! 시간은 plugin 안에 있었던 것이다.
//!
//! 그 덩어리의 모수는 위 둘과 또 다르다: **응답이 실제로 매칭된 요청만** 센다. 끝내
//! 답이 안 온 요청(취소 · deadline 만료 · plugin 종료)은 끝점이 없어 못 잰다.
//!
//! ## `db` — **디스크가 받아준** 시간이다
//!
//! `db` 는 `MemoryStore` 가 트랜잭션 commit 과 WAL checkpoint 에 쓴 시간이다. 위
//! 셋과 겹치지 않는 축이다 — commit 은 handler 시간 **안에** 들어 있으므로, 그 둘을
//! 나란히 두면 "handler 가 느리다" 와 "handler 안의 쓰기가 느리다" 가 갈린다.
//!
//! 그 덩어리 안이 다시 둘이다. `commits` 는 **성공한** 쓰기만 세고(거부는 롤백이라
//! 디스크에 남긴 것이 없다), `checkpoints` 는 부팅 때의 WAL 되감기다. 되감기는
//! 다른 커넥션이 읽는 중이면 못 끝내고 돌아오므로 `busy` 를 따로 센다 — 시간만
//! 봐서는 느린 것과 경합한 것이 안 갈린다.
//!
//! ## 아직 안 재는 값의 자리는 미리 비워 두지 않는다
//!
//! 연결 수 게이지가 뒤따를 예정이지만 빈 덩어리를 미리 넣지 않는다. 값이 0 인
//! 덩어리는 "연결이 없었다" 로 읽히고, 그것은 이 파일이 평균을 `null` 로 두는 것과
//! 정확히 같은 함정이다. 재는 자리가 생길 때 덩어리도 같이 생긴다.
//!
//! ## 두 수의 차이를 여기서 빼지 않는 이유
//!
//! `commands - calls` 는 "거부된 수" 가 **아니다.** 게이트를 통과하고도
//! `handle_checked_request` 를 안 지나는 갈래가 있다 — gui 의 app 층 메서드
//! (`App::ipc_step_app_methods`)가 그 자리에서 답하고 돌아간다. 그 차를 이름 붙여
//! 내보내면 실재하지 않는 양을 재는 것이 되므로, 두 모수를 **나란히** 두고 뺄셈은
//! 하지 않는다.
//!
//! ## 평균이 `null` 일 수 있는 이유
//!
//! 관측이 없으면 `None` 이다. 0 을 돌려주면 "기다림이 없었다" 와 "잰 적이 없다" 가
//! 같은 값이 된다 — 그 구분은 `PressureSnapshot` 이 이미 `Option` 으로 들고 있고
//! 여기서 무너뜨리지 않는다.

use serde_json::json;
use tasty_ipc::protocol::JsonRpcResponse;

/// `system.pressure` — 프로세스 수명 누계를 읽는다.
///
/// `&Core` 로 충분하다. 안이 전부 원자값이라 읽기가 요청 처리의 가변 빌림과 다투지
/// 않는다 — `Core::pressure` 가 `&self` 인 것과 같은 이유다.
pub(super) fn handle_system_pressure(
    core: &crate::core::Core,
    id: serde_json::Value,
) -> JsonRpcResponse {
    JsonRpcResponse::success(
        id,
        snapshot_json(
            &core.pressure().snapshot(),
            &core.plugin_wait().snapshot(),
            &core.db_latency().snapshot(),
        ),
    )
}

/// 스냅샷 하나를 응답 본문으로 옮긴다.
///
/// 핸들러에서 갈라 둔 이유는 시험이 **아는 값**을 넣고 자리마다 대조할 수 있게 하려는
/// 것이다. `Core` 를 세우면 그 안의 누계가 0 이 아니라 시험이 자기 입력을 못 고른다.
pub(super) fn snapshot_json(
    s: &tasty_telemetry::PressureSnapshot,
    p: &tasty_telemetry::PluginWaitSnapshot,
    d: &tasty_memory::DbLatencySnapshot,
) -> serde_json::Value {
    json!({
        "queue_before_gate": {
            "drains": s.queue_drains,
            "commands": s.queue_commands,
            "depth_max": s.queue_depth_max,
            "depth_mean": s.queue_depth_mean(),
            "wait_us_sum": s.queue_wait_us_sum,
            "wait_us_max": s.queue_wait_us_max,
            "wait_us_mean": s.queue_wait_us_mean(),
        },
        "handler_after_gate": {
            "calls": s.handler_calls,
            "us_sum": s.handler_us_sum,
            "us_max": s.handler_us_max,
            "us_mean": s.handler_us_mean(),
        },
        "plugin_round_trip": {
            "matched": p.matched,
            "us_sum": p.us_sum,
            "us_max": p.us_max,
            "us_mean": p.us_mean(),
        },
        "db": {
            "commits": d.commits,
            "commit_us_sum": d.commit_us_sum,
            "commit_us_max": d.commit_us_max,
            "commit_us_mean": d.commit_us_mean(),
            "checkpoints": d.checkpoints,
            "checkpoints_busy": d.checkpoints_busy,
            "checkpoint_us_sum": d.checkpoint_us_sum,
            "checkpoint_us_max": d.checkpoint_us_max,
            "checkpoint_us_mean": d.checkpoint_us_mean(),
        },
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tasty_memory::DbLatencyStats;
    use tasty_telemetry::{PluginWaitStats, PressureStats};

    /// 게이트 앞/뒤 두 모수가 **서로 다른 자리로** 나간다.
    ///
    /// 값을 전부 다르게 골라, 한 자리라도 다른 자리로 새면 대조가 깨지게 한다. 두
    /// 덩어리를 통째로 맞바꾸는 변경도 여기서 죽는다.
    #[test]
    fn the_two_moduli_land_in_their_own_blocks() {
        let p = PressureStats::default();
        p.record_drain(3);
        p.record_drain(5);
        p.record_queue_wait(Duration::from_micros(70));
        p.record_queue_wait(Duration::from_micros(130));
        p.record_handler(Duration::from_micros(11));

        let v = snapshot_json(
            &p.snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
        );
        let q = &v["queue_before_gate"];
        let h = &v["handler_after_gate"];

        assert_eq!(q["drains"], 2);
        assert_eq!(q["commands"], 8, "집어 든 명령 수의 합");
        assert_eq!(q["depth_max"], 5);
        assert_eq!(q["depth_mean"], 4);
        assert_eq!(q["wait_us_sum"], 200);
        assert_eq!(q["wait_us_max"], 130);
        assert_eq!(
            q["wait_us_mean"], 25,
            "분모는 명령 수(8)지 대기 관측 수가 아니다"
        );

        assert_eq!(h["calls"], 1);
        assert_eq!(h["us_sum"], 11);
        assert_eq!(h["us_max"], 11);
        assert_eq!(h["us_mean"], 11);

        assert!(
            h.get("wait_us_max").is_none() && q.get("us_max").is_none(),
            "두 모수가 같은 덩어리에 섞이면 안 된다"
        );
    }

    /// DB 지연은 **자기 덩어리**로 나가고 commit 과 checkpoint 가 섞이지 않는다.
    ///
    /// 두 모수를 다른 값으로 넣어, 한쪽이 다른 쪽 자리로 새면 대조가 깨지게 한다.
    #[test]
    fn the_db_populations_do_not_mix() {
        let d = DbLatencyStats::default();
        d.record_commit(Duration::from_micros(40));
        d.record_commit(Duration::from_micros(60));
        d.record_checkpoint(Duration::from_micros(900), false);

        let v = snapshot_json(
            &PressureStats::default().snapshot(),
            &PluginWaitStats::default().snapshot(),
            &d.snapshot(),
        );
        let db = &v["db"];
        assert_eq!(db["commits"], 2);
        assert_eq!(db["commit_us_sum"], 100);
        assert_eq!(db["commit_us_mean"], 50);
        assert_eq!(db["checkpoints"], 1);
        assert_eq!(db["checkpoint_us_max"], 900);
        assert_eq!(db["checkpoints_busy"], 1, "끝까지 못 간 되감기를 따로 센다");
        assert_eq!(
            db["commit_us_max"], 60,
            "checkpoint 시간이 commit 최댓값으로 새면 안 된다"
        );
        assert!(
            v["handler_after_gate"].get("commits").is_none(),
            "DB 모수가 handler 덩어리에 섞이면 안 된다"
        );
    }

    /// 라우터가 이 이름에 **실제로 답한다.**
    ///
    /// 위 시험들은 `snapshot_json` 만 보므로 dispatch 팔이 사라져도 살아남는다.
    /// 이것이 그 나머지 반이다 — 프로덕션 진입점(`handle_with_caller`)을 그대로
    /// 지나 덩어리들이 응답에 있는지 본다.
    #[test]
    fn the_router_answers_this_name_for_a_local_caller() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let req = tasty_ipc::protocol::JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: "system.pressure".into(),
            params: json!({}),
            session_token: None,
        };

        let resp = super::super::handle_with_caller(
            &mut core,
            &mut state,
            &mut engine,
            &req,
            &crate::ipc::caller::CallerContext::Local,
        );

        assert!(
            resp.error.is_none(),
            "local caller 는 통과한다: {:?}",
            resp.error
        );
        let result = resp.result.expect("result");
        assert!(
            result.get("queue_before_gate").is_some()
                && result.get("handler_after_gate").is_some()
                && result.get("db").is_some(),
            "덩어리들이 응답에 있어야 한다: {result}"
        );
    }

    /// plugin 은 못 부른다 — `local_only` 판정이 표에서 온다.
    ///
    /// 권한을 하나도 안 준 agent 로 부른다. 이 게이지는 caller 로 나누지 않으므로
    /// 어떤 권한을 준다고 열리는 값이 아니다(그것이 `local_only` 인 이유다).
    #[test]
    fn a_plugin_caller_is_refused_this_gauge() {
        let meta = tasty_ipc::method_meta::method_meta("system.pressure")
            .expect("표에 등재돼 있어야 한다 — 없으면 거부가 정책인지 누락인지 갈리지 않는다");
        assert!(
            !meta.plugin_callable,
            "프로세스 게이지는 caller 별 값이 아니라 plugin 표면이 아니다"
        );
        assert!(!meta.plugin_only, "local 은 부를 수 있어야 한다");
    }

    /// 관측이 없으면 평균은 `null` 이다 — 0 이 아니다.
    #[test]
    fn an_unobserved_average_is_null_not_zero() {
        let v = snapshot_json(
            &PressureStats::default().snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
        );
        assert!(v["queue_before_gate"]["wait_us_mean"].is_null());
        assert!(v["queue_before_gate"]["depth_mean"].is_null());
        assert!(v["handler_after_gate"]["us_mean"].is_null());
        assert!(v["db"]["commit_us_mean"].is_null());
        assert!(v["db"]["checkpoint_us_mean"].is_null());
        assert_eq!(
            v["queue_before_gate"]["wait_us_sum"], 0,
            "합은 0 이 맞다 — null 인 것은 평균뿐이다"
        );
    }
}
