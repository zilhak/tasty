//! IPC 명령 큐의 입장 판정 — 큐에 쌓인 **바이트**와 호스트 자신이 주입한 **명령 수**.
//!
//! 명령 큐(`mpsc::channel`)는 무제한이다. 그 위에 올릴 수 있는 명령의 **개수**는 두 생산자가
//! 모두 응답을 기다리며 블록한다는 사실에서 파생된다 — 소켓 연결 하나는 한 번에 한 건만
//! 올리고, 연결 수는 서버의 연결 상한이 자른다. 그 파생이 못 덮는 것이 둘이다.
//!
//! - **바이트.** 한 건의 크기는 줄 상한이 자르지만, 쌓인 양은 아무도 안 쟀다. 연결 상한 ×
//!   줄 상한이 이론상 상한이었고 그것은 수 GiB 다.
//! - **호스트 자신의 주입.** 주입기(`host_call::HostIpcInjector`)는 응답을 **시간 상한까지만**
//!   기다린다. 메인 루프가 서 있는 동안 상한이 지나면 호출자는 돌아가는데 명령은 큐에 그대로
//!   남는다. 그 호출자가 웹훅처럼 밖에서 계속 들어오는 사건이면 큐가 끝없이 자란다 — 연결
//!   상한이 이 인구를 안 센다.
//!
//! 그래서 이 모듈이 두 판정을 한다: 큐에 든 바이트의 합([`QueueLimits::queued_bytes`]), 그리고
//! 큐에 든 주입 명령의 수([`QueueLimits::injected_depth`]). 판정은 큐 **밖**의 장부 하나가 락
//! 하나로 한다 — 생산자가 여럿이라 원자값 둘로는 "합계의 마지막 자리" 를 원자적으로 못 가른다.
//!
//! ## 1 바이트는 무엇인가 (다음 계측이 이 정의를 그대로 쓴다)
//!
//! 명령 하나의 무게는 **그 요청의 JSON 텍스트 한 줄의 바이트 수**다 — 개행과 앞뒤 공백은
//! 빼고 센다.
//!
//! - 소켓으로 온 요청은 **받은 줄**을 잰다(`trim` 한 뒤의 길이). 다시 직렬화하지 않는다 —
//!   client 가 보낸 공백까지 호스트가 실제로 쥐고 있던 양이기 때문이다.
//! - 호스트가 주입한 요청은 줄이 없으므로 `serde_json::to_vec` 의 compact 직렬화 길이를 잰다.
//!
//! 두 갈래가 "같은 요청이면 같은 수" 를 약속하지는 않는다(공백이 다를 수 있다). 약속하는 것은
//! **요청 하나당 한 번, 그 요청이 호스트에 도착한 모양 그대로 잰다**는 것이다. 파싱된
//! `JsonRpcRequest` 가 메모리에서 차지하는 실제 크기는 이 수와 다르다(힙 할당 · `Value` 트리).
//! 이 수는 그 근삿값이 아니라 **소켓에서 재는 줄 상한과 같은 단위**를 고른 것이다 — 한 건의
//! 상한과 쌓인 양의 상한을 같은 자로 재야 둘의 관계를 적을 수 있다.
//!
//! ## 반납 시점
//!
//! 명령이 **큐에서 꺼내질 때** 몫이 빠진다(서버 port 의 `try_recv`). handler 가 실행 중인
//! 명령은 이미 큐 밖이다 — 이 장부가 재는 것은 대기열이지 처리 중인 일이 아니다. 몫은
//! [`AdmissionTicket`] 이 들고, 표가 버려질 때 빠진다. 그래서 큐째 버려지는 종료 경로나 송신
//! 실패로 명령이 되돌아오는 경로도 따로 배선하지 않아도 반납된다.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 큐에 쌓여 있을 수 있는 요청 바이트의 합. **파생이 아니다** — 근거는 ADR-0391.
///
/// 관계만 고정한다(본체 `tcp_ipc_server` 가 컴파일 시점에 단정한다): 줄 상한 두 건이 동시에
/// 들어갈 만큼 크고, 연결 상한 × 줄 상한(지금까지의 이론상 상한)보다 충분히 작다.
pub const QUEUED_BYTES_LIMIT: usize = 64 * 1024 * 1024;

/// 큐에 들어 있을 수 있는 **호스트 주입** 명령의 수. **파생이 아니다** — 근거는 ADR-0391.
///
/// 관계만 고정한다: 한 dispatch 회차의 예산(`DRAIN_BUDGET_PER_ROUND`)을 넘지 않는다. 그래야
/// 주입 몫만으로는 한 회차가 못 비우는 적체가 생기지 않는다 — **명령 수로는.** 회차는 시간
/// 예산에서도 멈추므로(ADR-0410) 무거운 주입 명령은 여러 회차에 나뉘어 비워지고, 그동안 남은
/// 것은 큐에 그대로 있어 이 장부가 센다.
pub const INJECTED_DEPTH_LIMIT: usize = 256;

/// 두 상한. 시험이 작은 값을 넣을 수 있도록 상수가 아니라 값으로 든다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueLimits {
    /// 큐에 든 요청 바이트 합의 상한. 빈 큐는 이 값보다 큰 한 건도 받는다.
    pub queued_bytes: usize,
    /// 큐에 든 호스트 주입 명령 수의 상한. 소켓 명령은 이 수에 안 든다.
    pub injected_depth: usize,
}

impl QueueLimits {
    /// 제품 기본값.
    pub const DEFAULT: Self = Self {
        queued_bytes: QUEUED_BYTES_LIMIT,
        injected_depth: INJECTED_DEPTH_LIMIT,
    };
}

impl Default for QueueLimits {
    fn default() -> Self {
        Self::DEFAULT
    }
}

/// 명령이 어디서 왔는가. 주입 명령만 개수 판정을 받는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    /// 소켓 연결이 올린 요청. 개수는 연결 상한이 이미 자른다.
    Socket,
    /// 호스트가 자기 스레드에서 주입한 요청(`HostIpcInjector`).
    Injected,
}

/// 입장 거절의 사유. **어느 쪽이든 명령은 큐에 들어가지 않았고 아무것도 실행되지 않았다.**
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// 이 요청을 더하면 큐의 바이트 합이 상한을 넘는다.
    QueueBytes {
        /// 판정 순간 큐에 있던 바이트.
        queued: usize,
        /// 이 요청의 바이트.
        request: usize,
        /// 상한.
        limit: usize,
    },
    /// 큐에 든 주입 명령이 이미 상한만큼이다.
    InjectedDepth {
        /// 판정 순간 큐에 있던 주입 명령 수.
        queued: usize,
        /// 상한.
        limit: usize,
    },
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Refusal::QueueBytes {
                queued,
                request,
                limit,
            } => write!(
                f,
                "the host's command queue already holds {queued} bytes and this request adds \
                 {request} (limit {limit})"
            ),
            Refusal::InjectedDepth { queued, limit } => write!(
                f,
                "the host's command queue already holds {queued} host-injected commands \
                 (limit {limit})"
            ),
        }
    }
}

/// 한 시점의 장부 값. 읽는 자리는 [`crate::dispatch::CommandQueueSnapshot::read`] 다(ADR-0412) —
/// IPC·CLI 노출은 아직 없다(ADR-0391 재검토 조건).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AdmissionSnapshot {
    /// 지금 큐에 든 요청 바이트 합.
    pub queued_bytes: u64,
    /// 지금 큐에 든 명령 수(소켓 + 주입). 입장 판정을 거친 것만 센다.
    pub queued_commands: u64,
    /// 그중 호스트 주입 명령 수.
    pub queued_injected: u64,
    /// 지금까지 본 `queued_bytes` 의 최댓값.
    pub peak_bytes: u64,
    /// 바이트 상한으로 거절한 누계.
    pub refused_bytes: u64,
    /// 주입 깊이 상한으로 거절한 누계.
    pub refused_depth: u64,
}

#[derive(Debug, Default)]
struct Held {
    bytes: usize,
    commands: usize,
    injected: usize,
}

/// 명령 큐 하나의 입장 장부. 서버가 하나 만들고, 소켓 경로와 주입기가 같은 것을 나눠 든다.
#[derive(Debug)]
pub struct CommandAdmission {
    held: Mutex<Held>,
    limits: QueueLimits,
    peak_bytes: AtomicU64,
    refused_bytes: AtomicU64,
    refused_depth: AtomicU64,
}

// 장부 락 poison 보고 플래그(첫 1 회만). 임계구역이 정수 덧뺄셈뿐이라 락을 든 채 죽은 스레드가
// 불변식을 깨지 않는다 — 복구가 맞다. 조용히 삼키면 모든 IPC 요청이 입장에서 멈춘다.
static HELD_POISONED: AtomicBool = AtomicBool::new(false);
const HELD_WHAT: &str = "ipc command admission ledger";

impl CommandAdmission {
    /// 주어진 상한으로 빈 장부를 만든다.
    pub fn new(limits: QueueLimits) -> Arc<Self> {
        Arc::new(Self {
            held: Mutex::new(Held::default()),
            limits,
            peak_bytes: AtomicU64::new(0),
            refused_bytes: AtomicU64::new(0),
            refused_depth: AtomicU64::new(0),
        })
    }

    /// 이 장부의 상한.
    pub fn limits(&self) -> QueueLimits {
        self.limits
    }

    /// `bytes` 무게의 명령 하나를 들일지 판정한다. 들이면 몫을 든 표를 돌려준다.
    ///
    /// **빈 큐는 한 건을 늘 받는다.** 상한보다 큰 한 건을 영영 못 들이면 그 요청은 어떤
    /// 부하에서도 못 들어간다 — 한 건의 크기는 이 장부가 아니라 줄 상한의 몫이다.
    /// 주입 깊이 판정이 바이트보다 먼저다: 주입 적체는 바이트가 작아도 그 자체로 신호다.
    pub fn admit(
        self: &Arc<Self>,
        bytes: usize,
        origin: Origin,
    ) -> Result<AdmissionTicket, Refusal> {
        let mut held =
            tasty_utils::poison::recover_mutex(self.held.lock(), HELD_WHAT, &HELD_POISONED);
        if origin == Origin::Injected && held.injected >= self.limits.injected_depth {
            self.refused_depth.fetch_add(1, Ordering::Relaxed);
            return Err(Refusal::InjectedDepth {
                queued: held.injected,
                limit: self.limits.injected_depth,
            });
        }
        if held.bytes > 0 && held.bytes.saturating_add(bytes) > self.limits.queued_bytes {
            self.refused_bytes.fetch_add(1, Ordering::Relaxed);
            return Err(Refusal::QueueBytes {
                queued: held.bytes,
                request: bytes,
                limit: self.limits.queued_bytes,
            });
        }
        held.bytes = held.bytes.saturating_add(bytes);
        held.commands += 1;
        if origin == Origin::Injected {
            held.injected += 1;
        }
        self.peak_bytes
            .fetch_max(held.bytes as u64, Ordering::Relaxed);
        Ok(AdmissionTicket {
            ledger: self.clone(),
            bytes,
            origin,
        })
    }

    fn release(&self, bytes: usize, origin: Origin) {
        let mut held =
            tasty_utils::poison::recover_mutex(self.held.lock(), HELD_WHAT, &HELD_POISONED);
        held.bytes = held.bytes.saturating_sub(bytes);
        held.commands = held.commands.saturating_sub(1);
        if origin == Origin::Injected {
            held.injected = held.injected.saturating_sub(1);
        }
    }

    /// 지금 값.
    pub fn snapshot(&self) -> AdmissionSnapshot {
        let held = tasty_utils::poison::recover_mutex(self.held.lock(), HELD_WHAT, &HELD_POISONED);
        AdmissionSnapshot {
            queued_bytes: held.bytes as u64,
            queued_commands: held.commands as u64,
            queued_injected: held.injected as u64,
            peak_bytes: self.peak_bytes.load(Ordering::Relaxed),
            refused_bytes: self.refused_bytes.load(Ordering::Relaxed),
            refused_depth: self.refused_depth.load(Ordering::Relaxed),
        }
    }
}

/// 큐에 든 명령 하나의 몫. 버려질 때 장부에서 빠진다.
#[derive(Debug)]
pub struct AdmissionTicket {
    ledger: Arc<CommandAdmission>,
    bytes: usize,
    origin: Origin,
}

impl Drop for AdmissionTicket {
    fn drop(&mut self) {
        self.ledger.release(self.bytes, self.origin);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn small(bytes: usize, depth: usize) -> Arc<CommandAdmission> {
        CommandAdmission::new(QueueLimits {
            queued_bytes: bytes,
            injected_depth: depth,
        })
    }

    #[test]
    fn bytes_past_the_limit_are_refused_and_counted() {
        let a = small(100, 10);
        let t1 = a.admit(60, Origin::Socket).expect("first fits");
        let refused = a.admit(50, Origin::Socket).expect_err("60 + 50 > 100");
        assert_eq!(
            refused,
            Refusal::QueueBytes {
                queued: 60,
                request: 50,
                limit: 100
            }
        );
        let t2 = a.admit(40, Origin::Socket).expect("60 + 40 == 100 fits");
        let s = a.snapshot();
        assert_eq!(
            (s.queued_bytes, s.queued_commands, s.refused_bytes),
            (100, 2, 1)
        );
        drop((t1, t2));
    }

    #[test]
    fn an_empty_queue_takes_one_request_larger_than_the_limit() {
        let a = small(100, 10);
        let big = a
            .admit(500, Origin::Socket)
            .expect("an empty queue always takes one");
        assert!(
            a.admit(1, Origin::Socket).is_err(),
            "anything beside it is over"
        );
        drop(big);
        assert!(
            a.admit(1, Origin::Socket).is_ok(),
            "released with the ticket"
        );
    }

    #[test]
    fn dropping_the_ticket_returns_its_share() {
        let a = small(100, 10);
        let t = a.admit(80, Origin::Injected).unwrap();
        assert_eq!(a.snapshot().queued_injected, 1);
        drop(t);
        let s = a.snapshot();
        assert_eq!(
            (s.queued_bytes, s.queued_commands, s.queued_injected),
            (0, 0, 0)
        );
        assert_eq!(s.peak_bytes, 80, "the peak stays");
    }

    #[test]
    fn only_injected_commands_meet_the_depth_limit() {
        let a = small(1_000, 2);
        let i1 = a.admit(1, Origin::Injected).unwrap();
        let i2 = a.admit(1, Origin::Injected).unwrap();
        assert_eq!(
            a.admit(1, Origin::Injected).expect_err("third injected"),
            Refusal::InjectedDepth {
                queued: 2,
                limit: 2
            }
        );
        let s1 = a
            .admit(1, Origin::Socket)
            .expect("socket commands are not counted by depth");
        assert_eq!(a.snapshot().refused_depth, 1);
        drop(i1);
        let i3 = a.admit(1, Origin::Injected).expect("a slot came back");
        drop((i2, i3, s1));
    }

    #[test]
    fn the_refusal_text_says_what_was_full() {
        let bytes = Refusal::QueueBytes {
            queued: 1,
            request: 2,
            limit: 3,
        }
        .to_string();
        assert!(
            bytes.contains("1 bytes") && bytes.contains("limit 3"),
            "{bytes}"
        );
        let depth = Refusal::InjectedDepth {
            queued: 4,
            limit: 4,
        }
        .to_string();
        assert!(depth.contains("host-injected"), "{depth}");
    }
}
