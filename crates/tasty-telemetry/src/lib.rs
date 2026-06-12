//! Agent telemetry — 메트릭 기록·롤업·조회 + cap·anomaly (Phase 4).
//!
//! ## 책임 범위
//!
//! - 도메인 타입 ([`TelemetryEvent`], [`MetricBucket`], [`Op`])
//! - key 컨벤션 (`event_key`, `bucket_key_1m/1h/1d`, `cap_key`, `anomaly_key`)
//! - 식별자 검증 (metric / agent_id 형식)
//! - pure aggregation: events → buckets, summary, top
//!
//! ## 비-책임 (호스트가 처리)
//!
//! - **영속 IO**: 호스트가 `Core.with_memory` 로 read/write
//! - **rollup task**: 호스트가 tokio interval 로 주기 호출
//! - **dispatcher 통합 / cap 평가 캐시**: 호스트 `src/ipc/handler` 측
//! - **agent 식별**: 호스트가 [`AgentId::from_caller`] 등으로 도출
//!
//! ## 4.1 범위
//!
//! - record / record_batch (단일 이벤트 직렬화)
//! - 조회: events → bucket aggregation, summary, top
//! - 롤업 task / cap / anomaly 는 후속 sub-phase
//!
//! 자세한 스키마는 `.claude-workspace/plans/ai-first-depth/04-observability.md`.

pub mod agent_id;

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use agent_id::AgentId;

// ============================================================
// Error
// ============================================================

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

// ============================================================
// Domain types
// ============================================================

/// 누적 연산.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Op {
    /// 값을 그대로 set. summary 의 `last` 만 갱신.
    Set,
    /// 값을 더함. summary 의 `sum/count` 누적.
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
    /// 부호 조정된 effective value (inc 양수, dec 음수, set 그대로).
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
    /// 마지막 set/inc/dec 후의 effective signed 값 (Set 은 value 그대로).
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

// ============================================================
// Validation
// ============================================================

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

// ============================================================
// Key conventions
// ============================================================

/// `tasty.telemetry.event.{ts:013}.{seq:04}`.
pub fn event_key(ts: u64, seq: u64) -> String {
    format!(
        "tasty.telemetry.event.{ts:013}.{seq:04}",
        ts = ts,
        seq = seq % 10_000
    )
}

/// `tasty.telemetry.bucket.{window}.{metric}.{agent}.{window_start:013}`.
///
/// `metric` / `agent` 는 식별자 검증 통과한 상태라야 안전 (점 구분 의존).
pub fn bucket_key(window: Window, metric: &str, agent: &str, window_start: u64) -> String {
    format!(
        "tasty.telemetry.bucket.{w}.{m}.{a}.{ws:013}",
        w = window.as_str(),
        m = metric,
        a = agent,
        ws = window_start
    )
}

/// `tasty.telemetry.event.` — 전체 event 키 prefix.
pub const EVENT_KEY_PREFIX: &str = "tasty.telemetry.event.";

/// `tasty.telemetry.bucket.{window}.` — 윈도우별 bucket prefix.
pub fn bucket_prefix(window: Window) -> String {
    format!("tasty.telemetry.bucket.{}.", window.as_str())
}

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

// ============================================================
// Aggregation — pure functions over event lists
// ============================================================

/// 이벤트 목록을 (metric, agent) 별 단일 버킷으로 집계.
mod aggregate;
mod anomaly;
mod cap;

pub use aggregate::*;
pub use anomaly::*;
pub use cap::*;

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
