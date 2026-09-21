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

use std::fmt;
use std::sync::{Arc, mpsc};
use std::time::Duration;

use serde_json::Value;

use crate::admission::{CommandAdmission, Origin, Refusal};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::server::{IpcCommand, IpcWaker};

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
    /// 응답 대기 상한이 지났다 — **결과 불명**(명령은 큐나 handler 에서 계속 갈 수 있다).
    Timeout(Duration),
    /// 응답 통로가 답 없이 버려졌다.
    Disconnected,
}

impl InjectError {
    /// 이 실패에서 요청이 **확실히 실행되지 않았는가**. 참이면 그대로 다시 걸어도 두 번째
    /// 효과가 남지 않는다.
    pub fn nothing_ran(&self) -> bool {
        matches!(self, InjectError::Refused(_) | InjectError::Send(_))
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

    /// JSON-RPC 메서드를 동기 호출. 응답은 `Ok(result)` 또는 [`InjectError`].
    /// timeout 은 응답 대기 시간 (App tick + plugin 처리 시간 모두 포함).
    pub fn dispatch(
        &self,
        method: &str,
        params: Value,
        timeout: Duration,
    ) -> Result<Value, InjectError> {
        let (resp_tx, resp_rx) = mpsc::sync_channel::<JsonRpcResponse>(1);
        let req = JsonRpcRequest {
            response_timeout_ms: None,
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
        self.sender
            .send(cmd)
            .map_err(|e| InjectError::Send(e.to_string()))?;
        (self.waker)();
        match resp_rx.recv_timeout(timeout) {
            Ok(resp) => {
                if let Some(err) = resp.error {
                    Err(InjectError::Rpc {
                        code: err.code,
                        message: err.message,
                    })
                } else {
                    Ok(resp.result.unwrap_or(Value::Null))
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Err(InjectError::Timeout(timeout)),
            Err(mpsc::RecvTimeoutError::Disconnected) => Err(InjectError::Disconnected),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::thread;

    use super::*;

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

        let h = thread::spawn(move || {
            let cmd = rx.recv().expect("recv cmd");
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
    }

    #[test]
    fn dispatch_times_out_when_no_response() {
        let (tx, _rx) = mpsc::channel::<IpcCommand>();
        // rx 는 drop 되지 않게 보유 — Disconnected 가 아니라 Timeout 을 받아야 함.
        let injector = HostIpcInjector::new(tx, noop_waker());
        let err = injector
            .dispatch("noop", Value::Null, Duration::from_millis(50))
            .expect_err("should timeout");
        assert!(err.to_string().contains("timeout"));
        assert!(
            !err.nothing_ran(),
            "a timeout is an unknown outcome, not a refusal"
        );
    }

    fn ledger(depth: usize) -> Arc<CommandAdmission> {
        CommandAdmission::new(crate::admission::QueueLimits {
            queued_bytes: crate::admission::QUEUED_BYTES_LIMIT,
            injected_depth: depth,
        })
    }

    // 주입 깊이 상한은 **시간 초과로 돌아간 호출이 큐에 남긴 명령**을 센다. 메인 루프가
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
            assert_eq!(err, InjectError::Timeout(Duration::from_millis(10)));
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
        assert!(matches!(first, Err(InjectError::Timeout(_))), "{first:?}");
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
        assert!(matches!(again, Err(InjectError::Timeout(_))), "{again:?}");
    }

    #[test]
    fn the_legacy_error_strings_are_kept() {
        assert_eq!(
            InjectError::Timeout(Duration::from_millis(5)).to_string(),
            "host_dispatch timeout after 5ms"
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
}
