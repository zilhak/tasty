//! IPC 명령·실행 상태·응답 채널과 메인 루프 깨우기 인터페이스.

use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock, mpsc};
use std::time::{Duration, Instant};

use crate::admission::{AdmissionTicket, CommandAdmission, Origin, Refusal};
use crate::dispatch::{DispatchStats, FlightTicket};
use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use tasty_telemetry::slow_requests::{HostOutcome, HostOutcomeCell};

/// 호스트가 프로세스 안에서 발급하는 요청 번호. JSON-RPC id와 이벤트 trace_id와는 별개다.
/// RequestSeq::next로 만들고 요청 전달 중에는 같은 번호를 유지한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RequestSeq(u64);

/// 다음에 줄 번호. 0 은 안 준다 — 첫 번호가 1 이다.
static NEXT_REQUEST_SEQ: AtomicU64 = AtomicU64::new(1);

impl RequestSeq {
    /// 새 번호를 하나 받는다. 순서만 보장하면 되는 단조 카운터라 `Relaxed` 로 충분하다.
    pub fn next() -> Self {
        Self(NEXT_REQUEST_SEQ.fetch_add(1, Ordering::Relaxed))
    }

    /// 번호의 값. 진단 응답·로그에 싣는 자리가 쓴다 — 메트릭 레이블로는 쓰지 않는다
    /// (요청마다 다른 값이라 레이블 수가 요청 수만큼 는다).
    pub fn get(self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for RequestSeq {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

/// A command received from an IPC client, with a channel to send the response back.
pub struct IpcCommand {
    pub request: JsonRpcRequest,
    pub response_tx: mpsc::SyncSender<JsonRpcResponse>,
    /// 큐 진입 시각. 도메인 Clock 없이 만드는 소켓/주입 경로가 있어 Instant로 잰다.
    /// 비공개 필드로 두어 모든 경로가 생성자를 거치게 한다.
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
    /// 호스트가 이 요청에 붙인 번호. 생성자가 받으므로 명령을 만드는 모든 경로(소켓 · 호스트
    /// 주입)가 번호를 갖는다 — `enqueued_at` 과 같은 강제다.
    request_seq: RequestSeq,
}

impl IpcCommand {
    /// 현재 시각과 compact JSON 바이트 길이로 명령을 만든다.
    /// 받은 원문 줄이 있는 소켓 경로는 with_wire_bytes로 그 길이를 전달한다.
    pub fn new(request: JsonRpcRequest, response_tx: mpsc::SyncSender<JsonRpcResponse>) -> Self {
        let wire_bytes = weigh(&request);
        Self::with_wire_bytes(request, response_tx, wire_bytes)
    }

    /// 무게를 호출자가 잰 값으로 받아 명령을 만든다 — 소켓 경로가 받은 줄의 길이를 넘긴다.
    pub fn with_wire_bytes(
        request: JsonRpcRequest,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        wire_bytes: usize,
    ) -> Self {
        Self::build(request, response_tx, wire_bytes, RequestSeq::next())
    }

    /// 멱등 relay처럼 같은 요청을 다른 응답 채널에 넘길 때 기존 번호를 유지한다.
    pub fn continuing(
        request: JsonRpcRequest,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        request_seq: RequestSeq,
    ) -> Self {
        let wire_bytes = weigh(&request);
        Self::build(request, response_tx, wire_bytes, request_seq)
    }

    fn build(
        request: JsonRpcRequest,
        response_tx: mpsc::SyncSender<JsonRpcResponse>,
        wire_bytes: usize,
        request_seq: RequestSeq,
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
            request_seq,
        }
    }

    /// 호스트가 이 요청에 붙인 번호([`RequestSeq`]).
    pub fn request_seq(&self) -> RequestSeq {
        self.request_seq
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

    /// 이 명령의 답이 끝난 방식을 담을 칸 — 느린 요청 링의 호스트 줄이 들고 간다. 채우는 쪽은
    /// 기다리는 쪽이다([`LifecycleHandle::record_answer`]).
    pub fn outcome_cell(&self) -> HostOutcomeCell {
        self.lifecycle.outcome.clone()
    }

    /// 실행 직전에 기한과 철회 여부를 확인한다. Run 이후 만료는 결과 불명이다.
    /// 시작 전 만료는 expired_before_run, 실행 시작은 in-flight에 기록한다.
    /// in-flight는 명령과 응답 대기자가 모두 lifecycle을 놓을 때 해제된다.
    pub fn claim(&self, stats: &Arc<DispatchStats>) -> Claim {
        if let Some(bound) = self.wait_bound {
            let waited = self.queue_wait();
            if waited >= bound {
                stats.record_expired_before_run();
                return match self.lifecycle.transition(QUEUED, WITHDRAWN) {
                    Ok(()) => Claim::Expired { waited, bound },
                    Err(_) => Claim::Withdrawn,
                };
            }
        }
        match self.lifecycle.transition(QUEUED, STARTED) {
            Ok(()) => {
                if self.lifecycle.flight.set(stats.begin_flight()).is_err() {
                    // 상태가 QUEUED 에서 STARTED 로 가는 것은 한 번뿐이라 이 칸이 두 번 채워질
                    // 길이 없다. 채워졌다면 그 몫은 여기서 버려지며 스스로 빠진다.
                    tracing::warn!("IPC command started twice — in-flight share dropped");
                }
                Claim::Run
            }
            Err(_) => {
                stats.record_expired_before_run();
                Claim::Withdrawn
            }
        }
    }
}

/// 줄 없이 만들어지는 명령의 무게 — 요청을 compact 직렬화한 길이.
fn weigh(request: &JsonRpcRequest) -> usize {
    match serde_json::to_vec(request) {
        Ok(v) => v.len(),
        Err(e) => {
            // JsonRpcRequest 는 문자열 키 map 과 Value 뿐이라 직렬화가 실패할 길이 없다.
            // 실패한다면 무게 0 으로 들어가 바이트 판정만 이 한 건을 못 본다.
            tracing::warn!("IpcCommand weight: request did not serialize: {e}");
            0
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

/// QUEUED에서 STARTED 또는 WITHDRAWN으로 한 번만 전환한다.
/// 비교-교환으로 실행과 철회 중 하나만 성공하게 한다.
#[derive(Debug, Default)]
pub struct CommandLifecycle {
    state: AtomicU8,
    /// 실행을 시작한 명령의 in-flight 몫. 이 칸을 든 마지막 쪽(명령 또는 기다리는 쪽)이 놓을 때
    /// 버려진다.
    flight: OnceLock<FlightTicket>,
    /// 호출자에게 나간 답이 끝난 방식. 기다리는 쪽이 답을 받거나 스스로 만든 순간 채우고, 느린
    /// 요청 링의 호스트 줄이 같은 칸을 들어 읽는다(docs/architecture/ipc-server.md#느린-요청-추적).
    outcome: HostOutcomeCell,
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

    /// 호출자에게 실제로 나간 답을 적는다 — 받은 답이든, 상한에서 스스로 만든 답이든. 한 명령에
    /// 처음 한 번만 적힌다.
    pub fn record_answer(&self, response: &JsonRpcResponse) {
        self.0.outcome.set(match &response.error {
            Some(err) => HostOutcome::Error { code: err.code },
            None => HostOutcome::Ok,
        });
    }

    /// 응답 봉투 없이 끝난 갈래(주입 경로의 상한 만료)를 코드로 적는다.
    pub fn record_error_code(&self, code: i32) {
        self.0.outcome.set(HostOutcome::Error { code });
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

    fn stats() -> Arc<DispatchStats> {
        Arc::new(DispatchStats::default())
    }

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

    #[test]
    fn a_started_command_cannot_be_withdrawn() {
        let (c, _rx) = cmd(Some(60_000));
        let waiter = c.lifecycle();
        assert_eq!(c.claim(&stats()), Claim::Run);
        assert_eq!(waiter.withdraw(), Withdraw::Started);
    }

    #[test]
    fn a_withdrawn_command_is_not_run_when_taken_later() {
        let (c, _rx) = cmd(Some(60_000));
        let waiter = c.lifecycle();
        assert_eq!(waiter.withdraw(), Withdraw::NotRun);
        assert_eq!(c.claim(&stats()), Claim::Withdrawn);
    }

    #[test]
    fn a_command_past_its_deadline_is_expired_by_the_taker() {
        let (c, _rx) = cmd(Some(1));
        let waiter = c.lifecycle();
        std::thread::sleep(Duration::from_millis(5));
        match c.claim(&stats()) {
            Claim::Expired { waited, bound } => {
                assert_eq!(bound, Duration::from_millis(1));
                assert!(waited >= bound, "{waited:?}");
            }
            other => panic!("expected Expired, got {other:?}"),
        }
        assert_eq!(waiter.withdraw(), Withdraw::NotRun);
    }

    #[test]
    fn the_answer_the_waiter_records_shows_in_the_command_cell() {
        let (c, _rx) = cmd(Some(60_000));
        let cell = c.outcome_cell();
        let waiter = c.lifecycle();
        assert_eq!(cell.get(), None, "nothing answered yet");
        waiter.record_answer(&JsonRpcResponse::error(
            serde_json::json!(1),
            crate::protocol::ERR_EXPIRED_BEFORE_RUN,
            "expired",
        ));
        waiter.record_answer(&JsonRpcResponse::success(
            serde_json::json!(1),
            serde_json::Value::Null,
        ));
        assert_eq!(
            cell.get(),
            Some(HostOutcome::Error {
                code: crate::protocol::ERR_EXPIRED_BEFORE_RUN
            })
        );

        let (ok, _rx) = cmd(None);
        let cell = ok.outcome_cell();
        ok.lifecycle().record_answer(&JsonRpcResponse::success(
            serde_json::json!(1),
            serde_json::Value::Null,
        ));
        assert_eq!(cell.get(), Some(HostOutcome::Ok));
    }

    #[test]
    fn no_bound_or_zero_means_no_deadline() {
        for bound in [None, Some(0)] {
            let (c, _rx) = cmd(bound);
            std::thread::sleep(Duration::from_millis(2));
            assert_eq!(c.claim(&stats()), Claim::Run, "{bound:?}");
        }
    }

    #[test]
    fn a_race_between_taker_and_waiter_has_one_winner() {
        for _ in 0..200 {
            let (c, _rx) = cmd(Some(60_000));
            let waiter = c.lifecycle();
            let t = std::thread::spawn(move || waiter.withdraw());
            let claim = c.claim(&stats());
            let withdraw = t.join().expect("withdraw thread");
            match (claim, withdraw) {
                (Claim::Run, Withdraw::Started) | (Claim::Withdrawn, Withdraw::NotRun) => {}
                other => panic!("both or neither won: {other:?}"),
            }
        }
    }

    /// 실행을 시작한 명령은 명령과 기다리는 쪽이 **둘 다** 놓을 때까지 in-flight 다. 실행하지 않은
    /// 명령은 in-flight 에 안 들고 `expired_before_run` 에 든다.
    #[test]
    fn a_started_command_is_in_flight_until_both_holders_let_go() {
        let stats = stats();
        let (c, _rx) = cmd(Some(60_000));
        let waiter = c.lifecycle();
        assert_eq!(c.claim(&stats), Claim::Run);
        drop(waiter);
        assert_eq!(stats.snapshot().in_flight, 1, "the command still holds it");
        drop(c);
        let s = stats.snapshot();
        assert_eq!((s.in_flight, s.started, s.expired_before_run), (0, 1, 0));

        let (c, _rx) = cmd(Some(60_000));
        c.lifecycle().withdraw();
        assert_eq!(c.claim(&stats), Claim::Withdrawn);
        let s = stats.snapshot();
        assert_eq!((s.in_flight, s.started, s.expired_before_run), (0, 1, 1));
    }

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

    #[test]
    fn every_constructor_issues_a_fresh_increasing_request_seq() {
        let (a, _ra) = cmd(None);
        let (tx, _rb) = mpsc::sync_channel(1);
        let b = IpcCommand::with_wire_bytes(a.request.clone(), tx, 10);
        let (c, _rc) = cmd(None);
        assert!(
            a.request_seq() < b.request_seq(),
            "{a:?} {b:?}",
            a = a.request_seq(),
            b = b.request_seq()
        );
        assert!(b.request_seq() < c.request_seq());
        assert!(a.request_seq().get() >= 1, "0 is never issued");
    }

    #[test]
    fn a_continuing_command_keeps_the_original_request_seq() {
        let (a, _ra) = cmd(None);
        let (tx, _rb) = mpsc::sync_channel(1);
        let relayed = IpcCommand::continuing(a.request.clone(), tx, a.request_seq());
        assert_eq!(relayed.request_seq(), a.request_seq());
        let (next, _rc) = cmd(None);
        assert!(next.request_seq() > a.request_seq());
    }
}
