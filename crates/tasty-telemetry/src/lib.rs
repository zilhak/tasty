//! 메트릭 이벤트·비용 제한·이상 신호의 타입과 검증·집계 함수.
//!
//! 조회는 보존된 raw event를 집계한다. MetricBucket은 조회 결과이며 따로 저장하거나
//! 주기적으로 갱신하지 않는다. 따라서 오래된 이벤트를 삭제하면 해당 구간은 조회할 수 없다.
//! 보존 정책은 docs/design/systems/storage.md#관측-로그-보존 참조.
//!
//! 파일 저장·보존 상한·호출자 식별·dispatcher 연결·cap 평가 캐시는 호스트가 맡는다.

// 이유: 테스트의 반환값 무시는 허용하되 제품 코드의 반환값 무시는 계속 검사한다.
#![cfg_attr(test, allow(clippy::let_underscore_must_use))]

pub mod agent_id;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use agent_id::AgentId;

#[derive(Debug, Error)]
pub enum TelemetryError {
    #[error("invalid metric name '{0}': must match [a-z][a-z0-9_]* (1..=64)")]
    InvalidMetric(String),
    #[error("invalid agent id '{0}': must match [a-zA-Z0-9_-]+ (1..=64)")]
    InvalidAgentId(String),
    #[error("invalid op")]
    InvalidOp,
    #[error("invalid window")]
    InvalidWindow,
    #[error("internal: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, TelemetryError>;

/// 누적 연산.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// summary와 bucket의 sum·last를 이 값으로 바꾼다. count도 증가한다.
    Set,
    /// sum에 더하고 last를 갱신한다. count도 증가한다.
    Inc,
    /// 값을 뺌 (음수 inc).
    Dec,
}

impl std::str::FromStr for Op {
    type Err = TelemetryError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "set" => Ok(Op::Set),
            "inc" => Ok(Op::Inc),
            "dec" => Ok(Op::Dec),
            _ => Err(TelemetryError::InvalidOp),
        }
    }
}

impl Op {
    /// Dec는 부호를 반대로 하고 Inc와 Set은 입력값을 그대로 반환한다.
    pub fn signed(self, v: f64) -> f64 {
        match self {
            Op::Inc | Op::Set => v,
            Op::Dec => -v,
        }
    }
}

/// 단일 메트릭 이벤트. 호스트가 `tasty.telemetry.event.{ts:013}.{seq:04}` 키로 저장.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TelemetryEvent {
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub workspace_id: Option<u32>,
    pub metric: String,
    pub value: f64,
    pub op: Op,
    /// unix ms.
    pub ts: u64,
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub tags: BTreeMap<String, String>,
}

impl TelemetryEvent {
    /// 검증 + 정상 생성. 잘못된 값은 [`TelemetryError`] 반환.
    pub fn new(
        agent: impl Into<String>,
        metric: impl Into<String>,
        value: f64,
        op: Op,
        ts: u64,
    ) -> Result<Self> {
        let agent = agent.into();
        let metric = metric.into();
        validate_agent_id(&agent)?;
        validate_metric(&metric)?;
        Ok(Self {
            agent,
            workspace_id: None,
            metric,
            value,
            op,
            ts,
            tags: BTreeMap::new(),
        })
    }

    pub fn with_workspace(mut self, ws: u32) -> Self {
        self.workspace_id = Some(ws);
        self
    }

    pub fn with_tag(mut self, k: impl Into<String>, v: impl Into<String>) -> Self {
        self.tags.insert(k.into(), v.into());
        self
    }
}

/// 시계열 버킷. 1m / 1h / 1d 단위 집계 결과.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricBucket {
    pub metric: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub workspace_id: Option<u32>,
    /// 윈도우 시작 unix ms.
    pub window_start: u64,
    /// 윈도우 크기 ms (60_000 / 3_600_000 / 86_400_000).
    pub window_size_ms: u64,
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    /// 마지막 이벤트의 부호를 적용한 값. 누적 합과는 별개다.
    pub last: f64,
}

/// 윈도우 크기.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Window {
    #[serde(rename = "1m")]
    OneMinute,
    #[serde(rename = "1h")]
    OneHour,
    #[serde(rename = "1d")]
    OneDay,
}

impl std::str::FromStr for Window {
    type Err = TelemetryError;

    fn from_str(s: &str) -> Result<Self> {
        match s {
            "1m" => Ok(Window::OneMinute),
            "1h" => Ok(Window::OneHour),
            "1d" => Ok(Window::OneDay),
            _ => Err(TelemetryError::InvalidWindow),
        }
    }
}

impl Window {
    pub fn size_ms(self) -> u64 {
        match self {
            Window::OneMinute => 60_000,
            Window::OneHour => 3_600_000,
            Window::OneDay => 86_400_000,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Window::OneMinute => "1m",
            Window::OneHour => "1h",
            Window::OneDay => "1d",
        }
    }

    /// `ts` 가 속한 윈도우의 시작 시각.
    pub fn align(self, ts: u64) -> u64 {
        let size = self.size_ms();
        (ts / size) * size
    }
}

pub fn validate_metric(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 64 {
        return Err(TelemetryError::InvalidMetric(s.into()));
    }
    let mut chars = s.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_lowercase() {
        return Err(TelemetryError::InvalidMetric(s.into()));
    }
    for c in chars {
        if !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_') {
            return Err(TelemetryError::InvalidMetric(s.into()));
        }
    }
    Ok(())
}

pub fn validate_agent_id(s: &str) -> Result<()> {
    if s.is_empty() || s.len() > 64 {
        return Err(TelemetryError::InvalidAgentId(s.into()));
    }
    for c in s.chars() {
        if !(c.is_ascii_alphanumeric() || c == '_' || c == '-') {
            return Err(TelemetryError::InvalidAgentId(s.into()));
        }
    }
    Ok(())
}

/// `tasty.telemetry.event.{ts:013}.{seq:04}`.
pub fn event_key(ts: u64, seq: u64) -> String {
    format!(
        "tasty.telemetry.event.{ts:013}.{seq:04}",
        ts = ts,
        seq = seq % 10_000
    )
}

/// 모든 raw event 키의 접두사. 집계 버킷은 영속하지 않는다.
pub const EVENT_KEY_PREFIX: &str = "tasty.telemetry.event.";

/// `tasty.telemetry.cap.` — 모든 cap 키 prefix.
#[derive(Debug, Default)]
pub struct TelemetrySeq {
    counter: AtomicU64,
}

impl TelemetrySeq {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn next(&self) -> u64 {
        self.counter.fetch_add(1, Ordering::Relaxed)
    }
}

/// 이벤트 목록을 (metric, agent) 별 단일 버킷으로 집계.
mod aggregate;
mod anomaly;
mod cap;
mod gate;
pub mod pressure;
pub mod slow_requests;

pub use aggregate::*;
pub use anomaly::*;
pub use cap::*;
pub use gate::{GateRefusal, GateSnapshot, GateStats};
// 히스토그램 경계는 HistogramSnapshot::bounds_us로 값과 함께 제공한다.
// 내부 필드 타입과 경계 상수는 pressure 모듈에서만 노출한다.
pub use pressure::{
    ConnectionSnapshot, ConnectionStats, HistogramSnapshot, LATENCY_BUCKET_COUNT,
    PluginWaitSnapshot, PluginWaitStats, PressureSnapshot, PressureStats,
};
// 개별 레코드 타입은 slow_requests 모듈에서 제공한다.
pub use slow_requests::SlowRequestLog;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
