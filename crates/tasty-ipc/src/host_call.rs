//! Host→plugin sync IPC dispatch — runner thread 등 off-main thread 가 plugin
//! IPC 메서드를 동기 호출할 때 사용.
//!
//! App 의 IPC command 큐에 `IpcCommand` 를 직접 주입하고 `sync_channel(1)` 의
//! recv_timeout 으로 응답을 기다린다. `IpcWaker` 를 호출해 main loop 가 즉시
//! 깨어나도록 한다. main loop 가 다음 tick 에서 `routing.rs` 의 plugin namespace
//! 포워딩 경로로 plugin worker 에 디스패치 → plugin 응답이 같은 sync_channel 로
//! 회신되면 본 호출자가 받는다.
//!
//! ## 입장 판정
//!
//! 주입은 응답을 **시간 상한까지만** 기다린다. 메인 루프가 서 있으면 호출자는 상한에서
//! 돌아가는데 명령은 큐에 남는다 — 그 호출자가 웹훅처럼 밖에서 계속 오는 사건이면 큐가
//! 끝없이 자란다. 그래서 서버와 같은 입장 장부([`CommandAdmission`])로 큐에 든 주입 명령
//! 수를 자르고, 거절은 [`InjectError::Refused`] 로 갈라 돌려준다 — **아무것도 실행되지
//! 않았다**는 사실이 시간 초과(결과 불명)와 구별돼야 호출자가 다음 일을 고를 수 있다.
//!
//! ## 기한
//!
//! 주입은 제 응답 대기 상한을 **명령의 기한**으로도 싣는다([`HostIpcInjector::dispatch`]) — 소켓
//! 요청의 봉투 상한과 같은 자리(`JsonRpcRequest::response_timeout_ms`)다. 그래서 호출자가 상한에서
//! 물러난 명령은 큐에 남아도 꺼내질 때 실행되지 않고, 호출자는 그 사실을
//! [`InjectError::Expired`](실행 안 됨)로 받는다. 이미 시작된 명령은 끊을 수 없으므로 그때는
//! 종전대로 [`InjectError::Timeout`](결과 불명)이다 — 만료는 취소가 아니다(ADR-0411).
//!
//! 호출자가 물러나도 **실행돼야 하는** 명령(밖에서 온 사건을 반영하는 훅 스텝)은
//! [`HostIpcInjector::dispatch_even_if_abandoned`] 로 기한 없이 넣는다. 근거는
//! `docs/adr/0451-a-host-injection-carries-its-wait-as-a-deadline.md`.

use std::fmt;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::Value;

use crate::admission::{CommandAdmission, Origin, Refusal};
use crate::protocol::{
    ERR_EXPIRED_BEFORE_RUN, ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN, JsonRpcRequest, JsonRpcResponse,
};
use crate::server::{IpcCommand, IpcWaker, Withdraw};

/// 주입 호출의 실패. 갈래마다 **요청이 실행됐는가**가 다르다 — [`InjectError::nothing_ran`].
///
/// `Display` 는 이 타입 이전의 `String` 오류 문구를 그대로 낸다(거절만 새 문구다). 문자열로
/// 받아 위로 올리던 호출자(`to_string`)의 출력이 바뀌지 않게 하려는 것이다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    /// 큐 입장 장부가 거절했다 — 명령은 큐에 안 들어갔고 실행되지 않았다.
    Refused(Refusal),
    /// 큐 송신 실패(메인 루프가 큐를 버렸다) — 실행되지 않았다.
    Send(String),
    /// handler 가 JSON-RPC 에러로 답했다 — 실행됐고 거절됐다.
    Rpc {
        /// JSON-RPC 에러 코드.
        code: i32,
        /// 에러 문구.
        message: String,
    },
    /// 응답 대기 상한이 지났다 — **결과 불명**. 기한을 실은 주입([`HostIpcInjector::dispatch`])
    /// 이면 명령은 이미 시작됐고 handler 에서 계속 갈 수 있다. 기한 없는 주입이면 큐에 남아
    /// 나중에 실행될 수도 있다.
    Timeout(Duration),
    /// 응답 대기 상한이 명령이 **큐에서 기다리는 동안** 지났다 — 명령은 실행되지 않았고 앞으로도
    /// 안 된다. 기한을 실은 주입만 이 갈래를 낸다.
    Expired(Duration),
    /// 응답 통로가 답 없이 버려졌다.
    Disconnected,
}

impl InjectError {
    /// 이 실패에서 요청이 **확실히 실행되지 않았는가**. 참이면 그대로 다시 걸어도 두 번째
    /// 효과가 남지 않는다.
    pub fn nothing_ran(&self) -> bool {
        matches!(
            self,
            InjectError::Refused(_) | InjectError::Send(_) | InjectError::Expired(_)
        )
    }
}

impl fmt::Display for InjectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            InjectError::Refused(r) => {
                write!(
                    f,
                    "host_dispatch refused before queueing (nothing ran): {r}"
                )
            }
            InjectError::Send(e) => write!(f, "inject IpcCommand: {e}"),
            InjectError::Rpc { code, message } => write!(f, "rpc error {code}: {message}"),
            InjectError::Timeout(t) => write!(f, "host_dispatch timeout after {t:?}"),
            InjectError::Expired(t) => write!(
                f,
                "host_dispatch timeout after {t:?} while still queued (nothing ran)"
            ),
            InjectError::Disconnected => {
                write!(f, "host_dispatch response channel disconnected")
            }
        }
    }
}

impl std::error::Error for InjectError {}

/// Host→plugin 동기 IPC 디스패처. `Clone` — Arc 기반이라 thread 간 자유 공유.
#[derive(Clone)]
pub struct HostIpcInjector {
    sender: mpsc::Sender<IpcCommand>,
    waker: IpcWaker,
    /// 서버와 나눠 든 큐 입장 장부. 없으면 판정 없이 넣는다 — 서버 없이 채널만 세운 시험의
    /// 자리다. 제품 경로는 hub 가 [`HostIpcInjector::with_admission`] 으로 붙인다.
    admission: Option<Arc<CommandAdmission>>,
}

impl HostIpcInjector {
    pub fn new(sender: mpsc::Sender<IpcCommand>, waker: IpcWaker) -> Self {
        Self {
            sender,
            waker,
            admission: None,
        }
    }

    /// 큐 입장 장부를 붙인다. 붙은 뒤로 주입은 그 장부의 주입 깊이·바이트 판정을 받는다.
    pub fn with_admission(mut self, admission: Arc<CommandAdmission>) -> Self {
        self.admission = Some(admission);
        self
    }

    /// 붙은 큐 입장 장부 — 서버와 **같은** 장부다. 진단이 큐에 든 바이트·거절 누계를 읽는
    /// 자리([`crate::dispatch::CommandQueueSnapshot::read`])가 이것으로 장부에 닿는다. 서버 없이
    /// 채널만 세운 조립이면 `None`.
    pub fn admission(&self) -> Option<&Arc<CommandAdmission>> {
        self.admission.as_ref()
    }

    /// JSON-RPC 메서드를 동기 호출. 응답은 `Ok(result)` 또는 [`InjectError`].
    /// timeout 은 응답 대기 시간 (App tick + plugin 처리 시간 모두 포함).
    ///
    /// 그 시간이 명령의 **기한**이기도 하다 — 상한에서 물러날 때 명령이 아직 큐에 있었으면
    /// 실행되지 않고 [`InjectError::Expired`] 가 온다. 이미 시작됐으면 [`InjectError::Timeout`]
    /// (결과 불명)이다. 포기한 명령이 나중에 실행되면 호출자는 "실패" 를 적은 뒤 그 효과를
    /// 맞는다(늦은 `\r` 이 사용자가 그 사이 친 입력을 제출하는 식) — 그것을 막는 것이 기한이다.
    pub fn dispatch(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, InjectError> {
        self.dispatch_inner(method, params, timeout, true)
    }

    /// [`HostIpcInjector::dispatch`] 와 같되 **기한을 싣지 않는다** — 상한에서 물러나도 명령은
    /// 큐에 남아 나중에 실행된다. 상한 만료는 늘 [`InjectError::Timeout`] 이다.
    ///
    /// 호출자가 결과를 못 받아도 효과는 나야 하는 명령에 쓴다 — 밖에서 이미 ACK 한 사건을
    /// 반영하는 훅 스텝이 그것이다(그 사건은 다시 오지 않는다).
    pub fn dispatch_even_if_abandoned(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, InjectError> {
        self.dispatch_inner(method, params, timeout, false)
    }

    fn dispatch_inner(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
        with_deadline: bool,
    ) -> Result<Value, InjectError> {
        let (resp_tx, resp_rx) = mpsc::sync_channel::<JsonRpcResponse>(1);
        let req = JsonRpcRequest {
            response_timeout_ms: with_deadline.then(|| deadline_ms(timeout)),
            idempotency_key: None,
            jsonrpc: "2.0".to_string(),
            method: method.to_string(),
            id: Some(Value::from(1u64)),
            params,
            session_token: None,
        };
        let mut cmd = IpcCommand::new(req, resp_tx);
        if let Some(admission) = &self.admission {
            cmd.admit(admission, Origin::Injected)
                .map_err(InjectError::Refused)?;
        }
        // 실행 상태 칸의 사본을 기다리는 동안 든다 — 명령이 시작되면 그 in-flight 몫이 이
        // 호출이 돌아갈 때까지 남는다(소켓 경로와 같은 모수). 상한에서 물러날 때 "아직 시작
        // 전이었나" 를 이것으로 묻는다.
        let lifecycle = cmd.lifecycle();
        self.sender
            .send(cmd)
            .map_err(|e| InjectError::Send(e.to_string()))?;
        (self.waker)();
        // 느린 요청 링의 결과 칸은 소켓 경로와 같은 코드로 적는다 — 받은 답은 그 답으로, 상한에서
        // 물러난 갈래는 소켓 경로가 그 자리에서 보냈을 코드로(ADR-0468). 기한 없이 끝난 대기는
        // 명령이 큐에 남아 뒤에 실행될 수 있으므로 적지 않는다.
        match resp_rx.recv_timeout(timeout) {
            Ok(resp) => {
                lifecycle.record_answer(&resp);
                answer(resp, timeout, with_deadline)
            }
            // 기한이 있으면 소켓 경로(`await_dispatch_response`)와 같은 판정이다 — 아직 큐에
            // 있었으면 꺼내질 때 실행되지 않게 막는다. 기한이 없으면 명령은 큐에 남는다.
            Err(mpsc::RecvTimeoutError::Timeout) if with_deadline => match lifecycle.withdraw() {
                Withdraw::NotRun => {
                    lifecycle.record_error_code(ERR_EXPIRED_BEFORE_RUN);
                    Err(InjectError::Expired(timeout))
                }
                Withdraw::Started => {
                    lifecycle.record_error_code(ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN);
                    Err(InjectError::Timeout(timeout))
                }
            },
            Err(mpsc::RecvTimeoutError::Timeout) => Err(InjectError::Timeout(timeout)),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(InjectError::Disconnected),
        }
    }
}

/// 대기 상한을 봉투의 밀리초로 옮긴다. `0` 은 봉투 규약상 "기한 없음" 이라 1 ms 밑의 상한도 1 로
/// 싣는다 — 기한을 실으려던 호출이 기한 없는 명령이 되면 안 된다.
fn deadline_ms(timeout: Duration) -> u64 {
    u64::try_from(timeout.as_millis())
        .unwrap_or(u64::MAX)
        .max(1)
}

/// 받은 답을 호출자의 갈래로 옮긴다. 꺼낸 쪽이 기한 경과를 먼저 보면 `-32067` 을 답으로 보내므로
/// (`ipc_round::claim_or_answer`), 기한을 실은 주입에서 그 코드는 [`InjectError::Expired`] 다 —
/// 기다리는 쪽이 먼저 물러났을 때와 같은 사실이다.
fn answer(
    resp: JsonRpcResponse,
    timeout: Duration,
    with_deadline: bool,
) -> Result<Value, InjectError> {
    match resp.error {
        Some(err) if with_deadline && err.code == ERR_EXPIRED_BEFORE_RUN => {
            Err(InjectError::Expired(timeout))
        }
        Some(err) => Err(InjectError::Rpc {
            code: err.code,
            message: err.message,
        }),
        None => Ok(resp.result.unwrap_or(Value::Null)),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;
    use crate::server::Claim;

    fn noop_waker() -> IpcWaker {
        Arc::new(|| {})
    }

    #[test]
    fn dispatch_sends_command_and_returns_response() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());

        // worker thread 가 cmd 를 받아 echo 응답 전송.
        let h = thread::spawn(move || {
            let cmd = rx.recv().expect("recv cmd");
            assert_eq!(cmd.request.method, "echo.method");
            let resp = JsonRpcResponse::success(
                cmd.request.id.clone().unwrap_or(Value::Null),
                serde_json::json!({"ok": true}),
            );
            cmd.response_tx.send(resp).expect("send resp");
        });

        let v = injector
            .dispatch("echo.method", serde_json::json!({}), Duration::from_secs(2))
            .expect("dispatch ok");
        assert_eq!(v, serde_json::json!({"ok": true}));
        h.join().unwrap();
    }

    #[test]
    fn dispatch_returns_err_on_rpc_error() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());

        let (cell_tx, cell_rx) = mpsc::channel();
        let h = thread::spawn(move || {
            let cmd = rx.recv().expect("recv cmd");
            cell_tx.send(cmd.outcome_cell()).expect("cell");
            let resp = JsonRpcResponse::error(
                cmd.request.id.clone().unwrap_or(Value::Null),
                -32601,
                "Method not found",
            );
            cmd.response_tx.send(resp).expect("send resp");
        });

        let err = injector
            .dispatch("missing", Value::Null, Duration::from_secs(2))
            .expect_err("should be err");
        assert!(err.to_string().contains("-32601"));
        assert!(!err.nothing_ran(), "an rpc error means the handler ran");
        h.join().unwrap();
        assert_eq!(
            cell_rx.recv().expect("cell").get(),
            Some(tasty_telemetry::slow_requests::HostOutcome::Error { code: -32601 }),
            "the waiter records the answer it received"
        );
    }

    fn stats() -> Arc<crate::dispatch::DispatchStats> {
        Arc::new(crate::dispatch::DispatchStats::default())
    }

    // 아무도 큐를 안 비우는 동안(메인 루프가 서 있다) 상한이 지나면 명령은 **실행 안 됨** 이고,
    // 나중에 꺼내져도 실행되지 않는다. 늦은 `\r` 이 실제로 제출되던 결함의 회귀 시험이다.
    #[test]
    fn an_injection_abandoned_in_the_queue_is_expired_and_not_run_later() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());
        let err = injector
            .dispatch("surface.send", Value::Null, Duration::from_millis(30))
            .expect_err("nobody answers");
        assert_eq!(err, InjectError::Expired(Duration::from_millis(30)));
        assert!(err.nothing_ran(), "it was still queued");

        let stale = rx.try_recv().expect("the command is still queued");
        assert_eq!(
            stale.request.response_timeout_ms,
            Some(30),
            "the deadline is the caller's own wait"
        );
        assert_eq!(
            stale.claim(&stats()),
            Claim::Withdrawn,
            "taken after the caller gave up, it must not run"
        );
        assert_eq!(
            stale.outcome_cell().get(),
            Some(tasty_telemetry::slow_requests::HostOutcome::Error {
                code: ERR_EXPIRED_BEFORE_RUN
            }),
            "the slow-request row reads the code the socket path would have sent"
        );
    }

    // 상한 전에 시작된 명령은 끊을 수 없다 — 그 만료는 종전대로 결과 불명이다.
    #[test]
    fn an_injection_that_started_before_its_bound_is_an_unknown_outcome() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());
        let (started_tx, started_rx) = mpsc::channel();
        let h = thread::spawn(move || {
            let cmd = rx.recv().expect("recv cmd");
            assert_eq!(cmd.claim(&stats()), Claim::Run);
            started_tx.send(()).expect("started");
            // 답하지 않은 채 상한을 넘긴다 — 굳은 handler 다.
            thread::sleep(Duration::from_millis(200));
            cmd.outcome_cell().get()
        });
        let err = injector
            .dispatch("noop", Value::Null, Duration::from_millis(50))
            .expect_err("should time out");
        started_rx.recv().expect("the command had started");
        assert_eq!(err, InjectError::Timeout(Duration::from_millis(50)));
        assert!(
            !err.nothing_ran(),
            "a started command's timeout is an unknown outcome, not a refusal"
        );
        assert_eq!(
            h.join().unwrap(),
            Some(tasty_telemetry::slow_requests::HostOutcome::Error {
                code: ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN
            })
        );
    }

    // 기한 없는 주입은 종전 동작 그대로다 — 상한에서 물러나도 명령은 큐에 남아 나중에 실행된다.
    #[test]
    fn an_injection_even_if_abandoned_carries_no_deadline_and_runs_later() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());
        let err = injector
            .dispatch_even_if_abandoned(
                "agent.task_set_result",
                Value::Null,
                Duration::from_millis(10),
            )
            .expect_err("nobody answers");
        assert_eq!(err, InjectError::Timeout(Duration::from_millis(10)));
        assert!(!err.nothing_ran());
        let stale = rx.try_recv().expect("still queued");
        assert_eq!(stale.request.response_timeout_ms, None);
        thread::sleep(Duration::from_millis(20));
        assert_eq!(stale.claim(&stats()), Claim::Run, "it still runs");
    }

    // 꺼낸 쪽이 기한 경과를 먼저 보면 `-32067` 을 답으로 보낸다 — 기한을 실은 주입에서 그것은
    // 호출자가 먼저 물러났을 때와 같은 "실행 안 됨" 이다. 기한 없는 주입에는 그 답이 올 수 없으니
    // 그대로 handler 의 오류로 둔다.
    #[test]
    fn the_takers_not_run_answer_is_expired_only_for_a_deadline() {
        let t = Duration::from_millis(7);
        let not_run = || crate::server::expired_before_run_response(Value::from(1u64), t);
        assert_eq!(answer(not_run(), t, true), Err(InjectError::Expired(t)));
        assert!(matches!(
            answer(not_run(), t, false),
            Err(InjectError::Rpc {
                code: ERR_EXPIRED_BEFORE_RUN,
                ..
            })
        ));
    }

    #[test]
    fn a_sub_millisecond_bound_still_carries_a_deadline() {
        assert_eq!(deadline_ms(Duration::from_micros(10)), 1);
        assert_eq!(deadline_ms(Duration::from_secs(5)), 5000);
    }

    fn ledger(depth: usize) -> Arc<CommandAdmission> {
        CommandAdmission::new(crate::admission::QueueLimits {
            queued_bytes: crate::admission::QUEUED_BYTES_LIMIT,
            injected_depth: depth,
        })
    }

    // 주입 깊이 상한은 **시간 초과로 돌아간 호출이 큐에 남긴 명령**을 센다(기한이 지나 실행은
    // 안 되지만 꺼내질 때까지 자리는 쥔다). 메인 루프가
    // 서 있는 동안(아무도 `rx` 를 안 비운다) 상한만큼은 들어가고 그다음은 큐에 닿지도
    // 않고 거절된다 — 그 거절이 시간 초과와 다른 갈래로 온다.
    #[test]
    fn a_stalled_queue_refuses_injection_past_the_depth_limit() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker()).with_admission(ledger(2));
        for _ in 0..2 {
            let err = injector
                .dispatch("noop", Value::Null, Duration::from_millis(10))
                .expect_err("nobody answers");
            assert_eq!(err, InjectError::Expired(Duration::from_millis(10)));
        }
        let err = injector
            .dispatch("noop", Value::Null, Duration::from_millis(10))
            .expect_err("third one is refused");
        assert!(
            matches!(
                err,
                InjectError::Refused(Refusal::InjectedDepth {
                    queued: 2,
                    limit: 2
                })
            ),
            "{err:?}"
        );
        assert!(err.nothing_ran());
        assert_eq!(
            rx.try_iter().count(),
            2,
            "the refused command never reached the queue"
        );
    }

    // 큐에서 꺼내면 몫이 돌아와 다음 주입이 다시 들어간다 — 거절이 영구 상태가 아니다.
    #[test]
    fn draining_the_queue_lets_injection_back_in() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker()).with_admission(ledger(1));
        let first = injector.dispatch("noop", Value::Null, Duration::from_millis(10));
        assert!(matches!(first, Err(InjectError::Expired(_))), "{first:?}");
        let refused = injector.dispatch("noop", Value::Null, Duration::from_millis(10));
        assert!(
            matches!(refused, Err(InjectError::Refused(_))),
            "{refused:?}"
        );

        let mut stale = rx
            .try_recv()
            .expect("the timed-out command is still queued");
        stale.mark_dequeued();
        drop(stale);

        let again = injector.dispatch("noop", Value::Null, Duration::from_millis(10));
        assert!(matches!(again, Err(InjectError::Expired(_))), "{again:?}");
    }

    #[test]
    fn the_legacy_error_strings_are_kept() {
        assert_eq!(
            InjectError::Timeout(Duration::from_millis(5)).to_string(),
            "host_dispatch timeout after 5ms"
        );
        assert_eq!(
            InjectError::Expired(Duration::from_millis(5)).to_string(),
            "host_dispatch timeout after 5ms while still queued (nothing ran)"
        );
        assert_eq!(
            InjectError::Disconnected.to_string(),
            "host_dispatch response channel disconnected"
        );
        assert_eq!(
            InjectError::Rpc {
                code: -1,
                message: "m".into()
            }
            .to_string(),
            "rpc error -1: m"
        );
    }

    /// 호스트 주입도 소켓과 같은 생성자를 지나 번호를 받는다 — JSON-RPC `id` 는 늘 `1` 인데
    /// 번호는 주입마다 다르다. 그것이 둘을 가르는 이유다.
    #[test]
    fn each_injection_gets_its_own_request_seq_while_the_rpc_id_stays_one() {
        let (tx, rx) = mpsc::channel::<IpcCommand>();
        let injector = HostIpcInjector::new(tx, noop_waker());
        let h = thread::spawn(move || {
            let mut seen = Vec::new();
            for _ in 0..2 {
                let cmd = rx.recv().expect("recv cmd");
                seen.push((cmd.request_seq(), cmd.request.id.clone()));
                let resp = JsonRpcResponse::success(Value::from(1u64), serde_json::json!({}));
                cmd.response_tx.send(resp).expect("send resp");
            }
            seen
        });
        for _ in 0..2 {
            injector
                .dispatch("echo.method", serde_json::json!({}), Duration::from_secs(2))
                .expect("dispatch ok");
        }
        let seen = h.join().unwrap();
        assert_eq!(seen[0].1, Some(Value::from(1u64)));
        assert_eq!(seen[0].1, seen[1].1, "the rpc id does not tell them apart");
        assert!(seen[0].0 < seen[1].0, "{:?}", seen);
    }
}
