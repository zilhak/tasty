//! 느린 요청 링 — **이 느린 요청**의 시간이 어느 단계에 있었고, 그 plugin 대기가 **어느
//! 요청**의 것이었는가.
//!
//! [`crate::PressureStats`] · [`crate::PluginWaitStats`] 는 단계마다 **분포**를 준다. 분포는
//! "꼬리가 있다" 까지는 말하지만 "그 꼬리의 한 건이 큐에 앉아 있었나, handler 안에 있었나,
//! plugin 을 기다렸나" 는 못 말한다 — 세 분포가 서로 다른 모수로 따로 접히기 때문이다. 이
//! 링은 느린 요청 **한 건**을 한 줄로 남긴다: 호스트가 발급한 요청 번호 · 메서드 · 큐 대기 ·
//! 호스트 처리 시간, 그리고 plugin 으로 넘겼으면 hop 마다의 plugin · 호스트 req_id · 대기 ·
//! 결과. 호스트 req_id 는 plugin 이 받은 JSON-RPC id 와 같은 값이라 plugin 로그를 원 요청으로
//! 되짚는 열쇠가 된다(근거 ADR-0436).
//!
//! **전 요청을 넣지 않는다.** 넣는 것은 [`SLOW_REQUEST_THRESHOLD`] 를 넘은 요청뿐이다. 전부
//! 넣으면 정상 요청이 링을 곧바로 밀어내 원인 요청이 안 남는다.
//!
//! **영구 기록이 아니다.** 메모리 안의 고정 용량이고 호출당 저장소 행이 0 이다 — 진단이
//! 호출당 기록 폭주를 다시 만들지 않는다(ADR-0085 · ADR-0246 · ADR-0277 과 같은 축).
//!
//! **싣지 않는 것**: params 원문 · session token · 멱등 키 · JSON-RPC `id` 값. 메서드는
//! canonical 이름이지만 **유한 집합이 아니다** — alias 해석은 모르는 이름을 받은 그대로
//! 통과시키므로(plugin namespace 메서드 · 오타 · 없는 이름) 호출자 문자열이 그대로 실린다. 그래서
//! 싣는 길이를 [`MAX_METHOD_BYTES`] 로 자른다. plugin id 는 설치 수만큼만 있다. 요청 번호는
//! 이 줄의 열쇠일 뿐 메트릭 레이블로 쓰지 않는다.
//!
//! ## 한 줄이 두 번에 나눠 채워진다
//!
//! 호스트 쪽 몫은 명령을 끝까지 다룬 뒤([`SlowRequestLog::finish_host`]), plugin 쪽 몫은 그
//! 뒤 프레임에서 응답·만료·취소가 올 때([`SlowRequestLog::finish_plugin_hop`]) 채워진다. 둘을
//! 잇기 위해 plugin 으로 넘긴 요청은 **열린 표**에 먼저 자리를 잡는다
//! ([`SlowRequestLog::note_forwarded`]). 열린 표는 끝나지 않은 forward 만 들고, 그 합이
//! 문턱을 넘는 순간 링으로 옮겨진다 — 그 뒤에 오는 hop 은 링 안의 줄에 붙는다.

use std::collections::VecDeque;
use std::sync::{Mutex, MutexGuard, PoisonError};
use std::time::Duration;

/// 링에 넣는 문턱 — 큐 대기 · 호스트 처리 · plugin 대기의 합이 이 값 **이상**이면 넣는다.
///
/// **파생값이 아니다** — 고른 값이다(근거 ADR-0436). 두 가지로 골랐다. ① 실측: ADR-0333 이
/// 격리 인스턴스에서 잰 정상 부하의 최댓값이 큐 대기 25.4 ms · handler 0.48 ms 였다 — 문턱이
/// 그보다 네 배 위라 정상 요청은 링에 안 든다. ② 대조: 분포의 버킷 경계
/// ([`crate::pressure::LATENCY_BUCKET_BOUNDS_US`])의 한 칸(100 ms)과 같은 값이라, 운영자가 분포에서
/// "100 ms 를 넘은 것이 몇 건" 을 읽은 자리에서 그 건들의 줄을 여기서 찾을 수 있다.
pub const SLOW_REQUEST_THRESHOLD: Duration = Duration::from_millis(100);

/// 링의 줄 수. 넘치면 가장 오래 전에 들어온 줄이 밀려난다.
///
/// **파생값이 아니다**(ADR-0436). 한 번의 조회로 "최근의 느린 요청들" 을 훑기에 충분하고,
/// 한 줄이 수백 바이트라 상한에서 수십 KB 다. 밀려난 수는 `admitted` 누계와 줄 수의 차로
/// 읽힌다.
pub const SLOW_REQUEST_CAPACITY: usize = 32;

/// 열린 표(아직 끝나지 않은 forward)의 상한.
///
/// 동시 IPC 연결 상한(ADR-0313 의 256)과 같은 값이다 — 소켓 연결 하나는 한 번에 요청 하나를
/// 기다리므로, 동시에 plugin 을 기다리는 IPC 요청 수가 대개 이 안에 든다. 넘치면 가장 오래
/// 열린 것이 버려진다(그때까지 문턱을 안 넘었으므로 링에 들 줄이 아니었다). 버려진 줄의 hop
/// 이 나중에 오면 호스트 몫 없이 그 hop 만으로 판정한다.
pub const OPEN_FORWARD_CAPACITY: usize = 256;

/// 한 줄이 드는 plugin hop 의 상한 — pre-hook · target · post-hook 셋.
pub const MAX_PLUGIN_HOPS: usize = 3;

/// 줄에 싣는 메서드 이름의 바이트 상한. 넘으면 이 길이 안의 마지막 char 경계에서 자른다.
///
/// 메서드 칸은 호출자가 보낸 문자열이다(모르는 이름은 alias 해석을 그대로 통과한다). 자르지
/// 않으면 한 호출자가 긴 이름으로 링 32 줄을 각각 임의 길이로 채울 수 있다. **파생값이 아니다**
/// (ADR-0436) — 실측 2026-09-21 에 등록 메서드 표(264 개)의 가장 긴 이름이 31 바이트였고, 그
/// 네 배 남짓이라 정상 이름은 잘리지 않는다.
pub const MAX_METHOD_BYTES: usize = 128;

/// 요청을 보낸 쪽의 종류. 봉투(session token 유무)가 말한 종류다 — 토큰이 무효인 요청은
/// 게이트에서 곧바로 거절돼 느린 줄이 될 일이 드물다.
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

/// 호스트가 명령 하나를 다룬 몫.
#[derive(Debug, Clone, Copy)]
pub struct HostLeg<'a> {
    pub request_seq: u64,
    /// canonical 메서드 이름 — 모르는 이름이면 받은 그대로다. 줄에는 [`MAX_METHOD_BYTES`] 까지만
    /// 실린다.
    pub method: &'a str,
    pub caller: CallerKind,
    /// 큐에 들어간 뒤 꺼내질 때까지.
    pub queue_wait: Duration,
    /// 꺼낸 뒤 호스트가 그 명령을 다 다룰 때까지(게이트 포함). plugin 으로 넘긴 요청이면 넘기는
    /// 데까지이고, plugin 을 기다린 시간은 hop 쪽에 있다.
    pub host: Duration,
}

/// 줄에 남는 호스트 몫.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostPart {
    pub method: String,
    pub caller: CallerKind,
    pub queue_wait_us: u64,
    pub host_us: u64,
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

/// 링의 한 줄.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlowRequest {
    pub request_seq: u64,
    /// 호스트 몫. 열린 표에서 밀려난 뒤 hop 만 온 줄이면 `None` 이다.
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

    /// 이 줄이 잰 시간의 합(마이크로초) — 큐 대기 · 호스트 처리 · hop 대기. 셋은 차례로
    /// 일어나므로 겹치지 않는다.
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

    /// 열린 줄이 문턱을 넘었으면 링으로 옮기고, 끝났으면(`last`) 표에서 뺀다.
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

/// 느린 요청 링과 열린 forward 표. 프로세스에 하나이고, 호스트 dispatch 루프와 plugin
/// 매니저가 같은 인스턴스를 나눠 든다(`Arc`).
#[derive(Debug, Default)]
pub struct SlowRequestLog {
    inner: Mutex<Inner>,
}

impl SlowRequestLog {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        // 안의 값은 진단용 줄 몇 개라, 다른 스레드가 쥔 채 죽었어도 그대로 쓰는 편이 옳다.
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// 요청 `seq` 를 plugin 으로 넘겼다 — 호스트 몫과 hop 을 이을 자리를 연다. 이미 자리가
    /// 있으면(사슬의 다음 hop) 아무것도 안 한다.
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

    /// 호스트가 명령 하나를 다 다뤘다. plugin 으로 넘긴 요청이면 열린 자리에 몫을 채우고,
    /// 아니면 문턱을 넘었을 때만 링에 넣는다.
    pub fn finish_host(&self, leg: HostLeg<'_>) {
        let (queue_wait_us, host_us) = (as_micros(leg.queue_wait), as_micros(leg.host));
        // 메서드 이름은 줄에 남길 때만 복사한다 — 이 자리는 요청마다 지난다.
        let part = || HostPart {
            method: clip_method(leg.method).to_string(),
            caller: leg.caller,
            queue_wait_us,
            host_us,
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

    /// 요청 `seq` 의 plugin hop 하나가 끝났다. `last` 는 이 hop 뒤로 사슬이 더 이어지지 않는다는
    /// 뜻이다(이어지면 열린 자리를 그대로 둔다).
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

/// 마이크로초로 접는다 — [`crate::PressureStats`] 와 같은 해상도.
fn as_micros(d: Duration) -> u64 {
    u64::try_from(d.as_micros()).unwrap_or(u64::MAX)
}

#[cfg(test)]
#[path = "slow_requests_tests.rs"]
mod tests;
