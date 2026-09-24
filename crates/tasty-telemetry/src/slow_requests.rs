//! 느린 요청의 큐 대기·호스트 처리·plugin 응답 대기를 한 레코드로 연결한다.
//! 호스트 요청 번호와 각 plugin 요청 ID를 남겨 관련 로그를 찾을 수 있다.
//! 총 관측 시간이 SLOW_REQUEST_THRESHOLD 이상인 요청만 고정 용량 메모리 버퍼에 보관한다.
//!
//! params·세션 토큰·멱등 키·호출자의 JSON-RPC id는 저장하지 않는다.
//! 메서드 이름은 알 수 없는 이름도 포함하므로 MAX_METHOD_BYTES로 길이를 제한한다.
//! 호스트 요청 번호는 로그 연결에만 쓰며 메트릭 레이블로 사용하지 않는다.
//!
//! 호스트 처리는 finish_host에서, plugin 응답·만료·취소는 finish_plugin_hop에서 기록한다.
//! note_forwarded가 미완료 요청을 먼저 등록한다. 합계가 기준에 도달하면 느린 요청 버퍼로
//! 옮기고 후속 hop도 같은 레코드에 추가한다.
//! 상세 범위: docs/architecture/ipc-server.md#느린-요청-추적.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::time::Duration;

/// 큐 대기·호스트 처리·plugin 대기의 합이 이 값 이상이면 보관한다.
/// 히스토그램의 100ms 경계에 맞춘 진단 기준이다. 정상 요청도 부하에 따라 포함될 수 있다.
/// 선택 근거: docs/architecture/ipc-server.md#느린-요청-추적.
pub const SLOW_REQUEST_THRESHOLD: Duration = Duration::from_millis(100);

/// 보관할 느린 요청 수. 가득 차면 가장 오래전에 들어온 요청을 내보낸다.
/// admitted 누계와 현재 길이의 차이로 내보낸 수를 계산한다.
pub const SLOW_REQUEST_CAPACITY: usize = 32;

/// 미완료 forward 요청의 보관 상한. 연결 상한을 참고한 값이며 모든 미완료 요청을 보장하지는 않는다.
/// 넘치면 가장 오래된 항목을 버린다. 이후 해당 hop이 도착하면 호스트 기록 없이 판정한다.
pub const OPEN_FORWARD_CAPACITY: usize = 256;

/// 한 줄이 드는 plugin hop 의 상한 — pre-hook · target · post-hook 셋.
pub const MAX_PLUGIN_HOPS: usize = 3;

/// 메서드 이름의 최대 바이트 수. 호출자가 보낸 긴 이름도 들어올 수 있으므로
/// 이 길이 안의 마지막 문자 경계에서 자른다. 선택 근거는 느린 요청 추적 가이드 참조.
pub const MAX_METHOD_BYTES: usize = 128;

/// 요청의 세션 토큰 유무로 구분한 호출자 종류. 토큰 검증 성공을 뜻하지는 않는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CallerKind {
    Local,
    Agent,
}

impl CallerKind {
    pub fn as_str(self) -> &'static str {
        match self {
            CallerKind::Local => "local",
            CallerKind::Agent => "agent",
        }
    }
}

/// plugin hop 하나가 끝난 방식.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HopOutcome {
    /// plugin 이 성공으로 답했다.
    Ok,
    /// plugin 이 오류로 답했다.
    Error,
    /// deadline 까지 답이 없어 호스트가 거뒀다.
    Expired,
    /// plugin 이 치워져(종료·재시작·비활성) 호스트가 거뒀다.
    Cancelled,
}

impl HopOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            HopOutcome::Ok => "ok",
            HopOutcome::Error => "error",
            HopOutcome::Expired => "expired",
            HopOutcome::Cancelled => "cancelled",
        }
    }
}

/// 호출자에게 반환한 성공·오류 결과. 오류는 JSON-RPC 코드도 함께 보관한다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostOutcome {
    /// 성공으로 답했다.
    Ok,
    /// 오류로 답했다.
    Error {
        /// 호출자가 받은 JSON-RPC 오류 코드.
        code: i32,
    },
}

impl HostOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            HostOutcome::Ok => "ok",
            HostOutcome::Error { .. } => "error",
        }
    }

    /// 오류면 그 코드, 성공이면 `None`.
    pub fn error_code(self) -> Option<i32> {
        match self {
            HostOutcome::Ok => None,
            HostOutcome::Error { code } => Some(code),
        }
    }
}

/// 응답을 기다린 쪽이 결과를 한 번 기록하는 칸.
/// plugin 응답은 호스트 처리가 끝난 뒤 올 수 있으므로 레코드는 이 칸을 공유한다.
/// 비어 있으면 아직 응답을 받지 못했거나 응답 없이 대기를 마친 상태다.
#[derive(Debug, Clone, Default)]
pub struct HostOutcomeCell(Arc<OnceLock<HostOutcome>>);

impl HostOutcomeCell {
    /// 처음 한 번만 채워진다 — 요청 하나에 답은 하나다.
    pub fn set(&self, outcome: HostOutcome) {
        // 이미 기록한 결과는 덮어쓰지 않는다.
        let _already = self.0.set(outcome);
    }

    pub fn get(&self) -> Option<HostOutcome> {
        self.0.get().copied()
    }
}

impl PartialEq for HostOutcomeCell {
    fn eq(&self, other: &Self) -> bool {
        self.get() == other.get()
    }
}

impl Eq for HostOutcomeCell {}

/// 호스트가 한 요청을 처리하며 기록한 값.
#[derive(Debug, Clone)]
pub struct HostLeg<'a> {
    pub request_seq: u64,
    /// alias를 해석한 메서드 이름. 모르는 이름은 그대로이며 보관할 때 길이를 제한한다.
    pub method: &'a str,
    pub caller: CallerKind,
    /// 큐에 들어간 뒤 꺼내질 때까지.
    pub queue_wait: Duration,
    /// 큐에서 꺼낸 뒤 진입 검사와 호스트 처리에 쓴 시간. plugin 응답 대기는 hop에 기록한다.
    pub host: Duration,
    /// 응답을 기다린 쪽이 기록하는 최종 결과.
    pub outcome: HostOutcomeCell,
}

/// 느린 요청에 보관하는 호스트 처리 기록.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPart {
    pub method: String,
    pub caller: CallerKind,
    pub queue_wait_us: u64,
    pub host_us: u64,
    /// 호출자가 받은 답. 읽는 순간에 비어 있을 수 있다([`HostOutcomeCell`]).
    pub outcome: HostOutcomeCell,
}

/// plugin hop 하나.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginHop {
    /// 이 hop 을 받은 plugin.
    pub plugin_id: String,
    /// 호스트가 이 hop 에 붙인 req_id — plugin 이 받은 JSON-RPC id 와 같다.
    pub host_request_id: u64,
    pub wait_us: u64,
    pub outcome: HopOutcome,
}

/// 느린 요청 하나의 기록.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowRequest {
    pub request_seq: u64,
    /// 호스트 처리 기록. 미완료 목록에서 밀려난 뒤 hop만 도착하면 None이다.
    pub host: Option<HostPart>,
    pub plugin_hops: Vec<PluginHop>,
}

impl SlowRequest {
    fn empty(request_seq: u64) -> Self {
        Self {
            request_seq,
            host: None,
            plugin_hops: Vec::new(),
        }
    }

    /// 기록된 큐 대기·호스트 처리·plugin hop 대기 시간의 합(마이크로초).
    pub fn total_us(&self) -> u64 {
        let host = self
            .host
            .as_ref()
            .map_or(0, |h| h.queue_wait_us.saturating_add(h.host_us));
        self.plugin_hops
            .iter()
            .fold(host, |acc, h| acc.saturating_add(h.wait_us))
    }

    fn is_slow(&self) -> bool {
        self.total_us() >= as_micros(SLOW_REQUEST_THRESHOLD)
    }

    fn push_hop(&mut self, hop: PluginHop) {
        if self.plugin_hops.len() < MAX_PLUGIN_HOPS {
            self.plugin_hops.push(hop);
        }
    }
}

/// [`SlowRequestLog`] 의 한 시점 읽기.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowRequestsSnapshot {
    /// 오래된 것부터.
    pub rows: Vec<SlowRequest>,
    /// 프로세스 수명 동안 링에 든 줄 수. 줄 수와의 차가 밀려난 수다.
    pub admitted: u64,
}

#[derive(Debug, Default)]
struct Inner {
    rows: VecDeque<SlowRequest>,
    open: VecDeque<SlowRequest>,
    admitted: u64,
}

impl Inner {
    fn admit(&mut self, row: SlowRequest) {
        self.rows.push_back(row);
        while self.rows.len() > SLOW_REQUEST_CAPACITY {
            self.rows.pop_front();
        }
        self.admitted = self.admitted.saturating_add(1);
    }

    fn row_mut(&mut self, seq: u64) -> Option<&mut SlowRequest> {
        self.rows.iter_mut().find(|r| r.request_seq == seq)
    }

    fn open_index(&self, seq: u64) -> Option<usize> {
        self.open.iter().position(|r| r.request_seq == seq)
    }

    /// 기준 이상이면 느린 요청 버퍼로 옮기고, 마지막 hop이면 미완료 목록에서 제거한다.
    fn settle_open(&mut self, at: usize, last: bool) {
        if self.open[at].is_slow() {
            if let Some(row) = self.open.remove(at) {
                self.admit(row);
            }
        } else if last {
            self.open.remove(at);
        }
    }
}

/// 호스트 dispatch와 plugin 매니저가 공유하는 느린 요청 버퍼 및 미완료 forward 목록.
#[derive(Debug, Default)]
pub struct SlowRequestLog {
    inner: Mutex<Inner>,
}

impl SlowRequestLog {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // 진단 기록은 poison을 복구해 유지한다.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// plugin으로 넘긴 요청을 등록한다. 같은 요청의 후속 hop이면 기존 항목을 유지한다.
    pub fn note_forwarded(&self, seq: u64) {
        let mut inner = self.lock();
        if inner.row_mut(seq).is_some() || inner.open_index(seq).is_some() {
            return;
        }
        inner.open.push_back(SlowRequest::empty(seq));
        while inner.open.len() > OPEN_FORWARD_CAPACITY {
            inner.open.pop_front();
        }
    }

    /// 호스트 처리 결과를 기록한다. forward는 미완료 목록에 연결하고 다른 요청은 기준 이상만 보관한다.
    pub fn finish_host(&self, leg: HostLeg<'_>) {
        let outcome = &leg.outcome;
        let (queue_wait_us, host_us) = (as_micros(leg.queue_wait), as_micros(leg.host));
        // 보관할 때만 메서드 이름을 복사한다.
        let part = || HostPart {
            method: clip_method(leg.method).to_string(),
            caller: leg.caller,
            queue_wait_us,
            host_us,
            outcome: outcome.clone(),
        };
        let mut inner = self.lock();
        if let Some(at) = inner.open_index(leg.request_seq) {
            inner.open[at].host = Some(part());
            inner.settle_open(at, false);
        } else if let Some(row) = inner.row_mut(leg.request_seq) {
            row.host = Some(part());
        } else if queue_wait_us.saturating_add(host_us) >= as_micros(SLOW_REQUEST_THRESHOLD) {
            inner.admit(SlowRequest {
                request_seq: leg.request_seq,
                host: Some(part()),
                plugin_hops: Vec::new(),
            });
        }
    }

    /// plugin hop의 종료를 기록한다. last가 true면 더 이어질 hop이 없다는 뜻이다.
    pub fn finish_plugin_hop(&self, seq: u64, hop: PluginHop, last: bool) {
        let mut inner = self.lock();
        if let Some(row) = inner.row_mut(seq) {
            row.push_hop(hop);
            return;
        }
        if let Some(at) = inner.open_index(seq) {
            inner.open[at].push_hop(hop);
            inner.settle_open(at, last);
            return;
        }
        let mut row = SlowRequest::empty(seq);
        row.push_hop(hop);
        if row.is_slow() {
            inner.admit(row);
        } else if !last {
            inner.open.push_back(row);
            while inner.open.len() > OPEN_FORWARD_CAPACITY {
                inner.open.pop_front();
            }
        }
    }

    pub fn snapshot(&self) -> SlowRequestsSnapshot {
        let inner = self.lock();
        SlowRequestsSnapshot {
            rows: inner.rows.iter().cloned().collect(),
            admitted: inner.admitted,
        }
    }
}

/// `method` 를 [`MAX_METHOD_BYTES`] 안의 마지막 char 경계까지로 줄인다.
fn clip_method(method: &str) -> &str {
    if method.len() <= MAX_METHOD_BYTES {
        return method;
    }
    let mut end = MAX_METHOD_BYTES;
    while !method.is_char_boundary(end) {
        end -= 1;
    }
    &method[..end]
}

/// PressureStats와 같은 마이크로초 단위로 환산한다.
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "slow_requests_tests.rs"]
mod tests;
