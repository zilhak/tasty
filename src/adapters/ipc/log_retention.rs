//! IPC 관측 로그 3종(audit · telemetry event · telemetry anomaly)의 보존 정책 —
//! **단일 소스**.
//!
//! 세 로그 모두 append-only 로 `memory.db` 에 쌓이고, 셋 다 IPC 호출량에 정비례한다.
//! 상한을 각자 들고 있으면 어긋난다: 실제로 audit 은 런타임 경로가 "30일", 부팅
//! 경로가 "5만 건" 이라는 **720배 차이 나는 두 값**을 따로 들고 있었고, 그래서
//! 어느 쪽도 실효가 없었다(런타임은 30일 미만이라 0건 삭제, 부팅은 재시작 전까지
//! 미집행). 정책과 집행을 이 모듈 한 곳에 모아 그 재발을 막는다. 근거는
//! [ADR-0085](../../../../docs/adr/0085-ipc-log-retention-bounded.md).
//!
//! ## 두 축을 함께 건다
//!
//! - **개수 상한(`keep`)** — DB 크기를 bound 한다. 집행이 주기적이라(아래
//!   [`APPEND_PRUNE_INTERVAL_MS`]) 그 사이 유입만큼은 상한을 넘을 수 있다 —
//!   정상 상태 최대치는 `keep + 1시간치 유입`이고, 집행마다 다시 `keep` 으로
//!   떨어진다. 유입 속도가 아무리 빨라도 무한히 늘지는 않는다는 것이 이 축의
//!   보장이다.
//! - **시간 상한(`ttl_ms`)** — "상한 안이지만 이미 무의미하게 오래된" 로그를 지운다.
//!   유입이 끊긴 인스턴스에서는 개수 상한이 영원히 안 걸리기 때문이다.
//!
//! 한 축만 걸면 반대쪽에 사각이 생긴다. 다만 유입 속도가 시간 상한을 무의미하게
//! 만드는 로그(telemetry event)는 개수 상한만 건다 — 아래 [`TELEMETRY_EVENT`] 참고.
//!
//! ## 두 경로가 같은 구현을 부른다
//!
//! - **부팅**: `boot::maintain_memory_at_boot` 이 [`ALL`] 을 순회.
//! - **런타임**: 세 로그의 append 경로가 [`maybe_prune_on_append`] 를 호출한다.
//!   게이트가 프로세스 전역이라 어느 writer 가 먼저 도착하든 주기당 1회만 돈다.

use std::sync::atomic::{AtomicU64, Ordering};

use tasty_memory::MemoryStorage;

/// 로그 한 종류의 보존 정책.
pub struct LogRetention {
    /// 이 로그의 memory 키 prefix. 키는 `{prefix}{ts:013}...` 형태여야 한다 —
    /// 두 prune 이 모두 "lexical = chronological" 에 기댄다.
    pub prefix: &'static str,
    /// 남길 최대 행 수.
    pub keep: u64,
    /// 이보다 오래된 행은 개수 상한과 무관하게 삭제. `None` = 개수 상한만.
    pub ttl_ms: Option<u64>,
}

impl LogRetention {
    /// 이 정책을 1회 집행한다. 삭제된 행 수를 반환하며, 실패는 warn 후 0.
    pub fn enforce(&self, mem: &mut dyn MemoryStorage, now_ms: u64) -> u64 {
        let mut removed = 0u64;
        if let Some(ttl) = self.ttl_ms {
            let cutoff = now_ms.saturating_sub(ttl);
            match mem.prune_prefix_older_than(self.prefix, cutoff) {
                Ok(n) => removed += n,
                Err(e) => tracing::warn!("log retention: ttl prune {} failed: {e}", self.prefix),
            }
        }
        match mem.prune_prefix_keep_recent(self.prefix, self.keep) {
            Ok(n) => removed += n,
            Err(e) => tracing::warn!("log retention: count prune {} failed: {e}", self.prefix),
        }
        removed
    }
}

/// 시간 상한 공통값 — **50시간**(사용자 지정). 하루를 넘겨 "어제 그 사고" 를 되짚을
/// 수 있고, 30일처럼 append rate 와 양립 불가능하지도 않은 지점이다(일 ~100MB 유입에
/// 30일이면 3GB 로, `memory.db` 의 1GB regular quota 를 애초에 넘는다).
pub const LOG_TTL_MS: u64 = 50 * 60 * 60 * 1_000;

/// IPC audit — deny 만 기록되므로 평시 유입이 거의 없다. 상한은 폭주 방어용 안전망.
pub const AUDIT: LogRetention = LogRetention {
    prefix: crate::adapters::ipc::audit::AUDIT_KEY_PREFIX,
    keep: 50_000,
    ttl_ms: Some(LOG_TTL_MS),
};

/// telemetry raw event — **개수 상한만** 건다.
///
/// 시간 상한은 이 유입 속도에 맞지 않는다: 실측 시간당 28,075건이라 50시간이면
/// 1,403,750건으로, 개수 상한(2만)의 70배이고 문제를 발견했을 당시 행 수(30만)의
/// 4.5배다. 즉 시간 상한을 걸면 지금보다 나빠진다.
///
/// raw event 는 조회의 유일한 SoT 다(영속 rollup bucket 은 존재하지 않는다). 그래서
/// 이 상한이 곧 **telemetry 조회 가능 범위**다 — "최근 2만 이벤트".
pub const TELEMETRY_EVENT: LogRetention = LogRetention {
    prefix: tasty_telemetry::EVENT_KEY_PREFIX,
    keep: 20_000,
    ttl_ms: None,
};

/// telemetry anomaly — 셋 중 가장 좁다.
///
/// 자동 대응이 없는 **읽히지 않는 경고**라(소비처가 `telemetry.anomaly.list` 조회
/// 하나뿐이다) 오래 보관해서 얻는 값이 가장 작다. 그럼에도 폴링형 워크로드에서는
/// `SlowLoop` 이 params 조합 수만큼 배증돼 시간당 1,000건대로 쌓인다(실측 18시간
/// 21,102건). 5,000 은 그 최악 유입의 4시간치이자, 평상시로는 수개월치다.
pub const TELEMETRY_ANOMALY: LogRetention = LogRetention {
    prefix: tasty_telemetry::ANOMALY_KEY_PREFIX,
    keep: 5_000,
    ttl_ms: Some(LOG_TTL_MS),
};

/// 부팅·런타임 양쪽이 순회하는 전체 목록. 새 관측 로그를 추가하면 여기에 넣는다 —
/// 넣지 않으면 정리 경로가 없는 로그가 된다(anomaly 가 정확히 그 상태였다).
pub const ALL: [LogRetention; 3] = [AUDIT, TELEMETRY_EVENT, TELEMETRY_ANOMALY];

/// append 경로 집행 주기 — 1시간. 스캔이 아니라 인덱스 DELETE 두 방이지만, IPC 마다
/// 돌 이유는 없으므로 시간으로 상한한다.
pub const APPEND_PRUNE_INTERVAL_MS: u64 = 60 * 60 * 1_000;

/// 마지막 집행 시각(프로세스 전역). 부팅 후 첫 append 에서도 1회 돈다(last=0) —
/// 이미 쌓여 있던 로그가 재시작 없이도 그 즉시 정리된다.
static LAST_PRUNE_MS: AtomicU64 = AtomicU64::new(0);

/// 로그 append 직후 호출 — 주기가 찼으면 [`ALL`] 을 집행한다.
///
/// 세 로그가 **같은 게이트**를 공유한다. 어느 로그의 append 가 주기를 채우든 셋 다
/// 정리되므로, "유입이 멈춘 로그는 정리도 멈춘다" 는 사각이 생기지 않는다 — audit 이
/// allow 기록을 그만두면서 append 자체가 희소해진 지금 이 성질이 특히 중요하다.
pub fn maybe_prune_on_append(mem: &mut dyn MemoryStorage, now_ms: u64) {
    let last = LAST_PRUNE_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < APPEND_PRUNE_INTERVAL_MS {
        return;
    }
    if LAST_PRUNE_MS
        .compare_exchange(last, now_ms, Ordering::Relaxed, Ordering::Relaxed)
        .is_err()
    {
        return; // 다른 스레드가 이번 주기를 가져갔다
    }
    let mut removed = 0u64;
    for policy in &ALL {
        removed += policy.enforce(mem, now_ms);
    }
    if removed > 0 {
        tracing::debug!("log retention: append-path pruned {removed} rows");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tasty_memory::{MemoryValue, PutOpts, Scope, testing::InMemoryStorage};

    fn put_log(mem: &mut InMemoryStorage, prefix: &str, ts: u64, seq: u64) {
        mem.put(
            tasty_memory::HOST_OWNER,
            &Scope::Global,
            &format!("{prefix}{ts:013}.{seq:04}"),
            &MemoryValue::Text("x".into()),
            &PutOpts::default(),
        )
        .unwrap();
    }

    fn rows(mem: &InMemoryStorage, prefix: &str) -> usize {
        mem.list(
            &Scope::Global,
            &tasty_memory::ListOpts {
                prefix: Some(prefix.to_string()),
                ..Default::default()
            },
        )
        .unwrap()
        .len()
    }

    /// 정책 테이블이 세 prefix 를 **전부** 덮는지. anomaly 가 목록에서 빠져 있어
    /// 유일하게 회수 불가능한 로그였던 것이 이 트랙의 출발점이다.
    #[test]
    fn every_ipc_log_prefix_has_a_policy() {
        let covered: Vec<&str> = ALL.iter().map(|p| p.prefix).collect();
        for expected in [
            crate::adapters::ipc::audit::AUDIT_KEY_PREFIX,
            tasty_telemetry::EVENT_KEY_PREFIX,
            tasty_telemetry::ANOMALY_KEY_PREFIX,
        ] {
            assert!(
                covered.contains(&expected),
                "'{expected}' 에 보존 정책이 없다 — 정리 경로 없는 로그가 된다"
            );
        }
    }

    /// 상한을 넘긴 행이 개수 기준으로 잘리고, 다른 prefix 는 건드리지 않는다.
    #[test]
    fn count_cap_trims_only_its_own_prefix() {
        let mut mem = InMemoryStorage::new();
        for i in 0..10u64 {
            put_log(&mut mem, "tasty.audit.", 1_000 + i, 0);
            put_log(&mut mem, "tasty.telemetry.event.", 1_000 + i, 0);
        }
        let policy = LogRetention {
            prefix: "tasty.audit.",
            keep: 3,
            ttl_ms: None,
        };
        assert_eq!(policy.enforce(&mut mem, 10_000), 7);
        assert_eq!(rows(&mem, "tasty.audit."), 3);
        assert_eq!(rows(&mem, "tasty.telemetry.event."), 10);

        // 남는 것은 **최신** 쪽이어야 한다. telemetry 조회가 raw event 를 즉석
        // 집계하므로(영속 bucket 없음), 정리가 최신을 지우면 "방금 것" 이 조회에서
        // 사라진다 — 상한이 곧 조회 범위라는 계약이 여기서 성립한다.
        let survivors: Vec<String> = mem
            .list(
                &Scope::Global,
                &tasty_memory::ListOpts {
                    prefix: Some("tasty.audit.".to_string()),
                    ..Default::default()
                },
            )
            .unwrap()
            .into_iter()
            .map(|e| e.key)
            .collect();
        for ts in [1_007u64, 1_008, 1_009] {
            assert!(
                survivors.contains(&format!("tasty.audit.{ts:013}.0000")),
                "최신 {ts} 가 지워졌다"
            );
        }
    }

    /// 시간 상한은 개수 상한 안에 있는 행도 지운다 — 유입이 끊긴 인스턴스에서
    /// 개수 상한이 영원히 안 걸리는 사각을 메우는 것이 이 축의 존재 이유다.
    #[test]
    fn ttl_removes_rows_that_the_count_cap_would_keep() {
        let mut mem = InMemoryStorage::new();
        put_log(&mut mem, "tasty.audit.", 1_000, 0);
        put_log(&mut mem, "tasty.audit.", 9_000, 0);
        let policy = LogRetention {
            prefix: "tasty.audit.",
            keep: 1_000, // 개수로는 하나도 안 걸린다
            ttl_ms: Some(2_000),
        };
        assert_eq!(policy.enforce(&mut mem, 10_000), 1);
        assert_eq!(rows(&mem, "tasty.audit."), 1);
    }
}
