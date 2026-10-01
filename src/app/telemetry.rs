//! Application observations persist independently of any engine or View.
use super::services::AppServices;
use tasty_memory::{MemoryValue, PutOpts, Scope};
use tasty_telemetry::{Anomaly, anomaly_key};
/// expires_at은 조회에서 제외할 뿐 디스크에서 지우지 않는다.
/// 물리 삭제는 부팅·실행 중 log_retention의 상한으로 별도 수행한다.
pub(crate) fn persist_anomaly(
    core: &AppServices,
    anomaly: &Anomaly,
) -> std::result::Result<(), String> {
    let key = anomaly_key(anomaly.detected_at, &anomaly.id);
    let value = MemoryValue::Json(serde_json::to_value(anomaly).map_err(|e| e.to_string())?);
    let opts = PutOpts {
        expires_at: Some((anomaly.detected_at + crate::store::log_retention::LOG_TTL_MS) as i64),
        cas: None,
    };
    core.with_memory(|s| {
        crate::store::log_retention::maybe_prune(s, anomaly.detected_at);
        s.put(
            tasty_memory::HOST_OWNER,
            &Scope::Global,
            &key,
            &value,
            &opts,
        )
    })
    .map(|_| ())
    .map_err(|e| format!("memory put failed: {e}"))
}

impl AppServices {
    pub(crate) fn record_rss_sample(&self, agent: &str, bytes: u64, time: u64) -> Option<Anomaly> {
        let sequence = self.telemetry_seq.next();
        let anomaly = self
            .anomaly_detector
            .record_rss_sample(agent, bytes, time, sequence)?;
        if let Err(error) = persist_anomaly(self, &anomaly) {
            tracing::warn!(%error,"anomaly persist failed");
        }
        Some(anomaly)
    }
}
