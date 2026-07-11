//! 웹훅 lifetime — **{영속성} × {제한} = 6종**.
//!
//! - 영속성(`Persistence`): tasty 재시작 시 유지 여부만 다르다. `Persistent` 는
//!   config(`~/.tasty/webhooks.toml`)에 발급 id·잔여 제한까지 저장해 재시작 후에도
//!   URL·남은 제한을 유지하고, `Temporary` 는 재시작 시 소멸(저장 안 함).
//! - 제한(`Limit`): 초과 시 웹훅이 **자동 소멸**한다. `TimeLimit` 은 절대 만료
//!   시각(Unix epoch secs)으로 저장해 재시작 후에도 정확히 만료되고, `CountLimit`
//!   은 매칭 성공 시 1 차감해 0 도달 시 소멸한다. `Unlimited` 는 명시적
//!   `unregister`/`sweep` 전까지 유지.
//!
//! 기존 훅 `once` = `CountLimit { remaining: 1 }` 에 해당한다.
//!
//! ## 만료 집행 = 타이머 없음 (lazy + sweep)
//! 별도 백그라운드 타이머를 두지 않는다. 만료는 ① 호출 시 lazy 확인(`is_expired`)
//! ② 재시작 시 필터 ③ 명시적 `webhook.sweep` — 세 시점에만 확정된다. 한 번도 안
//! 불린 시간제한 웹훅은 그때까지 등록 상태로 남아 있어도 무방하다.

use std::time::{SystemTime, UNIX_EPOCH};

/// 현재 Unix epoch 초. 시계 오류(1970 이전 등) 시 0 으로 폴백한다.
pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// tasty 재시작 시 유지 여부.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    /// config 로 복원 — 재시작 후에도 URL·잔여 제한 유지.
    Persistent,
    /// 재시작 시 소멸(config 저장 안 함).
    Temporary,
}

/// 자동 소멸 제한 (초과 시 웹훅 등록 해제 + path 회수).
///
/// 변형명 `TimeLimit`/`CountLimit` 는 명세(research.md §2 · 작업항목 4)가 확정한
/// 이름이라 `enum_variant_names` lint 를 허용한다(제한의 종류를 이름에 담는 편이
/// `Time`/`Count` 보다 코드 가독성이 높다).
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// 제한 없음 — 명시적 unregister/sweep 전까지 유지.
    Unlimited,
    /// 절대 만료 시각(Unix epoch secs). `now >= deadline` 이면 만료.
    TimeLimit { deadline_unix: u64 },
    /// 남은 호출 횟수. 매칭 성공 시 1 차감, 0 도달 시 소멸.
    CountLimit { remaining: u64 },
}

/// 웹훅의 lifetime — 영속성 + 제한.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Lifetime {
    pub persistence: Persistence,
    pub limit: Limit,
}

impl Lifetime {
    /// 재시작 시 config 로 복원되는가.
    pub fn is_persistent(&self) -> bool {
        self.persistence == Persistence::Persistent
    }

    /// 시간제한이 만료됐는가(`now >= deadline`). 시간제한이 아니면 항상 `false`.
    pub fn is_time_expired(&self, now_unix: u64) -> bool {
        match self.limit {
            Limit::TimeLimit { deadline_unix } => now_unix >= deadline_unix,
            _ => false,
        }
    }

    /// 횟수제한이 소진됐는가(`remaining == 0`). 횟수제한이 아니면 항상 `false`.
    pub fn is_exhausted(&self) -> bool {
        matches!(self.limit, Limit::CountLimit { remaining: 0 })
    }

    /// 어느 제한으로든 만료됐는가(호출 시 lazy 확인 / sweep / 재시작 필터 공용).
    pub fn is_expired(&self, now_unix: u64) -> bool {
        self.is_time_expired(now_unix) || self.is_exhausted()
    }

    /// 매칭 성공 시 횟수를 1 차감한다(횟수제한만). 차감 후 소진됐으면 `true`.
    /// 횟수제한이 아니면 아무 것도 하지 않고 `false`.
    pub fn consume(&mut self) -> bool {
        if let Limit::CountLimit { remaining } = &mut self.limit {
            *remaining = remaining.saturating_sub(1);
            *remaining == 0
        } else {
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 테스트용 임시·무제한 lifetime(제거된 `temporary_unlimited` 헬퍼 대체).
    fn temp_unlimited() -> Lifetime {
        Lifetime {
            persistence: Persistence::Temporary,
            limit: Limit::Unlimited,
        }
    }

    #[test]
    fn unlimited_never_expires() {
        let lt = temp_unlimited();
        assert!(!lt.is_expired(now_unix()));
        assert!(!lt.is_time_expired(u64::MAX));
        assert!(!lt.is_exhausted());
    }

    #[test]
    fn time_limit_expires_at_deadline() {
        let lt = Lifetime {
            persistence: Persistence::Persistent,
            limit: Limit::TimeLimit { deadline_unix: 1000 },
        };
        assert!(!lt.is_time_expired(999));
        assert!(lt.is_time_expired(1000)); // 경계 포함(>=)
        assert!(lt.is_time_expired(1001));
        assert!(lt.is_persistent());
    }

    #[test]
    fn count_limit_consume_and_exhaust() {
        let mut lt = Lifetime {
            persistence: Persistence::Temporary,
            limit: Limit::CountLimit { remaining: 2 },
        };
        assert!(!lt.is_exhausted());
        assert!(!lt.consume()); // 2 → 1, 아직 남음
        assert!(!lt.is_exhausted());
        assert!(lt.consume()); // 1 → 0, 소진
        assert!(lt.is_exhausted());
        assert!(lt.is_expired(now_unix()));
        // saturating — 0 밑으로 안 내려가고 여전히 소진.
        assert!(lt.consume());
        assert_eq!(lt.limit, Limit::CountLimit { remaining: 0 });
    }

    #[test]
    fn consume_noop_on_non_count() {
        let mut lt = temp_unlimited();
        assert!(!lt.consume());
        assert_eq!(lt.limit, Limit::Unlimited);
    }
}
