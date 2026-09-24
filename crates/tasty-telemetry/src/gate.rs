//! 권한 → 비용 제한 → 호출 빈도 제한 순서의 진입 검사 결과를 집계한다.
//! 먼저 거절되면 뒤 검사는 하지 않으므로 한 요청은 최대 한 거절 항목에 기록된다.
//! 세 검사를 통과했어도 이후 멱등 키 형식 검사나 handler에서 실패할 수 있다.
//!
//! Permission에는 권한 부족뿐 아니라 Plugin/Agent의 알 수 없는 메서드와
//! 해당 호출자에게 열리지 않은 메서드도 포함된다. 뒤의 두 경우는 권한을 추가해도
//! 해결되지 않는다. Local은 권한 검사에서 거절하지 않는다.
//!
//! 프로세스 시작부터 누적하며 재시작하면 0이 된다. 영속 버킷의 throttled_count와 달리
//! 게이트 밖 try_consume 거절은 세지 않는다. 자세한 범위는
//! docs/architecture/ipc-server.md#진입-검사-거절-집계 참조.

use std::sync::atomic::{AtomicU64, Ordering};

/// 요청을 돌려보낸 게이트. 판정 순서와 같은 순서로 적는다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateRefusal {
    /// 권한 검사에서 거절한 요청. Plugin/Agent의 권한 부족·없는 메서드·비공개 메서드를 포함한다.
    /// 앞선 토큰 검사와 뒤의 handler가 같은 -32001 코드를 내도 여기에는 세지 않는다.
    /// Local은 이 검사에서 거절하지 않는다.
    Permission,
    /// 텔레메트리 cap 게이트 — 걸린 cap 이 호출자를 막고 있다(`-32007`).
    Cap,
    /// rate limit 게이트 — 호출자의 `ipc_calls` 버킷이 비었다(`-32010`).
    Throttle,
}

/// 프로세스 수명 동안의 검사 횟수. Relaxed 카운터를 따로 읽으므로 스냅샷은 원자적이지 않다.
/// 읽는 중 갱신되면 judged가 거절 합보다 잠깐 작게 보일 수 있다.
#[derive(Debug, Default)]
pub struct GateStats {
    /// Local을 포함한 진입 요청 수. IPC 큐를 거치지 않는 plugin host-call도 센다.
    judged: AtomicU64,
    permission_denied: AtomicU64,
    cap_blocked: AtomicU64,
    throttled: AtomicU64,
}

impl GateStats {
    /// 요청 하나가 게이트에 들어왔다. 판정 결과와 무관하게 한 번 부른다.
    pub fn record_judged(&self) {
        self.judged.fetch_add(1, Ordering::Relaxed);
    }

    /// 요청 하나를 `by` 가 돌려보냈다.
    pub fn record_refusal(&self, by: GateRefusal) {
        let slot = match by {
            GateRefusal::Permission => &self.permission_denied,
            GateRefusal::Cap => &self.cap_blocked,
            GateRefusal::Throttle => &self.throttled,
        };
        slot.fetch_add(1, Ordering::Relaxed);
    }

    pub fn snapshot(&self) -> GateSnapshot {
        GateSnapshot {
            judged: self.judged.load(Ordering::Relaxed),
            permission_denied: self.permission_denied.load(Ordering::Relaxed),
            cap_blocked: self.cap_blocked.load(Ordering::Relaxed),
            throttled: self.throttled.load(Ordering::Relaxed),
        }
    }
}

/// [`GateStats`] 의 한 시점 읽기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub struct GateSnapshot {
    pub judged: u64,
    pub permission_denied: u64,
    pub cap_blocked: u64,
    pub throttled: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_refusal_lands_in_its_own_slot() {
        let g = GateStats::default();
        for _ in 0..4 {
            g.record_judged();
        }
        g.record_refusal(GateRefusal::Permission);
        g.record_refusal(GateRefusal::Cap);
        g.record_refusal(GateRefusal::Throttle);
        g.record_refusal(GateRefusal::Throttle);
        assert_eq!(
            g.snapshot(),
            GateSnapshot {
                judged: 4,
                permission_denied: 1,
                cap_blocked: 1,
                throttled: 2,
            }
        );
    }
}
