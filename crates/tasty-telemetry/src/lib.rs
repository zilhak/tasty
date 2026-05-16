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
//! - **영속 IO**: 호스트가 `tasty_memory::with_store` 로 read/write
//! - **rollup task**: 호스트가 tokio interval 로 주기 호출
//! - **dispatcher 통합 / cap 평가 캐시**: 호스트 `src/ipc/handler` 측
//! - **agent 식별**: 호스트가 [`tasty_core::AgentId::from_caller`] 등으로 도출
//!
//! ## 4.1 범위
//!
//! - record / record_batch (단일 이벤트 직렬화)
//! - 조회: events → bucket aggregation, summary, top
//! - 롤업 task / cap / anomaly 는 후속 sub-phase
//!
//! 자세한 스키마는 `.claude-workspace/plans/ai-first-depth/04-observability.md`.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use thiserror::Error;

pub use tasty_core::AgentId;

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

impl Op {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "set" => Ok(Op::Set),
            "inc" => Ok(Op::Inc),
            "dec" => Ok(Op::Dec),
            _ => Err(TelemetryError::InvalidOp),
        }
    }

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

impl Window {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "1m" => Ok(Window::OneMinute),
            "1h" => Ok(Window::OneHour),
            "1d" => Ok(Window::OneDay),
            _ => Err(TelemetryError::InvalidWindow),
        }
    }

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
    format!("tasty.telemetry.event.{ts:013}.{seq:04}", ts = ts, seq = seq % 10_000)
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
pub const CAP_KEY_PREFIX: &str = "tasty.telemetry.cap.";

/// `tasty.telemetry.cap.{id}`.
pub fn cap_key(id: &str) -> String {
    format!("{CAP_KEY_PREFIX}{id}")
}

// ============================================================
// Cost cap — Phase 4.3
// ============================================================

/// Cap 평가 기간. `Total` 은 전 기간 누적 (보존 기간이 retention 정책 안에 있을 때 유효).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CapWindow {
    Total,
    #[serde(rename = "1h")]
    Hour,
    #[serde(rename = "1d")]
    Day,
}

impl CapWindow {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "total" => Ok(CapWindow::Total),
            "1h" => Ok(CapWindow::Hour),
            "1d" => Ok(CapWindow::Day),
            _ => Err(TelemetryError::InvalidWindow),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CapWindow::Total => "total",
            CapWindow::Hour => "1h",
            CapWindow::Day => "1d",
        }
    }

    /// 윈도우의 시간 범위 ms. `Total` 은 `None`.
    pub fn span_ms(self) -> Option<u64> {
        match self {
            CapWindow::Total => None,
            CapWindow::Hour => Some(3_600_000),
            CapWindow::Day => Some(86_400_000),
        }
    }
}

/// 임계 초과 시 호스트가 취할 동작.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CapAction {
    /// IPC 거부 + (호스트 측에서) agent 강제 종료 트리거.
    Stop,
    /// IPC 거부만. `reset` 까지 영구.
    Pause,
    /// Approval 요청을 자동 발행하고 응답 따라 통과/거부.
    RequireApproval,
    /// 차단 없음. notification 만 발행.
    Notify,
}

impl CapAction {
    pub fn from_str(s: &str) -> Result<Self> {
        match s {
            "stop" => Ok(CapAction::Stop),
            "pause" => Ok(CapAction::Pause),
            "require_approval" => Ok(CapAction::RequireApproval),
            "notify" => Ok(CapAction::Notify),
            _ => Err(TelemetryError::Internal(format!("invalid action '{s}'"))),
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CapAction::Stop => "stop",
            CapAction::Pause => "pause",
            CapAction::RequireApproval => "require_approval",
            CapAction::Notify => "notify",
        }
    }
}

/// Cap 발동 흔적.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CapTriggered {
    /// unix ms.
    pub at: u64,
    /// 발동 당시의 측정 값.
    pub value: f64,
}

/// Cost cap — 메트릭 누적값에 대한 임계와 동작.
///
/// `triggered` 가 `Some` 이면 이미 발동된 상태. 호스트가 cap 평가 시 이 상태를
/// 그대로 적용해 IPC 를 거부/차단한다. `reset` 호출로 `None` 로 되돌릴 수 있다.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CostCap {
    pub id: String,
    pub agent: String,
    pub metric: String,
    pub threshold: f64,
    pub window: CapWindow,
    pub action: CapAction,
    /// unix ms.
    pub created_at: u64,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub triggered: Option<CapTriggered>,
}

impl CostCap {
    pub fn is_triggered(&self) -> bool {
        self.triggered.is_some()
    }
}

// ============================================================
// Seq generator
// ============================================================

/// 같은 ms 안에서 단조 증가하는 시퀀스. host singleton (`Arc<TelemetrySeq>`) 으로 공유.
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
pub fn fold_events_into_bucket(
    metric: &str,
    agent: &str,
    workspace_id: Option<u32>,
    window: Window,
    window_start: u64,
    events: impl IntoIterator<Item = TelemetryEvent>,
) -> MetricBucket {
    let mut bucket = MetricBucket {
        metric: metric.to_string(),
        agent: agent.to_string(),
        workspace_id,
        window_start,
        window_size_ms: window.size_ms(),
        count: 0,
        sum: 0.0,
        min: f64::INFINITY,
        max: f64::NEG_INFINITY,
        last: 0.0,
    };
    for ev in events {
        let signed = ev.op.signed(ev.value);
        bucket.count += 1;
        match ev.op {
            Op::Set => {
                bucket.sum = signed;
                bucket.last = signed;
            }
            Op::Inc | Op::Dec => {
                bucket.sum += signed;
                bucket.last = signed;
            }
        }
        if ev.value < bucket.min {
            bucket.min = ev.value;
        }
        if ev.value > bucket.max {
            bucket.max = ev.value;
        }
    }
    if bucket.count == 0 {
        bucket.min = 0.0;
        bucket.max = 0.0;
    }
    bucket
}

/// 이벤트 목록을 (metric, agent, window_start) 그룹으로 모아 윈도우별 버킷 리스트로.
pub fn aggregate_into_buckets(
    events: Vec<TelemetryEvent>,
    window: Window,
) -> Vec<MetricBucket> {
    use std::collections::HashMap;
    let mut grouped: HashMap<(String, String, u64, Option<u32>), Vec<TelemetryEvent>> =
        HashMap::new();
    for ev in events {
        let ws_start = window.align(ev.ts);
        let key = (
            ev.metric.clone(),
            ev.agent.clone(),
            ws_start,
            ev.workspace_id,
        );
        grouped.entry(key).or_default().push(ev);
    }
    let mut out: Vec<_> = grouped
        .into_iter()
        .map(|((metric, agent, ws_start, ws_id), evs)| {
            fold_events_into_bucket(&metric, &agent, ws_id, window, ws_start, evs)
        })
        .collect();
    out.sort_by(|a, b| {
        a.metric
            .cmp(&b.metric)
            .then_with(|| a.agent.cmp(&b.agent))
            .then_with(|| a.window_start.cmp(&b.window_start))
    });
    out
}

/// 메트릭 단일값 요약. summary IPC 응답용.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MetricSummary {
    pub metric: String,
    pub agent: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub workspace_id: Option<u32>,
    pub count: u64,
    pub sum: f64,
    pub min: f64,
    pub max: f64,
    pub last: f64,
}

/// 모든 이벤트를 (metric, agent) 별로 집계 (윈도우 없음).
pub fn summarize_events(events: Vec<TelemetryEvent>) -> Vec<MetricSummary> {
    use std::collections::HashMap;
    let mut grouped: HashMap<(String, String, Option<u32>), MetricSummary> = HashMap::new();
    for ev in events {
        let key = (ev.metric.clone(), ev.agent.clone(), ev.workspace_id);
        let entry = grouped.entry(key).or_insert(MetricSummary {
            metric: ev.metric.clone(),
            agent: ev.agent.clone(),
            workspace_id: ev.workspace_id,
            count: 0,
            sum: 0.0,
            min: f64::INFINITY,
            max: f64::NEG_INFINITY,
            last: 0.0,
        });
        let signed = ev.op.signed(ev.value);
        entry.count += 1;
        match ev.op {
            Op::Set => {
                entry.sum = signed;
                entry.last = signed;
            }
            Op::Inc | Op::Dec => {
                entry.sum += signed;
                entry.last = signed;
            }
        }
        if ev.value < entry.min {
            entry.min = ev.value;
        }
        if ev.value > entry.max {
            entry.max = ev.value;
        }
    }
    let mut out: Vec<_> = grouped
        .into_values()
        .map(|mut s| {
            if s.count == 0 {
                s.min = 0.0;
                s.max = 0.0;
            }
            s
        })
        .collect();
    out.sort_by(|a, b| a.metric.cmp(&b.metric).then_with(|| a.agent.cmp(&b.agent)));
    out
}

/// top 엔트리. agent 또는 workspace 기준 집계.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TopEntry {
    pub key: String,
    pub sum: f64,
    pub count: u64,
}

/// `by="agent"` 또는 `by="workspace"` 로 sum 내림차순 정렬.
pub fn top_n(events: Vec<TelemetryEvent>, by: &str, limit: usize) -> Vec<TopEntry> {
    use std::collections::HashMap;
    let mut buckets: HashMap<String, (f64, u64)> = HashMap::new();
    for ev in events {
        let key = match by {
            "agent" => ev.agent.clone(),
            "workspace" => ev
                .workspace_id
                .map(|w| w.to_string())
                .unwrap_or_else(|| "_none".into()),
            _ => ev.agent.clone(),
        };
        let entry = buckets.entry(key).or_insert((0.0, 0));
        entry.0 += ev.op.signed(ev.value);
        entry.1 += 1;
    }
    let mut out: Vec<TopEntry> = buckets
        .into_iter()
        .map(|(key, (sum, count))| TopEntry { key, sum, count })
        .collect();
    out.sort_by(|a, b| b.sum.partial_cmp(&a.sum).unwrap_or(std::cmp::Ordering::Equal));
    out.truncate(limit);
    out
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_validation() {
        assert!(validate_metric("input_tokens").is_ok());
        assert!(validate_metric("a").is_ok());
        assert!(validate_metric("a1_b").is_ok());
        assert!(validate_metric("").is_err());
        assert!(validate_metric("Input_tokens").is_err()); // 대문자
        assert!(validate_metric("1abc").is_err()); // 숫자 시작
        assert!(validate_metric("ab.cd").is_err()); // dot 금지
        assert!(validate_metric(&"x".repeat(65)).is_err()); // 길이
    }

    #[test]
    fn agent_validation() {
        assert!(validate_agent_id("_host").is_ok());
        assert!(validate_agent_id("claude_s42").is_ok());
        assert!(validate_agent_id("Codex-1").is_ok());
        assert!(validate_agent_id("").is_err());
        assert!(validate_agent_id("a b").is_err());
        assert!(validate_agent_id("a.b").is_err());
    }

    #[test]
    fn event_key_format() {
        let k = event_key(1_700_000_000_000, 5);
        assert_eq!(k, "tasty.telemetry.event.1700000000000.0005");
        // ts 가 13자 zero-pad 인지
        assert!(k.starts_with("tasty.telemetry.event."));
        assert_eq!(k.len(), "tasty.telemetry.event.1700000000000.0005".len());
    }

    #[test]
    fn bucket_key_format() {
        let k = bucket_key(
            Window::OneMinute,
            "input_tokens",
            "claude_s1",
            1_700_000_000_000,
        );
        assert_eq!(
            k,
            "tasty.telemetry.bucket.1m.input_tokens.claude_s1.1700000000000"
        );
    }

    #[test]
    fn op_signed() {
        assert_eq!(Op::Inc.signed(5.0), 5.0);
        assert_eq!(Op::Dec.signed(5.0), -5.0);
        assert_eq!(Op::Set.signed(5.0), 5.0);
    }

    #[test]
    fn window_align() {
        assert_eq!(Window::OneMinute.align(123_456), 120_000);
        assert_eq!(Window::OneHour.align(3_700_000), 3_600_000);
        assert_eq!(Window::OneDay.align(86_400_001), 86_400_000);
    }

    #[test]
    fn summarize_basic() {
        let evs = vec![
            TelemetryEvent::new("a", "input_tokens", 100.0, Op::Inc, 1000).unwrap(),
            TelemetryEvent::new("a", "input_tokens", 50.0, Op::Inc, 2000).unwrap(),
            TelemetryEvent::new("a", "input_tokens", 200.0, Op::Set, 3000).unwrap(),
            TelemetryEvent::new("b", "input_tokens", 30.0, Op::Inc, 1500).unwrap(),
        ];
        let s = summarize_events(evs);
        assert_eq!(s.len(), 2);
        let a = s.iter().find(|x| x.agent == "a").unwrap();
        assert_eq!(a.count, 3);
        // sum: Set은 sum을 통째 교체 → 마지막이 Set(200)이라 200
        assert_eq!(a.sum, 200.0);
        assert_eq!(a.last, 200.0);
        assert_eq!(a.max, 200.0);
        assert_eq!(a.min, 50.0);
    }

    #[test]
    fn summarize_inc_only() {
        let evs = vec![
            TelemetryEvent::new("a", "files_read", 1.0, Op::Inc, 1000).unwrap(),
            TelemetryEvent::new("a", "files_read", 1.0, Op::Inc, 1001).unwrap(),
            TelemetryEvent::new("a", "files_read", 1.0, Op::Inc, 1002).unwrap(),
        ];
        let s = summarize_events(evs);
        assert_eq!(s.len(), 1);
        assert_eq!(s[0].sum, 3.0);
        assert_eq!(s[0].count, 3);
    }

    #[test]
    fn aggregate_buckets_1m() {
        let evs = vec![
            // 1분 윈도우 0~60s
            TelemetryEvent::new("a", "m", 1.0, Op::Inc, 10_000).unwrap(),
            TelemetryEvent::new("a", "m", 2.0, Op::Inc, 30_000).unwrap(),
            // 다른 분 윈도우 60~120s
            TelemetryEvent::new("a", "m", 5.0, Op::Inc, 70_000).unwrap(),
        ];
        let buckets = aggregate_into_buckets(evs, Window::OneMinute);
        assert_eq!(buckets.len(), 2);
        assert_eq!(buckets[0].window_start, 0);
        assert_eq!(buckets[0].sum, 3.0);
        assert_eq!(buckets[0].count, 2);
        assert_eq!(buckets[1].window_start, 60_000);
        assert_eq!(buckets[1].sum, 5.0);
    }

    #[test]
    fn top_by_agent() {
        let evs = vec![
            TelemetryEvent::new("a", "m", 1000.0, Op::Inc, 1).unwrap(),
            TelemetryEvent::new("b", "m", 500.0, Op::Inc, 1).unwrap(),
            TelemetryEvent::new("a", "m", 100.0, Op::Inc, 2).unwrap(),
        ];
        let t = top_n(evs, "agent", 5);
        assert_eq!(t.len(), 2);
        assert_eq!(t[0].key, "a");
        assert_eq!(t[0].sum, 1100.0);
        assert_eq!(t[1].key, "b");
    }

    #[test]
    fn op_from_str() {
        assert!(matches!(Op::from_str("set"), Ok(Op::Set)));
        assert!(matches!(Op::from_str("inc"), Ok(Op::Inc)));
        assert!(matches!(Op::from_str("dec"), Ok(Op::Dec)));
        assert!(Op::from_str("nope").is_err());
    }

    #[test]
    fn window_from_str() {
        assert!(matches!(Window::from_str("1m"), Ok(Window::OneMinute)));
        assert!(matches!(Window::from_str("1h"), Ok(Window::OneHour)));
        assert!(matches!(Window::from_str("1d"), Ok(Window::OneDay)));
        assert!(Window::from_str("2m").is_err());
    }

    #[test]
    fn cap_window_parse() {
        assert!(matches!(CapWindow::from_str("total"), Ok(CapWindow::Total)));
        assert!(matches!(CapWindow::from_str("1h"), Ok(CapWindow::Hour)));
        assert!(matches!(CapWindow::from_str("1d"), Ok(CapWindow::Day)));
        assert!(CapWindow::from_str("1m").is_err());
        assert_eq!(CapWindow::Total.span_ms(), None);
        assert_eq!(CapWindow::Hour.span_ms(), Some(3_600_000));
    }

    #[test]
    fn cap_action_parse() {
        assert!(matches!(CapAction::from_str("stop"), Ok(CapAction::Stop)));
        assert!(matches!(CapAction::from_str("pause"), Ok(CapAction::Pause)));
        assert!(matches!(
            CapAction::from_str("require_approval"),
            Ok(CapAction::RequireApproval)
        ));
        assert!(matches!(CapAction::from_str("notify"), Ok(CapAction::Notify)));
        assert!(CapAction::from_str("bogus").is_err());
    }

    #[test]
    fn cap_key_format() {
        assert_eq!(cap_key("cap_abc"), "tasty.telemetry.cap.cap_abc");
    }

    #[test]
    fn telemetry_seq_monotonic() {
        let s = TelemetrySeq::new();
        assert_eq!(s.next(), 0);
        assert_eq!(s.next(), 1);
        assert_eq!(s.next(), 2);
    }
}
