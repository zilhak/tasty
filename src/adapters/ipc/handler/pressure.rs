//! system.pressure는 프로세스 진단값을 읽는다. 각 항목은 측정 대상이 다르다(ADR-0008).
//!
//! ## 요청 처리 시간
//!
//! - queue_before_gate: 큐에서 꺼낸 명령의 대기 시간·깊이. 이후 거절되는 요청도 포함한다.
//! - handler_after_gate: 진입 검사를 통과해 handle_checked_request를 실행한 시간.
//! - plugin_round_trip: 플러그인에 전달한 뒤 실제 응답과 연결된 요청의 대기 시간.
//!   취소·기한 만료·플러그인 종료로 응답이 없으면 포함하지 않는다.
//! - db: MemoryStore의 성공한 commit과 부팅 시 WAL checkpoint 시간.
//!   commit은 handler 시간에도 포함된다. 완료하지 못한 checkpoint는 busy로 따로 센다.
//!
//! commands와 calls의 차이를 거절 수로 해석하지 않는다. App에서 바로 처리되어
//! handle_checked_request를 거치지 않는 요청도 있다. 거절 수는 gate_refusals로 확인한다.
//!
//! ## 연결과 스트림
//!
//! connections는 요청 수가 아닌 TCP 연결을 센다. 요청을 보내지 않는 attach·mesh 스트림도
//! 포함한다. 포화 거절은 요청 처리 전이므로 ipc_calls에는 포함되지 않는다.
//! live는 현재 연결 수, live_max는 최댓값, accepted/refused_saturated는 누계다.
//! limit은 서버의 실제 연결 상한이다.
//! accept_wait_bound_us_*는 accept 큐의 실제 대기 시간이 아니라 상한이다.
//! 큐가 마지막으로 비어 있던 때부터 경과한 시간을 기록하며, accept_waits는 수락·거절을
//! 포함해 꺼낸 연결 수다. 요청을 읽기 전이므로 queue_before_gate와 구분한다.
//!
//! stream_push는 attach·mesh 프레임 전송을 센다. frames_dropped/clients_lagged_out는
//! 누계이고 backlog는 현재 연결들의 적재량이다. sink_capacity는 연결 하나의 상한이다.
//! 성공 시 0으로 초기화되는 연결별 연속 drop 수는 여기서 제공하지 않는다.
//! 스트림 허브가 주입되지 않은 시험 구성에서는 null이다.
//!
//! ## DB 설정
//!
//! db_pragmas는 DB를 열 때 요청한 pragma와 다시 읽은 실제값을 memory_db/state_db별로
//! 반환한다. journal_mode, synchronous, foreign_keys, journal_size_limit을 확인한다.
//! 해당 DB 모드의 허용값이 아니면 degraded로 표시하되 DB는 열린 상태로 사용한다.
//! in-memory DB는 WAL 요청을 그대로 적용하지 않을 수 있다.
//! state_db가 null이면 이 프로세스에서 열리지 않은 상태다. 헤드리스 또는 GUI 초기화 실패를
//! 이 값만으로 구분하지는 못한다. memory_db의 null은 저장소 없는 시험 구성이다.
//!
//! ## 큐와 멱등성
//!
//! queue_admission은 큐 진입을 시도한 요청을 센다. 현재 바이트·명령·주입 명령 수는
//! 감소할 수 있다. peak_bytes는 지금까지의 최대 크기, refused_bytes/refused_depth는 거절 누계다.
//! limit_bytes/limit_injected_depth는 실제 적용하는 상한이다. 서버가 없는 구성에서는 null이다.
//! queue_dispatch는 처리 회차, 개수·시간 예산으로 멈춘 회차, 실행 전 만료, 시작한 요청을 센다.
//! in_flight는 실행을 시작한 뒤 명령 또는 응답 대기자가 CommandLifecycle을 보유하는 동안
//! 유지된다. 대기자가 먼저 종료돼도 명령이 남아 있으면 줄지 않는다. in_flight_max는 최댓값이다.
//! queued_commands와 started의 차이는 대기 중인 수가 아니다. 만료·대기자 종료 조건이 다르다.
//!
//! keyed_requests는 키가 있는 요청의 판정 누계다. executed/replayed/conflicted/discarded/
//! in_flight를 요청을 맡은 라우터에서 한 번 센다. 여기의 in_flight는 진행 중인 기존 요청을
//! 만난 횟수이며 queue_dispatch의 현재 실행 수와 다르다.
//!
//! ## 느린 요청
//!
//! slow_requests는 호스트 IPC 큐를 거친 요청 중 threshold_us를 넘은 기록이다.
//! request_seq는 호스트 번호이며 JSON-RPC id나 Event Bus trace_id가 아니다.
//! method는 표준 이름으로 바꾸고 모르는 이름은 MAX_METHOD_BYTES까지 보관한다.
//! caller, queue_wait_us, host_us, outcome/error_code, plugin_hops, total_us를 반환한다.
//! host_us에는 진입 검사도 포함된다. 응답 전 outcome/error_code는 null이다.
//! 각 hop은 plugin_id, host_request_id, wait_us, outcome을 담는다.
//!
//! 큐를 거치지 않는 플러그인 host-call은 포함되지 않는다. file_handler.dispatch가 별도 큐로
//! 넘긴 요청도 원래 번호를 잃어 그 plugin hop을 연결하지 못한다. 조회 자체가 기록을 밀어내지
//! 않도록 system.pressure는 제외한다. 메모리 링이 차면 오래된 기록부터 지운다.
//! admitted는 누계이며 현재 행 수와의 차이가 퇴출된 수다. host가 null인 행은 원래 기록이
//! 퇴출된 뒤 plugin hop만 도착한 경우다.
//!
//! ## 진입 검사 거절
//!
//! gate_refusals는 check_request에서 판정한 모든 요청을 judged로 센다. Local과 큐를
//! 거치지 않는 host-call도 포함한다. permission_denied(-32001), cap_blocked(-32007),
//! throttled(-32010) 순서로 검사하고 첫 거절에서 끝나므로 한 요청이 중복 집계되지 않는다.
//! permission_denied에는 플러그인·에이전트의 없는 메서드와 허용되지 않은 메서드도 포함한다.
//! Local의 없는 메서드는 이 항목에 들지 않는다. 검사 전 토큰 거절과 검사 후 핸들러의
//! -32001도 포함되지 않는다. 키 길이 오류는 거절 항목에 들지 않으며 engine 없는 구간의
//! check_without_engine 거절은 judged에도 포함하지 않는다.
//! 이 값은 재시작 때 초기화된다. 저장되는 rate_limit_status.throttled_count와는 범위가 다르다.
//!
//! ## 분포와 미관측 값
//!
//! 큐 대기·handler·플러그인 대기에는 각각 bounds_us와 counts를 함께 제공한다.
//! counts는 누적값이 아니며 서로 겹치지 않는 구간의 건수다. bounds_us보다 한 칸 많고
//! 마지막은 상한 초과 구간이다. 합은 관측 수이며 관측된 최대 시간은 us_max로 확인한다.
//! DB와 연결에는 분포가 없다. DB 계측은 다른 크레이트의 타입을 사용하며 accept 값은 상한이다.
//! 측정하지 않은 항목을 0으로 채우지 않는다. 관측이 없는 평균도 null로 구분한다.

use serde_json::json;
use tasty_ipc::protocol::JsonRpcResponse;

/// 읽기 전용 진단 조회. engine에서는 주입된 스트림 허브를 얻는다.
pub(super) fn handle_system_pressure(
    core: &crate::core::Core,
    engine: &crate::core::CoreState,
    id: serde_json::Value,
) -> JsonRpcResponse {
    let mut body = snapshot_json(
        &core.pressure().snapshot(),
        &core.plugin_wait().snapshot(),
        &core.db_latency().snapshot(),
        &core.connections().snapshot(),
    );
    let state_db = crate::db::with_state_db(|db| db.applied_pragmas.clone());
    body["db_pragmas"] = db_pragmas_json(
        core.memory_pragmas(),
        core.memory_init_fallback(),
        state_db.as_ref(),
    );
    body["stream_push"] = stream_push_json(engine.attach.notifier().map(|hub| hub.loss()));
    // 큐 상태는 IPC 주입기가 가진 입장 기록에서 읽는다. 서버가 없으면 null이다.
    let ledger = core
        .host_ipc_injector
        .get()
        .and_then(|injector| injector.admission());
    let queue =
        tasty_ipc::dispatch::CommandQueueSnapshot::read(ledger.map(|a| &**a), core.dispatch());
    body["queue_admission"] = queue_admission_json(queue.admission.zip(ledger.map(|a| a.limits())));
    body["queue_dispatch"] = queue_dispatch_json(&queue.dispatch);
    body["keyed_requests"] = keyed_requests_json(&super::idempotency::retry_counts());
    body["slow_requests"] = slow_requests_json(&core.slow_requests().snapshot());
    body["gate_refusals"] = gate_refusals_json(&core.gate().snapshot());
    JsonRpcResponse::success(id, body)
}

/// 큐 진입 상태와 실제 상한. 기록이 없으면 null이다.
/// 원본 구조체를 .. 없이 분해해 필드가 추가되면 응답 변환도 검토하게 한다.
pub(super) fn queue_admission_json(
    ledger: Option<(
        tasty_ipc::admission::AdmissionSnapshot,
        tasty_ipc::admission::QueueLimits,
    )>,
) -> serde_json::Value {
    match ledger {
        Some((a, limits)) => {
            let tasty_ipc::admission::AdmissionSnapshot {
                queued_bytes,
                queued_commands,
                queued_injected,
                peak_bytes,
                refused_bytes,
                refused_depth,
            } = a;
            json!({
                "queued_bytes": queued_bytes,
                "queued_commands": queued_commands,
                "queued_injected": queued_injected,
                "peak_bytes": peak_bytes,
                "refused_bytes": refused_bytes,
                "refused_depth": refused_depth,
                "limit_bytes": limits.queued_bytes,
                "limit_injected_depth": limits.injected_depth,
            })
        }
        None => serde_json::Value::Null,
    }
}

/// 큐에서 꺼낸 요청의 집계. Core에 항상 있어 null이 되지 않는다.
pub(super) fn queue_dispatch_json(d: &tasty_ipc::dispatch::DispatchSnapshot) -> serde_json::Value {
    let tasty_ipc::dispatch::DispatchSnapshot {
        rounds,
        rounds_stopped_by_count,
        rounds_stopped_by_time,
        expired_before_run,
        started,
        in_flight,
        in_flight_max,
    } = *d;
    json!({
        "rounds": rounds,
        "rounds_stopped_by_count": rounds_stopped_by_count,
        "rounds_stopped_by_time": rounds_stopped_by_time,
        "expired_before_run": expired_before_run,
        "started": started,
        "in_flight": in_flight,
        "in_flight_max": in_flight_max,
    })
}

/// 프로세스 멱등성 저장소의 판정별 누계.
pub(super) fn keyed_requests_json(r: &super::idempotency::RetryCounts) -> serde_json::Value {
    let super::idempotency::RetryCounts {
        executed,
        replayed,
        conflicted,
        discarded,
        in_flight,
    } = *r;
    json!({
        "executed": executed,
        "replayed": replayed,
        "conflicted": conflicted,
        "discarded": discarded,
        "in_flight": in_flight,
    })
}

/// 진입 검사의 판정별 누계. 원본 필드를 빠짐없이 분해한다.
pub(super) fn gate_refusals_json(g: &tasty_telemetry::GateSnapshot) -> serde_json::Value {
    let tasty_telemetry::GateSnapshot {
        judged,
        permission_denied,
        cap_blocked,
        throttled,
    } = *g;
    json!({
        "judged": judged,
        "permission_denied": permission_denied,
        "cap_blocked": cap_blocked,
        "throttled": throttled,
    })
}

/// 느린 요청 링의 현재 내용. 원본 필드를 빠짐없이 분해한다.
pub(super) fn slow_requests_json(
    s: &tasty_telemetry::slow_requests::SlowRequestsSnapshot,
) -> serde_json::Value {
    use tasty_telemetry::slow_requests::{
        HostPart, PluginHop, SLOW_REQUEST_CAPACITY, SLOW_REQUEST_THRESHOLD, SlowRequest,
        SlowRequestsSnapshot,
    };
    let SlowRequestsSnapshot { rows, admitted } = s;
    let rows: Vec<serde_json::Value> = rows
        .iter()
        .map(|row| {
            let SlowRequest {
                request_seq,
                host,
                plugin_hops,
            } = row;
            let hops: Vec<serde_json::Value> = plugin_hops
                .iter()
                .map(|hop| {
                    let PluginHop {
                        plugin_id,
                        host_request_id,
                        wait_us,
                        outcome,
                    } = hop;
                    json!({
                        "plugin_id": plugin_id,
                        "host_request_id": host_request_id,
                        "wait_us": wait_us,
                        "outcome": outcome.as_str(),
                    })
                })
                .collect();
            let host = host.as_ref().map(|h| {
                let HostPart {
                    method,
                    caller,
                    queue_wait_us,
                    host_us,
                    outcome,
                } = h;
                let outcome = outcome.get();
                json!({
                    "method": method,
                    "caller": caller.as_str(),
                    "queue_wait_us": queue_wait_us,
                    "host_us": host_us,
                    "outcome": outcome.map(|o| o.as_str()),
                    "error_code": outcome.and_then(|o| o.error_code()),
                })
            });
            json!({
                "request_seq": request_seq,
                "host": host,
                "plugin_hops": hops,
                "total_us": row.total_us(),
            })
        })
        .collect();
    json!({
        "threshold_us": u64::try_from(SLOW_REQUEST_THRESHOLD.as_micros()).unwrap_or(u64::MAX),
        "capacity": SLOW_REQUEST_CAPACITY,
        "admitted": admitted,
        "rows": rows,
    })
}

/// 스트림 손실 누계와 현재 적재량. 허브가 없으면 null이다.
pub(super) fn stream_push_json(
    loss: Option<tasty_ipc::stream_hub::StreamLossSnapshot>,
) -> serde_json::Value {
    match loss {
        Some(l) => json!({
            "frames_dropped": l.frames_dropped,
            "clients_lagged_out": l.clients_lagged_out,
            "backlog": l.backlog,
            "sink_capacity": tasty_ipc::stream_hub::SINK_CAPACITY,
        }),
        None => serde_json::Value::Null,
    }
}

/// DB를 열 때 확인한 pragma 값. memory_db 초기화 실패로 대체 저장소를 쓰면
/// init_failure에 원인을 담고 pragma가 정상이어도 degraded로 표시한다.
/// state_db 초기화 실패는 대체 없이 종료하므로 init_failure 필드가 없다(ADR-0010).
pub(super) fn db_pragmas_json(
    memory_db: Option<&tasty_memory::pragma::AppliedPragmas>,
    memory_fallback: Option<&tasty_memory::InitFallback>,
    state_db: Option<&tasty_memory::pragma::AppliedPragmas>,
) -> serde_json::Value {
    let memory = memory_db.map(|a| {
        let mut v = applied_json(a);
        if memory_fallback.is_some() {
            v["degraded"] = json!(true);
        }
        v["init_failure"] = match memory_fallback {
            Some(f) => json!({ "cause": f.cause, "error": f.error }),
            None => serde_json::Value::Null,
        };
        v
    });
    json!({
        "memory_db": memory,
        "state_db": state_db.map(applied_json),
    })
}

fn applied_json(a: &tasty_memory::pragma::AppliedPragmas) -> serde_json::Value {
    let pragmas: serde_json::Map<String, serde_json::Value> = a
        .readings
        .iter()
        .map(|r| {
            (
                r.name.to_string(),
                json!({
                    "requested": r.requested,
                    "effective": r.effective,
                    "took": r.took,
                    "error": r.error,
                }),
            )
        })
        .collect();
    json!({
        "in_memory": a.in_memory,
        "degraded": a.degraded(),
        "pragmas": pragmas,
    })
}

/// 시험에서 지정한 스냅샷을 직접 비교할 수 있도록 응답 조립을 분리한다.
pub(super) fn snapshot_json(
    s: &tasty_telemetry::PressureSnapshot,
    p: &tasty_telemetry::PluginWaitSnapshot,
    d: &tasty_memory::DbLatencySnapshot,
    c: &tasty_telemetry::ConnectionSnapshot,
) -> serde_json::Value {
    json!({
        "queue_before_gate": {
            "drains": s.queue_drains,
            "commands": s.queue_commands,
            "waits": s.queue_waits,
            "depth_max": s.queue_depth_max,
            "depth_mean": s.queue_depth_mean(),
            "wait_us_sum": s.queue_wait_us_sum,
            "wait_us_max": s.queue_wait_us_max,
            "wait_us_mean": s.queue_wait_us_mean(),
            "wait_us_hist": hist_json(&s.queue_wait_hist),
        },
        "handler_after_gate": {
            "calls": s.handler_calls,
            "us_sum": s.handler_us_sum,
            "us_max": s.handler_us_max,
            "us_mean": s.handler_us_mean(),
            "us_hist": hist_json(&s.handler_hist),
        },
        "plugin_round_trip": {
            "matched": p.matched,
            "us_sum": p.us_sum,
            "us_max": p.us_max,
            "us_mean": p.us_mean(),
            "us_hist": hist_json(&p.hist),
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
        "connections": {
            "live": c.live,
            "live_max": c.live_max,
            "limit": crate::adapters::production::tcp_ipc_server::MAX_CONCURRENT_CONNECTIONS,
            "accepted": c.accepted,
            "refused_saturated": c.refused_saturated,
            "accept_waits": c.accept_waits,
            "accept_wait_bound_us_sum": c.accept_wait_bound_us_sum,
            "accept_wait_bound_us_max": c.accept_wait_bound_us_max,
            "accept_wait_bound_us_mean": c.accept_wait_bound_us_mean(),
        },
    })
}

/// 소비자가 구간을 따로 복제하지 않도록 분포 값과 경계를 함께 반환한다.
fn hist_json(h: &tasty_telemetry::HistogramSnapshot) -> serde_json::Value {
    json!({
        "bounds_us": tasty_telemetry::HistogramSnapshot::bounds_us(),
        "counts": h.counts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tasty_memory::DbLatencyStats;
    use tasty_telemetry::{ConnectionStats, PluginWaitStats, PressureStats};

    // 값이 서로 달라야 잘못된 필드로 옮긴 경우도 검출한다.
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
            &ConnectionStats::default().snapshot(),
        );
        let q = &v["queue_before_gate"];
        let h = &v["handler_after_gate"];

        assert_eq!(q["drains"], 2);
        assert_eq!(q["commands"], 8, "처리 대상으로 꺼낸 명령 수의 합");
        assert_eq!(q["depth_max"], 5);
        assert_eq!(q["depth_mean"], 4);
        assert_eq!(q["wait_us_sum"], 200);
        assert_eq!(q["wait_us_max"], 130);
        assert_eq!(
            q["waits"], 2,
            "대기를 기록한 명령 수 — 회차 합(8)과 따로 센다"
        );
        assert_eq!(
            q["wait_us_mean"], 100,
            "평균의 분모는 대기 측정 수(2)이며 회차별 명령 수의 합(8)이 아니다"
        );

        assert_eq!(h["calls"], 1);
        assert_eq!(h["us_sum"], 11);
        assert_eq!(h["us_max"], 11);
        assert_eq!(h["us_mean"], 11);

        assert!(
            h.get("wait_us_max").is_none() && q.get("us_max").is_none(),
            "명령 수와 대기 측정 수를 구분해야 한다"
        );
    }

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
            &ConnectionStats::default().snapshot(),
        );
        let db = &v["db"];
        assert_eq!(db["commits"], 2);
        assert_eq!(db["commit_us_sum"], 100);
        assert_eq!(db["commit_us_mean"], 50);
        assert_eq!(db["checkpoints"], 1);
        assert_eq!(db["checkpoint_us_max"], 900);
        assert_eq!(
            db["checkpoints_busy"], 1,
            "busy로 완료하지 못한 checkpoint를 따로 센다"
        );
        assert_eq!(
            db["commit_us_max"], 60,
            "checkpoint 시간이 commit 최댓값으로 새면 안 된다"
        );
        assert!(
            v["handler_after_gate"].get("commits").is_none(),
            "DB 계측을 handler 계측에 포함하면 안 된다"
        );
    }

    // 응답 조립뿐 아니라 실제 라우터 연결도 검사한다.
    #[test]
    fn the_router_answers_this_name_for_a_local_caller() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let req = tasty_ipc::protocol::JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
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
                && result.get("db").is_some()
                && result.get("connections").is_some()
                && result.get("db_pragmas").is_some()
                && result.get("stream_push").is_some()
                && result.get("queue_admission").is_some()
                && result.get("queue_dispatch").is_some()
                && result.get("keyed_requests").is_some()
                && result.get("slow_requests").is_some()
                && result.get("gate_refusals").is_some(),
            "계측 항목이 응답에 포함되어야 한다: {result}"
        );
        assert!(
            result["queue_admission"].is_null(),
            "주입기가 없는데 admission 계측값이 반환됐다: {result}"
        );
        assert!(
            result["stream_push"].is_null(),
            "허브가 없는데 스트림 계측값이 반환됐다: {result}"
        );
        assert!(
            result["db_pragmas"]["memory_db"].is_null(),
            "저장소가 없는데 DB 적용값이 반환됐다: {result}"
        );
    }

    #[test]
    fn the_pragma_block_carries_requested_effective_and_degraded_per_database() {
        use tasty_memory::pragma::{AppliedPragmas, PragmaReading};
        let reading = |name, requested: &str, effective: &str, took| PragmaReading {
            name,
            requested: requested.to_string(),
            effective: Some(effective.to_string()),
            error: None,
            took,
        };
        let healthy = AppliedPragmas {
            in_memory: false,
            readings: vec![reading("journal_mode", "WAL", "wal", true)],
        };
        let degraded = AppliedPragmas {
            in_memory: false,
            readings: vec![
                reading("journal_mode", "WAL", "delete", false),
                reading("synchronous", "NORMAL", "NORMAL", true),
            ],
        };

        let v = db_pragmas_json(Some(&healthy), None, Some(&degraded));
        let m = &v["memory_db"];
        assert_eq!(m["degraded"], false);
        assert_eq!(m["in_memory"], false);
        assert_eq!(m["pragmas"]["journal_mode"]["requested"], "WAL");
        assert_eq!(m["pragmas"]["journal_mode"]["effective"], "wal");
        let s = &v["state_db"];
        assert_eq!(
            s["degraded"], true,
            "적용되지 않은 pragma가 있는데 degraded가 false다"
        );
        assert_eq!(s["pragmas"]["journal_mode"]["took"], false);
        assert_eq!(
            s["pragmas"]["journal_mode"]["effective"], "delete",
            "실제값이 요청값으로 덮이면 안 된다"
        );
        assert_eq!(s["pragmas"]["synchronous"]["took"], true);

        assert!(
            m["init_failure"].is_null(),
            "대체 저장소가 아닌데 초기화 오류가 반환됐다"
        );
        assert!(
            s.get("init_failure").is_none(),
            "state.db는 대체 저장소 오류 필드를 포함하지 않는다"
        );

        let none = db_pragmas_json(Some(&healthy), None, None);
        assert!(none["state_db"].is_null(), "열리지 않은 DB 는 null 이다");
    }

    #[test]
    fn a_fallback_memory_store_is_degraded_and_names_its_cause() {
        use tasty_memory::pragma::{AppliedPragmas, PragmaReading};
        let all_took = AppliedPragmas {
            in_memory: true,
            readings: vec![PragmaReading {
                name: "journal_mode",
                requested: "WAL".into(),
                effective: Some("memory".into()),
                error: None,
                took: true,
            }],
        };
        let fallback = tasty_memory::InitFallback {
            cause: "corrupt",
            error: "memory.db corrupted: /x/memory.db".into(),
        };
        let v = db_pragmas_json(Some(&all_took), Some(&fallback), None);
        let m = &v["memory_db"];
        assert_eq!(m["degraded"], true, "대체 저장소가 정상으로 보고됐다: {v}");
        assert_eq!(m["in_memory"], true);
        assert_eq!(m["init_failure"]["cause"], "corrupt");
        assert_eq!(
            m["init_failure"]["error"],
            "memory.db corrupted: /x/memory.db"
        );
        assert_eq!(m["pragmas"]["journal_mode"]["took"], true);
    }

    // 실제 Core 계측을 읽는지 확인한다. 새 기본값을 만들어 반환하면 실패해야 한다.
    #[test]
    fn the_connection_block_reads_the_gauge_the_core_hands_to_the_server() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let gauge = core.connections().clone();
        assert!(gauge.try_open(4).is_some());
        assert!(gauge.try_open(4).is_some());
        gauge.close();

        let req = tasty_ipc::protocol::JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
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
        let c = resp.result.expect("result")["connections"].clone();
        assert_eq!(c["live"], 1, "핸들러가 Core 의 게이지를 읽어야 한다");
        assert_eq!(c["live_max"], 2);
        assert_eq!(c["accepted"], 2);
        assert_eq!(
            c["limit"],
            crate::adapters::production::tcp_ipc_server::MAX_CONCURRENT_CONNECTIONS,
            "상한은 서버가 집행하는 그 상수여야 한다"
        );
    }

    // 주입한 허브의 손실·적재량이 실제 라우터 응답에 나오는지 확인한다.
    #[test]
    fn the_stream_block_reads_the_hub_injected_into_the_engine() {
        use tasty_ipc::stream::{StreamFrame, StreamTag};
        use tasty_ipc::stream_hub::{PushResult, SINK_CAPACITY, StreamHub};

        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let hub = StreamHub::new();
        engine.attach.set_notifier(hub.clone());

        let id = hub.alloc_id();
        let rx = hub.register(id);
        for _ in 0..SINK_CAPACITY {
            assert_eq!(
                hub.push(id, StreamFrame::new(StreamTag::Data, b"x".to_vec())),
                PushResult::Sent
            );
        }
        assert_eq!(
            hub.push(id, StreamFrame::new(StreamTag::Data, b"lost".to_vec())),
            PushResult::Dropped
        );
        rx.recv().expect("프레임 하나를 받는다");

        let req = tasty_ipc::protocol::JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
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
        let s = resp.result.expect("result")["stream_push"].clone();
        assert_eq!(s["frames_dropped"], 1, "주입된 허브의 손실이 아니다: {s}");
        assert_eq!(s["clients_lagged_out"], 0);
        assert_eq!(
            s["backlog"],
            (SINK_CAPACITY - 1) as u64,
            "꺼낸 프레임을 제외한 현재 큐 크기여야 한다: {s}"
        );
        assert_eq!(s["sink_capacity"], SINK_CAPACITY);
    }

    #[test]
    fn the_queue_and_keyed_blocks_carry_their_sources_slot_by_slot() {
        use tasty_ipc::admission::{AdmissionSnapshot, QueueLimits};
        use tasty_ipc::dispatch::DispatchSnapshot;
        let a = AdmissionSnapshot {
            queued_bytes: 1,
            queued_commands: 2,
            queued_injected: 3,
            peak_bytes: 4,
            refused_bytes: 5,
            refused_depth: 6,
        };
        let limits = QueueLimits {
            queued_bytes: 7,
            injected_depth: 8,
        };
        let q = queue_admission_json(Some((a, limits)));
        for (k, v) in [
            ("queued_bytes", 1),
            ("queued_commands", 2),
            ("queued_injected", 3),
            ("peak_bytes", 4),
            ("refused_bytes", 5),
            ("refused_depth", 6),
            ("limit_bytes", 7),
            ("limit_injected_depth", 8),
        ] {
            assert_eq!(q[k], v, "queue_admission.{k}: {q}");
        }
        assert!(
            queue_admission_json(None).is_null(),
            "admission 계측이 없는데 값이 반환됐다"
        );

        let d = queue_dispatch_json(&DispatchSnapshot {
            rounds: 11,
            rounds_stopped_by_count: 12,
            rounds_stopped_by_time: 13,
            expired_before_run: 14,
            started: 15,
            in_flight: 16,
            in_flight_max: 17,
        });
        for (k, v) in [
            ("rounds", 11),
            ("rounds_stopped_by_count", 12),
            ("rounds_stopped_by_time", 13),
            ("expired_before_run", 14),
            ("started", 15),
            ("in_flight", 16),
            ("in_flight_max", 17),
        ] {
            assert_eq!(d[k], v, "queue_dispatch.{k}: {d}");
        }

        let r = keyed_requests_json(&super::super::idempotency::RetryCounts {
            executed: 21,
            replayed: 22,
            conflicted: 23,
            discarded: 24,
            in_flight: 25,
        });
        for (k, v) in [
            ("executed", 21),
            ("replayed", 22),
            ("conflicted", 23),
            ("discarded", 24),
            ("in_flight", 25),
        ] {
            assert_eq!(r[k], v, "keyed_requests.{k}: {r}");
        }
    }

    // 실제 입장 기록·dispatch 누계·멱등성 저장소에 값을 만든 뒤 라우터로 조회한다.
    #[test]
    fn the_new_blocks_read_the_ledger_the_dispatch_gauge_and_the_process_store() {
        use tasty_ipc::admission::{CommandAdmission, Origin, QueueLimits};
        use tasty_ipc::host_call::HostIpcInjector;

        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        let limits = QueueLimits {
            queued_bytes: 50,
            injected_depth: 3,
        };
        let ledger = CommandAdmission::new(limits);
        let (tx, _rx) = std::sync::mpsc::channel();
        core.set_host_ipc_injector(
            HostIpcInjector::new(tx, std::sync::Arc::new(|| {})).with_admission(ledger.clone()),
        );
        let _held = ledger.admit(40, Origin::Socket).expect("빈 큐는 받는다");
        ledger
            .admit(20, Origin::Socket)
            .expect_err("바이트 상한을 넘는다");
        // 바이트와 깊이 거절 횟수를 다르게 해 혼동을 검출한다. 현재 적재량은 소켓 요청만 남긴다.
        {
            let _injected: Vec<_> = (0..3)
                .map(|_| ledger.admit(1, Origin::Injected).expect("깊이 상한 안"))
                .collect();
            for _ in 0..2 {
                ledger
                    .admit(1, Origin::Injected)
                    .expect_err("주입 깊이 상한을 넘는다");
            }
        }
        // 명령과 응답 대기자가 lifecycle을 보유하는 실행 중 요청을 만든다.
        let (reply_tx, _reply_rx) = std::sync::mpsc::sync_channel(1);
        let running = tasty_ipc::server::IpcCommand::new(
            tasty_ipc::protocol::JsonRpcRequest {
                response_timeout_ms: None,
                idempotency_key: None,
                jsonrpc: "2.0".into(),
                id: Some(json!(9)),
                method: "system.info".into(),
                params: json!({}),
                session_token: None,
            },
            reply_tx,
        );
        assert!(crate::app::ipc_round::claim_or_answer(
            &running,
            core.dispatch()
        ));
        core.dispatch()
            .record_round(tasty_ipc::dispatch::RoundEnd::TimeBudget);

        let call = |core: &mut crate::core::Core,
                    state: &mut crate::state::AppState,
                    engine: &mut crate::core::CoreState,
                    method: &str,
                    params: serde_json::Value,
                    key: Option<&str>| {
            let req = tasty_ipc::protocol::JsonRpcRequest {
                response_timeout_ms: None,
                idempotency_key: key.map(Into::into),
                jsonrpc: "2.0".into(),
                id: Some(json!(1)),
                method: method.into(),
                params,
                session_token: None,
            };
            super::super::handle_with_caller(
                core,
                state,
                engine,
                &req,
                &crate::ipc::caller::CallerContext::Local,
            )
        };
        for _ in 0..2 {
            call(
                &mut core,
                &mut state,
                &mut engine,
                "workspace.create",
                json!({"name": "pressure-keyed-probe"}),
                Some("pressure-keyed-probe"),
            );
        }
        let floor = super::super::idempotency::retry_counts();

        let result = call(
            &mut core,
            &mut state,
            &mut engine,
            "system.pressure",
            json!({}),
            None,
        )
        .result
        .expect("result");
        let a = &result["queue_admission"];
        assert_eq!(
            a["queued_bytes"], 40,
            "주입기의 admission 계측값과 다르다: {a}"
        );
        assert_eq!(a["queued_commands"], 1);
        assert_eq!(
            a["refused_bytes"], 1,
            "바이트 제한 거절 수가 admission 계측과 다르다: {a}"
        );
        assert_eq!(
            a["refused_depth"], 2,
            "깊이 제한 거절 수가 admission 계측과 다르다: {a}"
        );
        assert_eq!(
            a["queued_injected"], 0,
            "반환한 주입 요청이 큐 집계에 남아 있다: {a}"
        );
        assert_eq!(
            a["limit_bytes"], 50,
            "상한은 실제 admission 설정값이어야 한다"
        );
        assert_eq!(a["limit_injected_depth"], 3);
        let d = &result["queue_dispatch"];
        assert_eq!(
            d["in_flight"], 1,
            "Core의 진행 중인 dispatch 수와 다르다: {d}"
        );
        assert_eq!(d["started"], 1);
        assert_eq!(d["rounds"], 1);
        assert_eq!(d["rounds_stopped_by_time"], 1);
        // 병렬 시험이 전역 누계를 올릴 수 있어 이 시험이 만든 최솟값과 비교한다.
        let k = &result["keyed_requests"];
        assert!(
            floor.replayed >= 1
                && k["replayed"].as_u64() >= Some(floor.replayed)
                && k["executed"].as_u64() >= Some(floor.executed),
            "프로세스 보존소의 판정이 아니다: {floor:?} → {k}"
        );
    }

    // 서로 다른 구간에 값을 넣어 각 분포가 섞이지 않는지 확인한다.
    #[test]
    fn each_distribution_ships_inside_its_own_block_with_its_bounds() {
        let p = PressureStats::default();
        // 31_623 초과 100_000 이하 → counts[8]
        p.record_queue_wait(Duration::from_micros(40_000));
        // 10 이하 → counts[0]
        p.record_handler(Duration::from_micros(5));
        let w = PluginWaitStats::default();
        // 마지막 상한(1_000_000) 초과 → 넘침 칸 counts[LATENCY_BUCKET_COUNT - 1]
        w.record(Duration::from_micros(2_000_000));

        let v = snapshot_json(
            &p.snapshot(),
            &w.snapshot(),
            &DbLatencyStats::default().snapshot(),
            &ConnectionStats::default().snapshot(),
        );
        let qh = &v["queue_before_gate"]["wait_us_hist"];
        let hh = &v["handler_after_gate"]["us_hist"];
        let ph = &v["plugin_round_trip"]["us_hist"];

        let bounds = qh["bounds_us"].as_array().expect("경계 배열");
        assert_eq!(
            bounds.len(),
            tasty_telemetry::LATENCY_BUCKET_COUNT - 1,
            "분포 구간 수는 경계 수보다 하나 많아야 한다. 마지막은 상한 초과 구간이다"
        );
        assert_eq!(
            hh["bounds_us"], qh["bounds_us"],
            "각 분포는 같은 경계값을 사용해야 한다"
        );
        assert_eq!(
            ph["bounds_us"], qh["bounds_us"],
            "각 분포는 같은 경계값을 사용해야 한다"
        );

        let counts = |x: &serde_json::Value| -> Vec<u64> {
            x["counts"]
                .as_array()
                .expect("칸 배열")
                .iter()
                .map(|n| n.as_u64().expect("u64"))
                .collect()
        };
        let (q, h, pl) = (counts(qh), counts(hh), counts(ph));
        assert_eq!(q.iter().sum::<u64>(), 1);
        assert_eq!(h[0], 1, "5 µs 는 counts[0]");
        assert_eq!(q[0], 0, "큐 대기 40 ms 가 counts[0] 에 오면 안 된다");
        assert_eq!(q[8], 1, "40 ms 는 상한 100_000 인 counts[8] 이다");
        assert_eq!(
            pl[tasty_telemetry::LATENCY_BUCKET_COUNT - 1],
            1,
            "2 s 는 넘침 칸이다"
        );
        assert!(
            v["db"].get("us_hist").is_none() && v["connections"].get("us_hist").is_none(),
            "분포를 제공하지 않는 항목에는 빈 배열도 넣지 않는다"
        );
    }

    #[test]
    fn the_accept_wait_bound_ships_in_the_connection_block() {
        let c = ConnectionStats::default();
        c.record_accept_wait(Duration::from_micros(100_100));
        c.record_accept_wait(Duration::from_micros(300));
        let v = snapshot_json(
            &PressureStats::default().snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
            &c.snapshot(),
        );
        let k = &v["connections"];
        assert_eq!(k["accept_waits"], 2);
        assert_eq!(k["accept_wait_bound_us_sum"], 100_400);
        assert_eq!(k["accept_wait_bound_us_max"], 100_100);
        assert_eq!(k["accept_wait_bound_us_mean"], 50_200);
        assert_eq!(
            v["queue_before_gate"]["wait_us_sum"], 0,
            "큐 대기에 섞이면 안 된다"
        );
        let unobserved = snapshot_json(
            &PressureStats::default().snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
            &ConnectionStats::default().snapshot(),
        );
        assert!(unobserved["connections"]["accept_wait_bound_us_mean"].is_null());
    }

    #[test]
    fn a_saturated_port_is_not_a_slow_handler() {
        let c = ConnectionStats::default();
        assert!(c.try_open(1).is_some());
        assert!(c.try_open(1).is_none());

        let v = snapshot_json(
            &PressureStats::default().snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
            &c.snapshot(),
        );
        assert_eq!(v["connections"]["live"], 1);
        assert_eq!(v["connections"]["refused_saturated"], 1);
        assert_eq!(
            v["handler_after_gate"]["calls"], 0,
            "거절된 연결은 요청이 된 적이 없다"
        );
        assert!(
            v["handler_after_gate"].get("refused_saturated").is_none()
                && v["connections"].get("calls").is_none(),
            "자원 계측을 시간 계측 항목에 포함하면 안 된다"
        );
    }

    // 프로세스 전체 정보이므로 권한 부여와 관계없이 Local만 조회할 수 있어야 한다.
    #[test]
    fn a_plugin_caller_is_refused_this_gauge() {
        let meta = tasty_ipc::method_meta::method_meta("system.pressure")
            .expect("표에 등재돼 있어야 한다 — 없으면 거부가 정책인지 누락인지 갈리지 않는다");
        assert!(
            !meta.plugin_callable,
            "프로세스 전체 계측은 plugin 호출자에게 공개하지 않는다"
        );
        assert!(!meta.plugin_only, "local 은 부를 수 있어야 한다");
    }

    // 처리 회차가 끝나기 전 조회도 waits를 분모로 써 평균과 분포가 일치해야 한다.
    #[test]
    fn a_query_inside_an_open_round_keeps_the_mean_under_the_max() {
        let p = PressureStats::default();
        p.record_queue_wait(Duration::from_micros(207_972));
        p.record_queue_wait(Duration::from_micros(327));
        let v = snapshot_json(
            &p.snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
            &ConnectionStats::default().snapshot(),
        );
        let q = &v["queue_before_gate"];
        assert_eq!(q["commands"], 0, "대조군: 회차가 아직 안 끝났다");
        let mean = q["wait_us_mean"]
            .as_u64()
            .expect("대기를 쟀으니 평균이 있다");
        let max = q["wait_us_max"].as_u64().expect("최댓값");
        assert!(mean <= max, "평균 {mean} 이 최댓값 {max} 을 넘었다");
        let hist_total: u64 = q["wait_us_hist"]["counts"]
            .as_array()
            .expect("counts")
            .iter()
            .map(|c| c.as_u64().expect("정수"))
            .sum();
        assert_eq!(
            q["waits"].as_u64(),
            Some(hist_total),
            "분포의 합이 평균의 분모다"
        );
    }

    #[test]
    fn an_unobserved_average_is_null_not_zero() {
        let v = snapshot_json(
            &PressureStats::default().snapshot(),
            &PluginWaitStats::default().snapshot(),
            &DbLatencyStats::default().snapshot(),
            &ConnectionStats::default().snapshot(),
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

    // params·토큰·JSON-RPC id는 느린 요청 기록에 포함하지 않는다.
    #[test]
    fn the_slow_request_block_carries_each_row_slot_by_slot() {
        use tasty_telemetry::slow_requests::{CallerKind, HopOutcome, HostLeg, PluginHop};
        let log = tasty_telemetry::SlowRequestLog::default();
        log.note_forwarded(42);
        log.finish_host(HostLeg {
            request_seq: 42,
            method: "orig.run",
            caller: CallerKind::Local,
            queue_wait: Duration::from_micros(300),
            host: Duration::from_micros(20),
            outcome: Default::default(),
        });
        log.finish_plugin_hop(
            42,
            PluginHop {
                plugin_id: "com.example.owner".into(),
                host_request_id: 9001,
                wait_us: 250_000,
                outcome: HopOutcome::Expired,
            },
            true,
        );
        let v = slow_requests_json(&log.snapshot());
        assert_eq!(v["threshold_us"], 100_000);
        assert_eq!(v["capacity"], 32);
        assert_eq!(v["admitted"], 1);
        let row = &v["rows"][0];
        assert_eq!(row["request_seq"], 42);
        assert_eq!(row["total_us"], 250_320);
        assert_eq!(
            row["host"],
            json!({"method": "orig.run", "caller": "local", "queue_wait_us": 300, "host_us": 20,
                   "outcome": null, "error_code": null})
        );
        assert_eq!(
            row["plugin_hops"],
            json!([{"plugin_id": "com.example.owner", "host_request_id": 9001,
                    "wait_us": 250_000, "outcome": "expired"}])
        );
        let mut keys: Vec<&str> = row
            .as_object()
            .expect("row")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(keys, ["host", "plugin_hops", "request_seq", "total_us"]);
    }

    #[test]
    fn the_host_part_carries_the_answer_the_caller_got() {
        use tasty_telemetry::slow_requests::{CallerKind, HostLeg, HostOutcome, HostOutcomeCell};
        let log = tasty_telemetry::SlowRequestLog::default();
        let cells: Vec<HostOutcomeCell> = (0..3).map(|_| HostOutcomeCell::default()).collect();
        for (seq, cell) in (1..).zip(&cells) {
            log.finish_host(HostLeg {
                request_seq: seq,
                method: "workspace.list",
                caller: CallerKind::Local,
                queue_wait: Duration::from_millis(150),
                host: Duration::ZERO,
                outcome: cell.clone(),
            });
        }
        cells[0].set(HostOutcome::Ok);
        cells[1].set(HostOutcome::Error { code: -32001 });
        cells[2].set(HostOutcome::Error { code: -32067 });
        cells[2].set(HostOutcome::Ok);
        let v = slow_requests_json(&log.snapshot());
        let seen: Vec<(serde_json::Value, serde_json::Value)> = (0..3)
            .map(|i| {
                let host = &v["rows"][i]["host"];
                (host["outcome"].clone(), host["error_code"].clone())
            })
            .collect();
        assert_eq!(
            seen,
            [
                (json!("ok"), json!(null)),
                (json!("error"), json!(-32001)),
                (json!("error"), json!(-32067)),
            ],
            "두 번째 답은 첫 답을 덮지 않는다"
        );
    }

    #[test]
    fn the_gate_block_carries_its_source_slot_by_slot() {
        let v = gate_refusals_json(&tasty_telemetry::GateSnapshot {
            judged: 17,
            permission_denied: 2,
            cap_blocked: 3,
            throttled: 5,
        });
        assert_eq!(
            v,
            json!({"judged": 17, "permission_denied": 2, "cap_blocked": 3, "throttled": 5})
        );
    }

    #[test]
    fn the_router_reads_the_ring_the_core_holds() {
        let _home = crate::test_support::TastyHomeGuard::new();
        let mut core = super::super::cli_entry_tests::test_core();
        let (mut state, mut engine) = crate::state::tests::test_state();
        core.slow_requests()
            .finish_host(tasty_telemetry::slow_requests::HostLeg {
                request_seq: 77,
                method: "workspace.list",
                caller: tasty_telemetry::slow_requests::CallerKind::Local,
                queue_wait: Duration::from_millis(150),
                host: Duration::ZERO,
                outcome: Default::default(),
            });
        let req = tasty_ipc::protocol::JsonRpcRequest {
            response_timeout_ms: None,
            idempotency_key: None,
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
        let result = resp.result.expect("result");
        assert_eq!(
            result["slow_requests"]["rows"][0]["request_seq"], 77,
            "{result}"
        );
    }
}
