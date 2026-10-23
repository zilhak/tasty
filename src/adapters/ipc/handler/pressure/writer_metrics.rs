//! Generic writer measurements only; store-specific pressure fields belong to their owners.
use serde_json::json;

/// 저장 writer(memory.db·state.db)의 접수·묶음·예산 누계. writer가 없는 구성이면 null이다.
/// 원본 구조체를 .. 없이 분해해 필드가 추가되면 응답 변환도 검토하게 한다.
pub(super) fn writer_stats_json(
    stats: Option<tasty_utils::batch_writer::WriterStatsSnapshot>,
) -> serde_json::Value {
    let Some(tasty_utils::batch_writer::WriterStatsSnapshot {
        admitted_bytes,
        batch_bytes,
        batch_bytes_max,
        batch_wait,
        batch_execution,
        executor_execution,
        completed_ok,
        completed_rejected,
        completed_failed,
        completed_unknown,
        completed_lost,
        queued_items,
        queued_bytes,
        pending_items,
        pending_bytes,
        oldest_pending_age_us,
        admitted,
        rejected_saturated,
        rejected_closed,
        batches,
        batch_requests,
        batch_size_max,
        batch_sizes,
        superseded,
        retries,
        flushes,
        held_items,
        held_bytes,
        peak_items,
        peak_bytes,
    }) = stats
    else {
        return serde_json::Value::Null;
    };
    json!({
        "admitted_bytes": admitted_bytes, "batch_bytes": batch_bytes, "batch_bytes_max": batch_bytes_max,
        "batch_wait": duration_json(batch_wait),
        "batch_execution": duration_json(batch_execution),
        "executor_execution": duration_json(executor_execution),
        "completed_ok": completed_ok, "completed_rejected": completed_rejected,
        "completed_failed": completed_failed, "completed_unknown": completed_unknown, "completed_lost": completed_lost,
        "queued_items": queued_items, "queued_bytes": queued_bytes,
        "pending_items": pending_items, "pending_bytes": pending_bytes,
        "oldest_pending_age_us": oldest_pending_age_us,
        "admitted": admitted,
        "rejected_saturated": rejected_saturated,
        "rejected_closed": rejected_closed,
        "batches": batches,
        "batch_requests": batch_requests,
        "batch_size_max": batch_size_max,
        "batch_sizes": {
            "le": tasty_utils::batch_writer::BATCH_SIZE_BUCKETS,
            "counts": batch_sizes,
        },
        "superseded": superseded,
        "retries": retries,
        "flushes": flushes,
        "held_items": held_items,
        "held_bytes": held_bytes,
        "peak_items": peak_items,
        "peak_bytes": peak_bytes,
    })
}

fn duration_json(stats: tasty_utils::batch_writer::DurationStatsSnapshot) -> serde_json::Value {
    json!({ "samples": stats.samples, "sum_us": stats.sum_us, "max_us": stats.max_us,
        "le_us": tasty_utils::batch_writer::WRITER_LATENCY_BOUNDS_US, "counts": stats.counts })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_all_new_fields_and_empty_observations_without_fabricating_age() {
        assert_eq!(
            tasty_utils::batch_writer::WRITER_LATENCY_BOUNDS_US,
            *tasty_telemetry::HistogramSnapshot::bounds_us()
        );
        let mut stats = tasty_utils::batch_writer::WriterStatsSnapshot {
            queued_items: 2,
            queued_bytes: 20,
            admitted_bytes: 91,
            batch_bytes: 80,
            batch_bytes_max: 40,
            completed_ok: 1,
            completed_rejected: 2,
            completed_failed: 3,
            completed_unknown: 4,
            completed_lost: 5,
            pending_items: 6,
            pending_bytes: 77,
            oldest_pending_age_us: Some(125),
            ..Default::default()
        };
        stats.batch_wait.samples = 1;
        stats.batch_wait.sum_us = 10;
        stats.batch_wait.max_us = 10;
        stats.batch_wait.counts[0] = 1;
        let value = writer_stats_json(Some(stats));
        for (key, expected) in [
            ("queued_items", 2),
            ("queued_bytes", 20),
            ("admitted_bytes", 91),
            ("batch_bytes", 80),
            ("batch_bytes_max", 40),
            ("completed_ok", 1),
            ("completed_rejected", 2),
            ("completed_failed", 3),
            ("completed_unknown", 4),
            ("completed_lost", 5),
            ("pending_items", 6),
            ("pending_bytes", 77),
            ("oldest_pending_age_us", 125),
        ] {
            assert_eq!(value[key], expected);
        }
        assert_eq!(value["batch_wait"]["samples"], 1);
        assert_eq!(value["batch_wait"]["sum_us"], 10);
        assert_eq!(value["batch_wait"]["counts"][0], 1);
        assert_eq!(value["batch_execution"]["samples"], 0);
        assert_eq!(value["executor_execution"]["max_us"], 0);
        assert!(writer_stats_json(Some(Default::default()))["oldest_pending_age_us"].is_null());
        assert!(writer_stats_json(None).is_null());
    }
}
