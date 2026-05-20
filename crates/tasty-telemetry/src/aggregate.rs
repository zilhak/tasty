//! `tasty-telemetry` aggregation — events → buckets, summary, top.

use serde::{Deserialize, Serialize};

use super::{MetricBucket, Op, TelemetryEvent, Window};

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
pub fn aggregate_into_buckets(events: Vec<TelemetryEvent>, window: Window) -> Vec<MetricBucket> {
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
    out.sort_by(|a, b| {
        b.sum
            .partial_cmp(&a.sum)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(limit);
    out
}

// ============================================================
// Tests
// ============================================================

#[cfg(test)]
#[path = "lib_tests.rs"]
mod tests;
