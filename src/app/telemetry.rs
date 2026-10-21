//! Application observations persist independently of any engine or View.
use super::services::AppServices;
use tasty_memory::{MemoryValue, PutOpts, Scope};
use tasty_telemetry::{Anomaly, anomaly_key};
/// expires_at은 조회에서 제외할 뿐 디스크에서 지우지 않는다.
/// 물리 삭제는 부팅·실행 중 log_retention의 상한으로 별도 수행한다.
/// 기다리지 않는 writer 요청이다. 탐지는 이미 메모리에서 끝났고 기록 실패는 경고만 남긴다.
pub(crate) fn persist_anomaly(core: &AppServices, anomaly: &Anomaly) {
    let key = anomaly_key(anomaly.detected_at, &anomaly.id);
    let value = match serde_json::to_value(anomaly) {
        Ok(v) => MemoryValue::Json(v),
        Err(error) => {
            tracing::warn!(%error, "anomaly persist failed");
            return;
        }
    };
    let opts = PutOpts {
        expires_at: Some((anomaly.detected_at + crate::store::log_retention::LOG_TTL_MS) as i64),
        cas: None,
    };
    let bytes = key.len()
        + crate::adapters::ipc::handler::deferred::params_bytes(match &value {
            MemoryValue::Json(v) => v,
            _ => &serde_json::Value::Null,
        });
    core.memory_access().submit_detached(
        tasty_memory::writer::MemoryRequest::unit(bytes, move |s| {
            s.put(
                tasty_memory::HOST_OWNER,
                &Scope::Global,
                &key,
                &value,
                &opts,
            )
            .map(|_| ())
        }),
        "anomaly persist",
        None,
    );
    core.submit_log_prune_if_due(anomaly.detected_at);
}

impl AppServices {
    pub(crate) fn record_rss_sample(&self, agent: &str, bytes: u64, time: u64) -> Option<Anomaly> {
        let sequence = self.telemetry_seq.next();
        let anomaly = self
            .anomaly_detector
            .record_rss_sample(agent, bytes, time, sequence)?;
        persist_anomaly(self, &anomaly);
        Some(anomaly)
    }
}
