//! `tasty-telemetry` anomaly detection 도메인.

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
    /// 동일 메서드 패턴이 진전 없이 반복됨. (4.4b 이후 활성화 예정)
    SlowLoop,
    /// agent 보고 RSS 가 급증함. (4.4b 이후 활성화 예정)
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
    /// 관련 메서드/메트릭 식별자 — `CallBurst` 는 method 이름이 들어간다.
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
/// 같은 (agent, method) 의 burst 가 연속 emit 되지 않도록 막는 쿨다운.
pub const ANOMALY_DEDUP_COOLDOWN_MS: u64 = 60_000;

/// In-memory 이상 탐지기. 호스트 singleton 으로 공유 (`Arc<AnomalyDetector>`).
///
/// 모든 상태가 in-memory — 호스트 재시작 시 윈도우가 비워진다 (의도). 검출된
/// anomaly **레코드** 는 호스트가 memory store 에 영속.
#[derive(Debug, Default)]
pub struct AnomalyDetector {
    /// (agent, method) → 1분 윈도우의 호출 시각 (ms).
    call_windows: std::sync::Mutex<
        std::collections::HashMap<(String, String), std::collections::VecDeque<u64>>,
    >,
    /// 마지막 emit 시각 — 같은 (agent, kind, subject) 의 연속 emit 방지.
    last_emitted: std::sync::Mutex<std::collections::HashMap<(String, AnomalyKind, String), u64>>,
}

impl AnomalyDetector {
    pub fn new() -> Self {
        Self::default()
    }

    /// IPC 호출 1회 기록 + burst 검출. anomaly 발화 시 `Some(Anomaly)` 반환.
    ///
    /// 호스트 dispatcher 의 `record_ipc_call` 직후에서 호출되도록 설계.
    /// `agent` 가 `_host` 이거나 method 가 `telemetry.` 로 시작하면 호출하지 않는다
    /// (호스트 측이 책임). `id_seq` 는 anomaly id 의 단조 증가 컴포넌트.
    pub fn record_call(
        &self,
        agent: &str,
        method: &str,
        ts_ms: u64,
        id_seq: u64,
    ) -> Option<Anomaly> {
        let key = (agent.to_string(), method.to_string());
        let count = {
            let mut windows = self.call_windows.lock().ok()?;
            let dq = windows.entry(key.clone()).or_default();
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

        // dedup — 같은 (agent, CallBurst, method) 가 쿨다운 내에 이미 emit 됐다면 skip.
        let dedup_key = (
            agent.to_string(),
            AnomalyKind::CallBurst,
            method.to_string(),
        );
        {
            let mut last = self.last_emitted.lock().ok()?;
            if let Some(&prev) = last.get(&dedup_key)
                && ts_ms.saturating_sub(prev) < ANOMALY_DEDUP_COOLDOWN_MS
            {
                return None;
            }
            last.insert(dedup_key, ts_ms);
        }

        let id = format!("anom_{ts_ms:013}{seq:04}", seq = id_seq % 10_000);
        Some(Anomaly {
            id,
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
}
