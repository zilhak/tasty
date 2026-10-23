//! Common file helper operations observed only on the file writer, not generic writer batches.
use serde_json::{Value, json};
use tasty_utils::file_replace::{FILE_OPERATION_BOUNDS_US, OperationSnapshot};
fn operation(value: OperationSnapshot, payload: bool) -> Value {
    let samples = value.durations.iter().sum::<u64>();
    json!({
        "attempts": value.attempts,
        "succeeded": value.succeeded,
        "failed": value.failed,
        "unknown": value.unknown,
        "payload_bytes": payload.then_some(value.payload_bytes),
        "published_bytes": payload.then_some(value.published_bytes),
        "duration": {
            "samples": samples,
            "sum_us": (samples > 0).then_some(value.duration_us),
            "max_us": (samples > 0).then_some(value.max_duration_us),
            "bounds_us": FILE_OPERATION_BOUNDS_US,
            "counts": value.durations,
        },
        "errors": {
            "not_found": value.errors[0], "permission_denied": value.errors[1],
            "already_exists": value.errors[2], "invalid_data_or_input": value.errors[3],
            "other": value.errors[4],
        },
    })
}
pub(super) fn snapshot(counts: Option<crate::app::file_writer::FileWriteCounts>) -> Value {
    counts.map_or(Value::Null, |value| {
        json!({
            "replace": operation(value.operations.replace, true),
            "delete": operation(value.operations.delete, false),
            "executed_commands": {
                "known_success": value.commands.known_success,
                "known_failure": value.commands.known_failure,
                "unknown": value.commands.unknown,
                "unclassified": value.commands.unclassified,
                "partial": value.commands.partial,
            },
        })
    })
}
