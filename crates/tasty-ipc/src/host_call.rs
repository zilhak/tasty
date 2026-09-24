//! 메인 스레드 밖에서 호스트 IPC 큐에 명령을 넣고 동기 응답을 기다린다.
//! IpcWaker로 루프를 깨우며 서버와 같은 바이트·주입 개수 제한을 적용한다.
//!
//! dispatch는 응답 대기 상한을 명령 기한으로도 싣는다. 시작 전에 만료되면
//! Expired(미실행), 시작한 뒤 대기가 끝나면 Timeout(결과 불명)이다.
//! dispatch_even_if_abandoned는 기한 없이 보내 호출자가 떠나도 나중에 실행할 수 있다.

use std::fmt;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::Value;

use crate::admission::{CommandAdmission, Origin, Refusal};
use crate::protocol::{
    ERR_EXPIRED_BEFORE_RUN, ERR_RESPONSE_TIMEOUT_OUTCOME_UNKNOWN, JsonRpcRequest, JsonRpcResponse,
};
use crate::server::{IpcCommand, IpcWaker, Withdraw};

/// 주입 실패 사유. nothing_ran이 true인 경우만 실행되지 않았음이 확실하다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InjectError {
    /// 큐 입장 장부가 거절했다 — 명령은 큐에 안 들어갔고 실행되지 않았다.
    Refused(Refusal),
    /// 큐 송신 실패(메인 루프가 큐를 버렸다) — 실행되지 않았다.
    Send(String),
    /// JSON-RPC 오류 응답. 실제 실행 여부는 오류 사유에 따라 다르다.
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
    /// 서버와 공유하는 입장 장부. 서버 없이 만든 시험 채널에서는 None일 수 있다.
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

    /// 큐 통계 조회에 쓰는 공용 입장 장부. 서버 없이 조립했으면 None이다.
    pub fn admission(&self) -> Option<&Arc<CommandAdmission>> {
        self.admission.as_ref()
    }

    /// 대기 상한을 명령 기한으로 싣는다. 시작 전 만료는 Expired, 시작 후 만료는 Timeout이다.
    pub fn dispatch(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, InjectError> {
        self.dispatch_inner(method, params, timeout, true)
    }

    /// 호출자가 대기를 끝내도 나중에 실행할 명령을 기한 없이 넣는다.
    /// 이미 ACK한 이벤트를 반영하는 훅 등에 쓰며 대기 만료는 항상 Timeout이다.
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
        // 시작/철회를 판정하고 in-flight 수명을 유지하도록 대기 중 lifecycle을 보유한다.
        let lifecycle = cmd.lifecycle();
        self.sender
            .send(cmd)
            .map_err(|e| InjectError::Send(e.to_string()))?;
        (self.waker)();
        // 실제 응답 또는 기한 만료의 결과를 기록한다. 기한 없는 대기 만료는 이후 실행될 수 있어 기록하지 않는다.
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
