//! 큐에 대기 중인 요청의 바이트 합과 호스트 주입 명령 수를 제한한다.
//! 소켓과 호스트 주입이 같은 장부를 쓰며 한 락 안에서 입장 여부를 결정한다.
//!
//! 소켓 요청은 받은 JSON 줄에서 앞뒤 공백·개행을 뺀 길이, 호스트 주입은 compact
//! JSON 길이를 센다. 두 값은 메모리 사용량이 아니라 전송 데이터의 바이트 수다.
//!
//! AdmissionTicket은 명령이 큐에서 빠질 때 반납한다. handler 실행 중인 요청은
//! 이 장부에서 세지 않는다. 송신 실패나 큐 폐기 때도 ticket의 Drop으로 반납한다.

use std::fmt;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

/// 대기 중인 요청 바이트 상한. ipc-server 가이드의 입장 상한을 따른다.
/// 본체는 최대 요청 두 개를 수용하면서 연결 한도 × 요청 한도보다 작다는 관계를 검사한다.
pub const QUEUED_BYTES_LIMIT: usize = 64 * 1024 * 1024;

/// 호스트 주입 명령 수 상한. 한 dispatch 회차의 명령 수 예산 이하다.
/// 시간 예산에 먼저 도달하면 남은 명령은 여러 회차에 걸쳐 처리될 수 있다.
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
    /// 소켓에서 받은 요청. 주입 명령 수 제한에는 포함하지 않는다.
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

/// system.pressure의 queue_admission에 제공하는 스냅샷.
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

// 메모리 카운터의 poison을 한 번 보고하고 복구한다.
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

    /// 요청을 받아들이면 AdmissionTicket을 반환한다. 주입 개수 제한을 먼저 검사한다.
    /// 바이트 합이 0이면 한 요청이 바이트 한도를 넘더라도 받는다. 요청 한 줄의 상한은 별도로 적용한다.
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
