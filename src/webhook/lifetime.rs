//! 웹훅의 영속성(Persistent/Temporary)과 제한(Unlimited/TimeLimit/CountLimit).
//! Persistent는 ID와 남은 제한을 저장하려고 시도하며 Temporary는 저장하지 않는다.
//! 시간 제한은 Unix 절대 시각, 횟수 제한은 매칭·인증을 통과한 요청 수로 판단한다.
//! 횟수 차감은 시퀀스 실행 성공 여부와 무관하다.
//!
//! 별도 만료 타이머는 없다. 요청 시, 재시작 복원 시, 명시적 sweep에서 만료를 확인한다.
//! 따라서 시간상 만료돼도 다음 확인까지 등록 목록에는 남을 수 있다.

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
    /// 저장된 설정으로 ID와 남은 제한을 복원한다.
    Persistent,
    /// 재시작 시 소멸(config 저장 안 함).
    Temporary,
}

/// 시간·횟수 제한. 종류가 분명하게 드러나도록 enum_variant_names를 허용한다.
#[allow(clippy::enum_variant_names)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Limit {
    /// 시간·횟수로 만료되지 않는다.
    Unlimited,
    /// 절대 만료 시각(Unix epoch secs). `now >= deadline` 이면 만료.
    TimeLimit { deadline_unix: u64 },
    /// 남은 접수 횟수. 매칭·인증 통과 시 차감하고 0이면 등록을 제거한다.
    CountLimit { remaining: u64 },
}

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

    /// 횟수를 하나 차감한다. 횟수 제한이 없으면 false, 차감 후 소진됐으면 true다.
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
            limit: Limit::TimeLimit {
                deadline_unix: 1000,
            },
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
