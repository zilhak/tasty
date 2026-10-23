//! Journal-only serializer. The process pressure router owns supplying its cached handle.
use crate::runtime::journal_product::{JournalMetricsSnapshot, observability::WorkCounts};
use serde_json::{Value, json};
use tasty_event_store::{JOURNAL_LATENCY_BOUNDS_US, JournalDurationSnapshot};

// reason: the journal worker cannot wire AppServices or the process router it does not own.
#[allow(
    dead_code,
    reason = "independent serializer integration boundary for process diagnostics"
)]
pub(crate) fn snapshot_json(snapshot: Option<&JournalMetricsSnapshot>) -> Value {
    let Some(s) = snapshot else {
        return Value::Null;
    };
    let WorkCounts {
        admitted,
        admitted_bytes_estimate,
        rejected_busy,
        rejected_too_large,
        rejected_stopped,
        handled_ok,
        handled_error,
        deferred,
        lost,
        queued_items,
        queued_bytes_estimate,
        inflight_items,
        inflight_bytes_estimate,
        pending_items,
        pending_bytes_estimate,
    } = s.work;
    let mut value = json!({
        "scope": "transport_work_steps", "bytes_kind": "request_input_budget_estimate",
        "admitted": admitted, "admitted_bytes_estimate": admitted_bytes_estimate,
        "rejected_busy": rejected_busy, "rejected_too_large": rejected_too_large,
        "rejected_stopped": rejected_stopped, "handled_ok": handled_ok, "handled_error": handled_error,
        "deferred": deferred, "lost": lost, "queued_items": queued_items, "queued_bytes_estimate": queued_bytes_estimate,
        "inflight_items": inflight_items, "inflight_bytes_estimate": inflight_bytes_estimate,
        "pending_items": pending_items, "pending_bytes_estimate": pending_bytes_estimate,
    });
    value
        .as_object_mut()
        .expect("object")
        .extend(details(s).as_object().expect("object").clone());
    value
}
fn details(s: &JournalMetricsSnapshot) -> Value {
    let store = s.store.map(|s| json!({
        "physical_commits": s.commits.physical_commits, "structural_commits": s.commits.structural_commits,
        "logical_requests": s.commits.logical_requests, "failed_commits": s.commits.failed_commits,
        "commit_ns": s.commits.commit_ns, "max_commit_ns": s.commits.max_commit_ns,
        "grouped_commits": s.grouped_commits, "grouped_logical_requests": s.grouped_logical_requests,
        "commit_success": duration(s.commit_success), "commit_failure": duration(s.commit_failure),
    }));
    json!({
        "oldest_pending_age_us": s.oldest_pending_age_us, "dirty_age_us": Value::Null,
        "publication_known": s.publication_known, "unpublished_age_us": s.unpublished_age_us, "open_admissions": s.open_admissions,
        "waiting_admissions": s.waiting_admissions, "admission_bytes_estimate": s.admission_bytes_estimate,
        "batches": s.batches, "batch_requests": s.batch_requests, "batch_size_max": s.batch_size_max,
        "publication_errors": s.publication_errors, "batch_errors": s.batch_errors, "batch_bytes_estimate": s.batch_bytes_estimate,
        "batch_sizes": {"le": [1,2,4,8,16,32,64], "counts": s.batch_sizes}, "group_revision_retries": s.group_revision_retries,
        "scalar_revision_retries": s.scalar_revision_retries, "sqlite_busy_retries": Value::Null, "observation_overflow": s.observation_overflow,
        "queue_wait": duration(s.queue_wait), "execution": duration(s.execution), "completion": duration(s.completion),
        "batch_wait": duration(s.batch_wait), "batch_execution": duration(s.batch_execution), "store": store,
    })
}

fn duration(s: JournalDurationSnapshot) -> Value {
    json!({"samples": s.samples, "sum_us": s.sum_us, "max_us": s.max_us,
        "mean_us": (s.samples > 0).then(|| s.sum_us / s.samples),
        "bounds_us": JOURNAL_LATENCY_BOUNDS_US, "counts": s.counts})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn missing_and_unobserved_are_distinct_and_snapshot_is_nonconsuming() {
        assert!(snapshot_json(None).is_null());
        let metrics = crate::runtime::journal_product::JournalMetrics::default();
        let s = metrics.snapshot();
        let v = snapshot_json(Some(&s));
        assert!(v["store"].is_null());
        assert!(v["dirty_age_us"].is_null());
        assert!(v["queue_wait"]["mean_us"].is_null());
        assert!(v["oldest_pending_age_us"].is_null());
        assert_eq!(v["queue_wait"]["counts"].as_array().unwrap().len(), 12);
        assert_eq!(v, snapshot_json(Some(&metrics.snapshot())));
        assert!(v.get("rows").is_none());
    }
}
