//! IPC wire 타입 + 응답 헬퍼 — `IpcCommand` / `IpcWaker` / `send_response`.
//!
//! 서버 인스턴스 본문은 `crate::adapters::production::tcp_ipc_server::TcpIpcServer`
//! (D.3.D.2.b) 로 이전. 본 모듈은 wire 형식과 강결합된 타입 정의만 보유 —
//! verify 자율 결정으로 ports/ 가 아닌 wire 모듈 옆에 둔다.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::{Arc, mpsc};
use std::time::{Duration, Instant};

use crate::admission::{AdmissionTicket, CommandAdmission, Origin, Refusal};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};

/// A command received from an IPC client, with a channel to send the response back.
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
    /// 이 명령이 큐에 들어간 순간(monotonic).
    ///
    /// 큐에서 기다린 시간과 handler 안에서 보낸 시간은 **다른 값**인데, 이 표시가 없으면
    /// 둘을 가를 수 없다 — 응답이 느린 것만 보이고 그것이 적체인지 handler 비용인지
    /// 고를 수 없다.
    ///
    /// `Clock` port 가 아니라 `Instant::now()` 인 이유: 명령을 만드는 두 자리(소켓 accept
    /// 스레드와 plugin host-call 주입부)는 둘 다 `Core` 를 들고 있지 않다. 여기서 재는
    /// 것은 도메인 시각이 아니라 큐 체류 시간이라 monotonic 원천이면 충분하다.
    ///
    /// 비공개인 것이 [`IpcCommand::new`] 강제의 **전부**다 — 이 필드가 `pub` 이면 다른
    /// 크레이트가 구조체 리터럴로 임의 시각을 찍어도 컴파일되고, 그 경로는 생성자 호출자
    /// 수를 세는 어떤 판정에도 안 잡힌다. 크레이트 밖에서 이 값을 직접 읽는 자리는 없고
    /// 필요한 것은 [`IpcCommand::queue_wait`] 뿐이라 비공개로 두는 데 드는 비용이 없다.
    enqueued_at: Instant,
    /// 이 요청의 무게 — 요청 JSON 한 줄의 바이트 수(정의는 `crate::admission`).
    wire_bytes: usize,
    /// 큐 입장 장부에서 받은 몫. 큐에서 꺼낼 때([`IpcCommand::mark_dequeued`]) 버려져
    /// 반납된다. 명령이 큐째 버려지는 경로에서도 표가 함께 버려지므로 따로 반납할 일이 없다.
    ///
    /// 이 타입 자체에 `Drop` 을 달지 않는 이유: 소비자가 필드를 꺼내 옮기는(`request` 를
    /// move) 자리가 있고, `Drop` 이 붙은 구조체는 필드를 옮길 수 없다.
    admission: Option<AdmissionTicket>,
    /// 호출자가 실은 응답 대기 상한([`JsonRpcRequest::response_timeout_ms`]). 없거나 0 이면
    /// 상한이 없다 — 봉투 규약 그대로다. 큐 진입 시각과 합쳐 이 명령의 **기한**이 된다.
    wait_bound: Option<Duration>,
    /// 실행 전/후 상태. 기다리는 쪽이 [`IpcCommand::lifecycle`] 로 사본을 든다.
    lifecycle: Arc<CommandLifecycle>,
}

impl IpcCommand {
    /// 지금을 큐 진입 시각으로 찍어 명령을 만든다.
    ///
    /// 구조체 리터럴 대신 이 생성자를 쓰는 이유는 새 주입 경로가 시각을 **빠뜨릴 수
    /// 없게** 하려는 것이다 — 빠뜨리면 그 경로의 대기 시간만 조용히 0 이 된다.
    /// 그 강제는 이 doc 이 아니라 `enqueued_at` 의 비공개성이 한다: 크레이트 밖에서는
    /// 리터럴로 이 타입을 만들 수 없으므로 주입 경로는 여기를 지날 수밖에 없다.
    ///
    /// 무게는 요청을 compact 직렬화한 길이로 잰다 — 줄 없이 만들어지는 명령(호스트 주입)의
    /// 정의다. 받은 줄이 있으면 [`IpcCommand::with_wire_bytes`] 로 그 길이를 넘긴다.
    pub fn new(request: JsonRpcRequest, response_tx: mpsc::SyncSender<JsonRpcResponse>) -> Self {
        let wire_bytes = match serde_json::to_vec(&request) {
            Ok(v) => v.len(),
            Err(e) => {
                // JsonRpcRequest 는 문자열 키 map 과 Value 뿐이라 직렬화가 실패할 길이 없다.
                // 실패한다면 무게 0 으로 들어가 바이트 판정만 이 한 건을 못 본다.
                tracing::warn!("IpcCommand weight: request did not serialize: {e}");
                0
            }
        };
        Self::with_wire_bytes(request, response_tx, wire_bytes)
    }

    /// 무게를 호출자가 잰 값으로 받아 명령을 만든다 — 소켓 경로가 받은 줄의 길이를 넘긴다.
    pub fn with_wire_bytes(
        request: JsonRpcRequest,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        wire_bytes: usize,
    ) -> Self {
        let wait_bound = request
            .response_timeout_ms
            .filter(|ms| *ms > 0)
            .map(Duration::from_millis);
        Self {
            request,
            response_tx,
            enqueued_at: Instant::now(),
            wire_bytes,
            admission: None,
            wait_bound,
            lifecycle: Arc::new(CommandLifecycle::default()),
        }
    }

    /// 이 요청의 무게(바이트).
    pub fn wire_bytes(&self) -> usize {
        self.wire_bytes
    }

    /// 입장 장부에서 이 명령의 몫을 받는다. 거절이면 명령은 큐에 넣지 말아야 한다.
    pub fn admit(&mut self, ledger: &Arc<CommandAdmission>, origin: Origin) -> Result<(), Refusal> {
        self.admission = Some(ledger.admit(self.wire_bytes, origin)?);
        Ok(())
    }

    /// 큐에서 꺼냈다 — 몫을 반납한다. 여러 번 불러도 한 번만 반납된다.
    pub fn mark_dequeued(&mut self) {
        self.admission = None;
    }

    /// 큐에 들어간 뒤 지금까지 기다린 시간.
    pub fn queue_wait(&self) -> Duration {
        self.enqueued_at.elapsed()
    }

    /// 이 명령의 실행 상태 사본 — 응답을 기다리는 쪽이 **보내기 전에** 든다. 기다림이 상한에서
    /// 끝나면 [`LifecycleHandle::withdraw`] 로 "아직 시작 전이었나" 를 묻는다.
    pub fn lifecycle(&self) -> LifecycleHandle {
        LifecycleHandle(self.lifecycle.clone())
    }

    /// 실행 **직전**에 부른다 — 이 명령을 지금 실행해도 되는가.
    ///
    /// 기한(큐 진입 + 호출자의 응답 대기 상한)이 지났으면 실행하지 않는다. 그 요청을 기다리던
    /// 호출자는 이미 돌아갔거나 곧 돌아가므로, 지금 실행하면 결과를 아무도 못 받는 효과만
    /// 남는다. 기다리던 쪽이 먼저 물러났어도(`withdraw`) 실행하지 않는다.
    ///
    /// `Run` 을 돌려준 뒤로 이 명령은 **시작된 것**이다 — 기다리는 쪽의 상한이 그 뒤에 지나면
    /// 그 답은 "결과 불명" 이다.
    pub fn claim(&self) -> Claim {
        if let Some(bound) = self.wait_bound {
            let waited = self.queue_wait();
            if waited >= bound {
                return match self.lifecycle.transition(QUEUED, WITHDRAWN) {
                    Ok(()) => Claim::Expired { waited, bound },
                    Err(_) => Claim::Withdrawn,
                };
            }
        }
        match self.lifecycle.transition(QUEUED, STARTED) {
            Ok(()) => Claim::Run,
            Err(_) => Claim::Withdrawn,
        }
    }
}

/// [`IpcCommand::claim`] 의 답.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Claim {
    /// 실행한다. 이 순간부터 명령은 시작된 것이다.
    Run,
    /// 큐에서 기다리는 동안 기한이 지났다 — 실행하지 않는다. 기다리는 쪽이 아직 있을 수 있으므로
    /// 꺼낸 쪽이 [`expired_before_run_response`] 로 답한다.
    Expired {
        /// 큐에서 기다린 시간.
        waited: Duration,
        /// 호출자가 실은 응답 대기 상한.
        bound: Duration,
    },
    /// 기다리던 쪽이 상한에서 먼저 물러났다 — 실행하지 않는다. 답은 그쪽이 이미 썼다.
    Withdrawn,
}

const QUEUED: u8 = 0;
const STARTED: u8 = 1;
const WITHDRAWN: u8 = 2;

/// 명령 하나의 실행 전/후 상태. 꺼내는 쪽(메인 스레드)과 기다리는 쪽(연결 스레드)이 나눠 든다.
///
/// 상태는 한 방향으로만 한 번 움직인다 — `QUEUED` 에서 `STARTED`(꺼낸 쪽이 실행을 시작) 또는
/// `WITHDRAWN`(기한이 지나 실행하지 않기로 함) 중 **먼저 온 쪽**으로. 비교-교환 하나로 정하므로
/// 두 스레드가 같은 순간에 다퉈도 답이 하나다: 실행했으면 기다리는 쪽은 "결과 불명" 을, 안
/// 했으면 "실행하지 않음" 을 말한다. 둘 다 말하는 경우는 없다.
#[derive(Debug, Default)]
pub struct CommandLifecycle {
    state: AtomicU8,
}

impl CommandLifecycle {
    fn transition(&self, from: u8, to: u8) -> Result<(), u8> {
        self.state
            .compare_exchange(from, to, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
    }
}

/// 기다리는 쪽이 드는 [`CommandLifecycle`] 사본.
#[derive(Debug, Clone)]
pub struct LifecycleHandle(Arc<CommandLifecycle>);

/// [`LifecycleHandle::withdraw`] 의 답.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Withdraw {
    /// 명령은 실행되지 않았고 앞으로도 안 된다. 그대로 다시 보내도 두 번째 효과가 없다.
    NotRun,
    /// 명령은 이미 시작됐다 — 끝났는지, 무엇을 남겼는지는 모른다.
    Started,
}

impl LifecycleHandle {
    /// 기다림을 상한에서 끝낸다. 명령이 아직 큐에 있었으면 실행되지 않게 막는다.
    pub fn withdraw(&self) -> Withdraw {
        match self.0.transition(QUEUED, WITHDRAWN) {
            Ok(()) => Withdraw::NotRun,
            Err(WITHDRAWN) => Withdraw::NotRun,
            Err(_) => Withdraw::Started,
        }
    }
}

/// 기한이 큐에서 지나 실행하지 않은 요청의 답. 꺼낸 쪽과 기다리는 쪽 어느 쪽이 쓰든 같은 문장이
/// 나가도록 한 자리에 둔다.
pub fn expired_before_run_response(id: serde_json::Value, bound: Duration) -> JsonRpcResponse {
    JsonRpcResponse::error(
        id,
        crate::protocol::ERR_EXPIRED_BEFORE_RUN,
        format!(
            "the caller's response timeout of {} ms passed while the request was still queued; \
             it did not run — sending it again is safe",
            bound.as_millis()
        ),
    )
}

/// IPC 응답 송신용 헬퍼. 클라이언트가 응답 전에 연결을 끊었거나 receiver가 drop된
/// 경우(`SendError`)에는 trace로만 흔적을 남긴다 — 정상적인 take-and-go 케이스라
/// warn 레벨로 올릴 만한 사건은 아니다.
pub fn send_response(tx: &mpsc::SyncSender<JsonRpcResponse>, response: JsonRpcResponse) {
    if let Err(e) = tx.send(response) {
        tracing::trace!("IPC response dropped (client disconnected): {e}");
    }
}

/// Callback to wake the main event loop when an IPC command arrives.
pub type IpcWaker = Arc<dyn Fn() + Send + Sync>;

#[cfg(test)]
mod tests {
    use super::*;

    fn cmd(bound_ms: Option<u64>) -> (IpcCommand, mpsc::Receiver<JsonRpcResponse>) {
        let (tx, rx) = mpsc::sync_channel(1);
        let req = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "workspace.create".to_string(),
            params: serde_json::Value::Null,
            id: Some(serde_json::json!(1)),
            session_token: None,
            response_timeout_ms: bound_ms,
            idempotency_key: None,
        };
        (IpcCommand::new(req, tx), rx)
    }

    /// 꺼낸 쪽이 먼저 시작하면 기다리는 쪽의 만료는 "시작됨"(결과 불명)이다.
    #[test]
    fn a_started_command_cannot_be_withdrawn() {
        let (c, _rx) = cmd(Some(60_000));
        let waiter = c.lifecycle();
        assert_eq!(c.claim(), Claim::Run);
        assert_eq!(waiter.withdraw(), Withdraw::Started);
    }

    /// 기다리는 쪽이 먼저 물러나면 그 명령은 나중에 꺼내도 실행되지 않는다.
    #[test]
    fn a_withdrawn_command_is_not_run_when_taken_later() {
        let (c, _rx) = cmd(Some(60_000));
        let waiter = c.lifecycle();
        assert_eq!(waiter.withdraw(), Withdraw::NotRun);
        assert_eq!(c.claim(), Claim::Withdrawn);
    }

    /// 기한이 큐에서 지난 명령은 꺼낸 쪽이 실행하지 않고, 그 뒤 기다리는 쪽의 만료도 "실행 안 됨" 이다.
    #[test]
    fn a_command_past_its_deadline_is_expired_by_the_taker() {
        let (c, _rx) = cmd(Some(1));
        let waiter = c.lifecycle();
        std::thread::sleep(Duration::from_millis(5));
        match c.claim() {
            Claim::Expired { waited, bound } => {
                assert_eq!(bound, Duration::from_millis(1));
                assert!(waited >= bound, "{waited:?}");
            }
            other => panic!("expected Expired, got {other:?}"),
        }
        assert_eq!(waiter.withdraw(), Withdraw::NotRun);
    }

    /// 상한이 없거나 0 이면 기한도 없다 — 봉투 규약 그대로다.
    #[test]
    fn no_bound_or_zero_means_no_deadline() {
        for bound in [None, Some(0)] {
            let (c, _rx) = cmd(bound);
            std::thread::sleep(Duration::from_millis(2));
            assert_eq!(c.claim(), Claim::Run, "{bound:?}");
        }
    }

    /// 두 쪽이 같은 순간에 다퉈도 답은 하나다 — 시작과 물러남이 둘 다 이기는 경우가 없다.
    #[test]
    fn a_race_between_taker_and_waiter_has_one_winner() {
        for _ in 0..200 {
            let (c, _rx) = cmd(Some(60_000));
            let waiter = c.lifecycle();
            let t = std::thread::spawn(move || waiter.withdraw());
            let claim = c.claim();
            let withdraw = t.join().expect("withdraw thread");
            match (claim, withdraw) {
                (Claim::Run, Withdraw::Started) | (Claim::Withdrawn, Withdraw::NotRun) => {}
                other => panic!("both or neither won: {other:?}"),
            }
        }
    }

    /// 꺼낸 쪽과 기다리는 쪽이 쓰는 "실행 안 됨" 답은 한 함수에서 나온다.
    #[test]
    fn the_not_run_answer_carries_its_code_the_id_and_the_bound() {
        let r = expired_before_run_response(serde_json::json!(4), Duration::from_millis(250));
        assert_eq!(r.id, serde_json::json!(4));
        let e = r.error.expect("error");
        assert_eq!(e.code, crate::protocol::ERR_EXPIRED_BEFORE_RUN);
        assert!(
            e.message.contains("250 ms") && e.message.contains("did not run"),
            "{}",
            e.message
        );
    }
}
