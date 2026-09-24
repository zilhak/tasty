//! 호출 빈도·반복과 RSS 증가를 보고 조사할 만한 신호를 찾는다.
//! 실제 정체·성능 저하·메모리 누수를 확정하지 않으며 오탐과 누락이 있을 수 있다.

use std::sync::atomic::AtomicBool;

use serde::{Deserialize, Serialize};

pub const ANOMALY_KEY_PREFIX: &str = "tasty.telemetry.anomaly.";

/// `tasty.telemetry.anomaly.{ts:013}.{id}` — 시간 정렬 prefix scan 가능.
pub fn anomaly_key(ts: u64, id: &str) -> String {
    format!("{ANOMALY_KEY_PREFIX}{ts:013}.{id}")
}

/// 검출된 이상 신호의 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AnomalyKind {
    /// 짧은 시간 안에 동일 IPC 메서드가 비정상적으로 많이 호출됨.
    CallBurst,
    /// 같은 method와 params가 반복된다. 정상 폴링도 포함되므로 진행 정체를 뜻하지는 않는다.
    SlowLoop,
    /// 최근 RSS 샘플이 계속 증가한다. 정상 캐시 증가와 누수를 구별하지는 못한다.
    RssSurge,
}

impl AnomalyKind {
    pub fn as_token(&self) -> &'static str {
        match self {
            AnomalyKind::CallBurst => "call_burst",
            AnomalyKind::SlowLoop => "slow_loop",
            AnomalyKind::RssSurge => "rss_surge",
        }
    }
}

/// 영속 anomaly 레코드. 검출 시점에 host 가 memory + notification 으로 내보낸다.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Anomaly {
    pub id: String,
    pub kind: AnomalyKind,
    pub agent: String,
    /// 관련 메서드/메트릭 식별자 — `CallBurst`/`SlowLoop` 는 method 이름,
    /// `RssSurge` 는 [`RSS_METRIC_NAME`].
    pub subject: String,
    /// unix ms.
    pub detected_at: u64,
    /// 휴리스틱별 보조 정보 (count, window_ms, threshold 등).
    pub detail: serde_json::Value,
}

/// CallBurst 휴리스틱: (agent, method) 단위로 호출 시각의 sliding window 를
/// 유지하고, 윈도우 안의 카운트가 임계 이상이면 anomaly 를 발화한다.
pub const CALL_BURST_WINDOW_MS: u64 = 60_000;
pub const CALL_BURST_THRESHOLD: usize = 1000;
/// 같은 (agent, kind, subject) 의 anomaly 가 연속 emit 되지 않도록 막는 쿨다운.
/// 세 검출기 모두 공유.
pub const ANOMALY_DEDUP_COOLDOWN_MS: u64 = 60_000;

/// SlowLoop 휴리스틱: (agent, method, params-hash) 단위로 호출 시각의
/// sliding window 를 유지하고, 5분 안에 동일 파라미터 조합이 20회 이상
/// 반복되면 anomaly 를 발화한다.
pub const SLOW_LOOP_WINDOW_MS: u64 = 300_000;
pub const SLOW_LOOP_THRESHOLD: usize = 20;

/// 최근 N개 RSS 샘플이 각각 직전보다 커야 한다. 증가 뒤 평탄해지는 패턴은 제외한다.
pub const RSS_SURGE_MIN_SAMPLES: usize = 5;
/// `telemetry.record`/host sampling 이 RSS 를 보고할 때 쓰는 고정 메트릭 이름.
/// `sysinfo::Process::memory()`(0.35, 단위: bytes) 또는 agent 자가보고 값을
/// 그대로 담는다 — 단위 변환은 하지 않는다.
pub const RSS_METRIC_NAME: &str = "rss_bytes";

fn hash_params(params: &serde_json::Value) -> u64 {
    use std::hash::{Hash, Hasher};
    // serde_json 은 워크스페이스 전체에서 `preserve_order` feature 를 켜지
    // 않으므로 `Map` 이 `BTreeMap` 기반 — 키 순서가 항상 정렬되어 결정적
    // 문자열화가 가능하다 (동일 논리 파라미터가 항상 동일 해시).
    let s = serde_json::to_string(params).unwrap_or_default();
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    s.hash(&mut hasher);
    hasher.finish()
}

/// In-memory 이상 탐지기. 호스트 singleton 으로 공유 (`Arc<AnomalyDetector>`).
///
/// 모든 상태가 in-memory — 호스트 재시작 시 윈도우가 비워진다 (의도). 검출된
/// anomaly **레코드** 는 호스트가 memory store 에 영속.
#[derive(Debug, Default)]
pub struct AnomalyDetector {
    /// (agent, method) → 1분 윈도우의 호출 시각 (ms). CallBurst 용.
    call_windows: std::sync::Mutex<
        std::collections::HashMap<(String, String), std::collections::VecDeque<u64>>,
    >,
    /// (agent, method, params_hash) → 5분 윈도우의 호출 시각 (ms). SlowLoop 용.
    loop_windows: std::sync::Mutex<
        std::collections::HashMap<(String, String, u64), std::collections::VecDeque<u64>>,
    >,
    /// agent → 최근 [`RSS_SURGE_MIN_SAMPLES`]개의 RSS 샘플 (bytes). RssSurge 용.
    rss_samples:
        std::sync::Mutex<std::collections::HashMap<String, std::collections::VecDeque<u64>>>,
    /// 마지막 emit 시각 — 같은 (agent, kind, subject) 의 연속 emit 방지.
    last_emitted: std::sync::Mutex<std::collections::HashMap<(String, AnomalyKind, String), u64>>,
}

/// 탐지용 자료구조 락은 poison을 복구해 관측을 계속하고, 각각 처음 한 번 보고한다.
static CALL_WINDOWS_POISONED: AtomicBool = AtomicBool::new(false);
static LOOP_WINDOWS_POISONED: AtomicBool = AtomicBool::new(false);
static RSS_SAMPLES_POISONED: AtomicBool = AtomicBool::new(false);
static LAST_EMITTED_POISONED: AtomicBool = AtomicBool::new(false);

const CALL_WINDOWS_WHAT: &str = "telemetry call-burst window";
const LOOP_WINDOWS_WHAT: &str = "telemetry slow-loop window";
const RSS_SAMPLES_WHAT: &str = "telemetry rss sample window";
const LAST_EMITTED_WHAT: &str = "telemetry anomaly dedup map";

impl AnomalyDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// IPC 호출 1회 기록 + CallBurst/SlowLoop 검출. 한 호출에서 두 종류가
    /// 동시에 발화할 수 있으므로 `Vec` 을 반환한다 (보통은 비어있음).
    ///
    /// 호스트 dispatcher 의 `record_ipc_call` 직후에서 호출되도록 설계.
    /// `agent` 가 `_host` 이거나 method 가 `telemetry.` 로 시작하면 호출하지 않는다
    /// (호스트 측이 책임). `id_seq` 는 anomaly id 의 단조 증가 컴포넌트 — 두
    /// anomaly 가 같은 tick 에서 함께 발화해도 id 가 겹치지 않도록 SlowLoop 쪽은
    /// `id_seq` 를 1 증가시켜 사용한다.
    pub fn record_call(
        &self,
        agent: &str,
        method: &str,
        params: &serde_json::Value,
        ts_ms: u64,
        id_seq: u64,
    ) -> Vec<Anomaly> {
        let mut out = Vec::new();
        if let Some(a) = self.check_call_burst(agent, method, ts_ms, id_seq) {
            out.push(a);
        }
        let params_hash = hash_params(params);
        if let Some(a) =
            self.check_slow_loop(agent, method, params_hash, ts_ms, id_seq.wrapping_add(1))
        {
            out.push(a);
        }
        out
    }

    fn check_call_burst(
        &self,
        agent: &str,
        method: &str,
        ts_ms: u64,
        id_seq: u64,
    ) -> Option<Anomaly> {
        let key = (agent.to_string(), method.to_string());
        let count = {
            let mut windows = tasty_utils::poison::recover_mutex(
                self.call_windows.lock(),
                CALL_WINDOWS_WHAT,
                &CALL_WINDOWS_POISONED,
            );
            let dq = windows.entry(key).or_default();
            dq.push_back(ts_ms);
            let cutoff = ts_ms.saturating_sub(CALL_BURST_WINDOW_MS);
            while let Some(&front) = dq.front() {
                if front < cutoff {
                    dq.pop_front();
                } else {
                    break;
                }
            }
            dq.len()
        };

        if count < CALL_BURST_THRESHOLD {
            return None;
        }

        let dedup_key = (
            agent.to_string(),
            AnomalyKind::CallBurst,
            method.to_string(),
        );
        if !self.try_mark_emitted(dedup_key, ts_ms) {
            return None;
        }

        Some(Anomaly {
            id: anomaly_id(ts_ms, id_seq),
            kind: AnomalyKind::CallBurst,
            agent: agent.to_string(),
            subject: method.to_string(),
            detected_at: ts_ms,
            detail: serde_json::json!({
                "window_ms": CALL_BURST_WINDOW_MS,
                "threshold": CALL_BURST_THRESHOLD,
                "count": count,
            }),
        })
    }

    fn check_slow_loop(
        &self,
        agent: &str,
        method: &str,
        params_hash: u64,
        ts_ms: u64,
        id_seq: u64,
    ) -> Option<Anomaly> {
        let key = (agent.to_string(), method.to_string(), params_hash);
        let count = {
            let mut windows = tasty_utils::poison::recover_mutex(
                self.loop_windows.lock(),
                LOOP_WINDOWS_WHAT,
                &LOOP_WINDOWS_POISONED,
            );
            let dq = windows.entry(key).or_default();
            dq.push_back(ts_ms);
            let cutoff = ts_ms.saturating_sub(SLOW_LOOP_WINDOW_MS);
            while let Some(&front) = dq.front() {
                if front < cutoff {
                    dq.pop_front();
                } else {
                    break;
                }
            }
            dq.len()
        };

        if count < SLOW_LOOP_THRESHOLD {
            return None;
        }

        // dedup subject 에 params_hash 를 포함 — 같은 method 라도 파라미터
        // 조합이 다르면 독립된 loop 로 취급(각자 자기 쿨다운을 가진다).
        let dedup_key = (
            agent.to_string(),
            AnomalyKind::SlowLoop,
            format!("{method}#{params_hash:016x}"),
        );
        if !self.try_mark_emitted(dedup_key, ts_ms) {
            return None;
        }

        Some(Anomaly {
            id: anomaly_id(ts_ms, id_seq),
            kind: AnomalyKind::SlowLoop,
            agent: agent.to_string(),
            subject: method.to_string(),
            detected_at: ts_ms,
            detail: serde_json::json!({
                "window_ms": SLOW_LOOP_WINDOW_MS,
                "threshold": SLOW_LOOP_THRESHOLD,
                "count": count,
                "params_hash": format!("{params_hash:016x}"),
            }),
        })
    }

    /// RSS 샘플을 기록하고 증가 신호를 찾는다. 같은 agent 문자열은 같은 기록을 공유한다.
    /// 호출자가 Plugin 샘플과 Agent 자체 보고의 식별자를 구분해야 한다.
    pub fn record_rss_sample(
        &self,
        agent: &str,
        rss_bytes: u64,
        ts_ms: u64,
        id_seq: u64,
    ) -> Option<Anomaly> {
        let (is_monotonic, snapshot) = {
            let mut samples = tasty_utils::poison::recover_mutex(
                self.rss_samples.lock(),
                RSS_SAMPLES_WHAT,
                &RSS_SAMPLES_POISONED,
            );
            let dq = samples.entry(agent.to_string()).or_default();
            dq.push_back(rss_bytes);
            while dq.len() > RSS_SURGE_MIN_SAMPLES {
                dq.pop_front();
            }
            let monotonic = dq.len() == RSS_SURGE_MIN_SAMPLES
                && dq.iter().zip(dq.iter().skip(1)).all(|(a, b)| a < b);
            (monotonic, dq.iter().copied().collect::<Vec<_>>())
        };

        if !is_monotonic {
            return None;
        }

        let dedup_key = (
            agent.to_string(),
            AnomalyKind::RssSurge,
            RSS_METRIC_NAME.to_string(),
        );
        if !self.try_mark_emitted(dedup_key, ts_ms) {
            return None;
        }

        Some(Anomaly {
            id: anomaly_id(ts_ms, id_seq),
            kind: AnomalyKind::RssSurge,
            agent: agent.to_string(),
            subject: RSS_METRIC_NAME.to_string(),
            detected_at: ts_ms,
            detail: serde_json::json!({
                "samples": snapshot,
                "min_samples": RSS_SURGE_MIN_SAMPLES,
                "latest_rss_bytes": rss_bytes,
            }),
        })
    }

    /// dedup 체크 + emit 마킹을 한 번에. 쿨다운 내면 `false`(발화 취소).
    fn try_mark_emitted(&self, dedup_key: (String, AnomalyKind, String), ts_ms: u64) -> bool {
        let mut last = tasty_utils::poison::recover_mutex(
            self.last_emitted.lock(),
            LAST_EMITTED_WHAT,
            &LAST_EMITTED_POISONED,
        );
        if let Some(&prev) = last.get(&dedup_key)
            && ts_ms.saturating_sub(prev) < ANOMALY_DEDUP_COOLDOWN_MS
        {
            return false;
        }
        last.insert(dedup_key, ts_ms);
        true
    }
}

fn anomaly_id(ts_ms: u64, id_seq: u64) -> String {
    format!("anom_{ts_ms:013}{seq:04}", seq = id_seq % 10_000)
}

#[cfg(test)]
mod poison_tests {
    use super::*;

    /// 락 poison을 복구한 뒤에도 RSS 증가를 검출하는지 확인한다.
    #[test]
    fn a_poisoned_sample_window_still_fires() {
        let d = AnomalyDetector::new();
        let held = std::sync::Arc::new(d);
        let poisoner = std::sync::Arc::clone(&held);
        // poison이 실제로 발생했는지 확인한 뒤 복구 경로를 검사한다.
        std::thread::spawn(move || {
            let _guard = poisoner.rss_samples.lock().expect("fresh lock");
            panic!("poison the rss sample window on purpose");
        })
        .join()
        .expect_err("패닉한 스레드는 Err 로 join 된다");
        assert!(held.rss_samples.is_poisoned(), "전제: 락이 poison 이다");

        let mut fired = None;
        for i in 0..RSS_SURGE_MIN_SAMPLES {
            let rss = 100_000_000 + (i as u64) * 10_000_000;
            fired = held.record_rss_sample("plugin_a", rss, 1_000 + i as u64, i as u64);
        }
        let a = fired.expect("poison 이후에도 RssSurge 가 발화해야 한다");
        assert_eq!(a.kind, AnomalyKind::RssSurge);
    }
}
