//! Opt-in measurements of the real immutable capture/worker path, with detached terminals only.
mod fixture;
use super::*;
use crate::core::layout_persistence::import::surface_data::SurfaceData;
use crate::runtime::surface_capture::{self, RestoreLookup};
use fixture::*;
use serde_json::json;
use sha2::{Digest, Sha256};
use std::time::Instant;

/// This is a measurement/control, not a latency threshold or a proposed storage implementation.
#[test]
#[ignore = "explicit capture cost measurement; set TASTY_CAPTURE_COST_OUTPUT and private TMPDIR"]
#[cfg(target_os = "linux")]
fn measure_full_immutable_capture_between_maintenance_cuts() {
    let output = std::env::var_os("TASTY_CAPTURE_COST_OUTPUT").expect("explicit output path");
    let mut results = Vec::new();
    for (name, rows, width, append) in [
        ("unchanged", 1_000, 80, 0),
        ("small_append", 1_000, 80, 1),
        ("large_unchanged", 50_000, 160, 0),
        ("large_scrollback", 50_000, 160, 1),
    ] {
        for repetition in 0..5 {
            results.push(measure_case(name, rows, width, append, repetition));
            std::fs::write(&output, serde_json::to_vec_pretty(&results).unwrap()).unwrap();
        }
    }
    assert_eq!(results.len(), 20);
}

fn save(
    worker: &JournalWorker,
    session: &crate::runtime::engine_session::EngineSession,
    binding: &EngineBinding,
    ticket: u64,
    home: &std::path::Path,
    tid: u32,
) -> serde_json::Value {
    let io_before = thread_io(tid);
    let db_before = database(home);
    let metrics = worker.metrics_snapshot();
    let start = Instant::now();
    let surfaces = surface_capture::capture(session).unwrap();
    let capture_us = elapsed(start);
    let payload_bytes: usize = surfaces.iter().map(|surface| surface.bytes.len()).sum();
    let work = Work::Capture {
        binding: binding.clone(),
        surfaces,
        restore: RestoreLookup::of(session),
        drop_uncaptured_scrollback: false,
    };
    let queued_bytes = request_size(&work);
    assert!(queued_bytes < MAX_QUEUED_BYTES);
    let start = Instant::now();
    submit(worker, ticket, work);
    let mut ack_us = 0;
    loop {
        match receive(worker) {
            Completion::Publish { batch, .. } => {
                let now = Instant::now();
                worker.acknowledge(batch.batch_id, Ok(())).unwrap();
                ack_us += elapsed(now);
            }
            Completion::Finished {
                ticket: got,
                result,
            } => {
                assert_eq!(ticket, got);
                result.unwrap();
                break;
            }
            other => panic!("capture publication: {other:?}"),
        }
    }
    // Finished follows the post-publication production maintenance cut.
    let worker_roundtrip_us = elapsed(start);
    let io = io_delta(&io_before, &thread_io(tid));
    let after = worker.metrics_snapshot();
    let db_after = database(home);
    let max_before = db_before["max_payload"].as_u64().unwrap();
    let connection = sql(home);
    let inserted: u64 = connection
        .query_row(
            "SELECT COALESCE(SUM(length(bytes)),0) FROM payloads WHERE payload_id>?1",
            [max_before],
            |r| r.get(0),
        )
        .unwrap();
    json!({"capture_us": capture_us, "queued_bytes": queued_bytes, "payload_bytes_before_lookup": payload_bytes,
        "worker_roundtrip_us": worker_roundtrip_us, "ack_call_us": ack_us,
        "worker_execution_us": after.execution.sum_us - metrics.execution.sum_us,
        "queue_wait_us": after.queue_wait.sum_us - metrics.queue_wait.sum_us,
        "worker_completion_us": after.completion.sum_us - metrics.completion.sum_us,
        "sql_commits": after.store.as_ref().unwrap().commits.physical_commits - metrics.store.as_ref().unwrap().commits.physical_commits,
        "sql_commit_ns": after.store.as_ref().unwrap().commits.commit_ns - metrics.store.as_ref().unwrap().commits.commit_ns,
        "new_retained_payload_bytes": inserted, "worker_io": io,
        "maintenance_cut": db_after["maintenance_cut"], "max_payload": db_after["max_payload"]})
}

fn measure_case(
    name: &str,
    rows: usize,
    width: usize,
    append: usize,
    repetition: usize,
) -> serde_json::Value {
    let (home, worker, mut session, binding, sid) = fixture(rows, width);
    let worker_tid = journal_tid();
    let cut_a = save(&worker, &session, &binding, 100, home.path(), worker_tid);
    let swap_a = swap_files();
    let model = read_model(&worker, &binding.stream, 101);
    let reference_a = model.surfaces[&sid].data.unwrap();
    let lease = worker.retain_payloads(vec![reference_a]).unwrap();
    lazy_control(&mut session, sid, &model.surfaces[&sid]);
    let mutation_io_before = thread_io(current_tid());
    let mutation_start = Instant::now();
    if append != 0 {
        session
            .runtime
            .terminals
            .get_mut(sid)
            .unwrap()
            .inject_scrollback(lines(rows, 1, width));
    }
    let mutation_us = elapsed(mutation_start);
    let mutation_io = io_delta(&mutation_io_before, &thread_io(current_tid()));
    let baseline = database(home.path());
    let telemetry_before = thread_io(current_tid());
    let telemetry_control = io_delta(&telemetry_before, &thread_io(current_tid()));
    let copy_io_before = thread_io(current_tid());
    let start = Instant::now();
    let frozen = session
        .runtime
        .terminals
        .get(sid)
        .unwrap()
        .capture_history_and_screen();
    let copy_us = elapsed(start);
    let copied_rows = frozen.len();
    let screen_rows = copied_rows - rows - append;
    assert_eq!(screen_rows, 8);
    let copy_io = io_delta(&copy_io_before, &thread_io(current_tid()));
    let text_bytes: usize = frozen
        .iter()
        .flat_map(|line| line.cells())
        .map(|(s, _)| s.len())
        .sum();
    let cells: usize = frozen.iter().map(|line| line.cell_count()).sum();
    let start = Instant::now();
    let serialized = tasty_terminal::disk_scrollback::serialize_lines(&frozen);
    let serialize_us = elapsed(start);
    let serialized_len = serialized.len();
    let expected_body_hash = Sha256::digest(&serialized);
    drop(frozen);
    drop(serialized);
    let start = Instant::now();
    let mut captured = surface_capture::capture(&session).unwrap();
    let full_capture_us = elapsed(start);
    assert_eq!(captured.len(), 1);
    let start = Instant::now();
    let decoded = SurfaceData::decode(&captured[0].bytes).unwrap();
    let decode_copy_us = elapsed(start);
    let SurfaceData::Terminal {
        scrollback: Some(body),
        ..
    } = &decoded
    else {
        panic!("complete terminal")
    };
    assert_eq!(
        Sha256::digest(body),
        expected_body_hash,
        "phase probe and actual capture use the same immutable cut"
    );
    let start = Instant::now();
    let reencoded = decoded.encode().unwrap();
    let encode_copy_us = elapsed(start);
    assert_eq!(reencoded, captured[0].bytes);
    drop(reencoded);
    drop(decoded);
    let start = Instant::now();
    RestoreLookup::of(&session).fill(&mut captured).unwrap();
    let lookup_rewrite_us = elapsed(start);
    let rewritten_payload_bytes = captured[0].bytes.len();
    drop(captured);
    let cut_b = save(&worker, &session, &binding, 102, home.path(), worker_tid);
    let after = database(home.path());
    let new_model = read_model(&worker, &binding.stream, 103);
    let reference_b = new_model.surfaces[&sid].data.unwrap();
    let old_body = read_body(home.path(), reference_a.0);
    let new_body = read_body(home.path(), reference_b.0);
    let stored_surface_bytes = read_payload_len(home.path(), reference_b.0);
    assert_eq!(stored_surface_bytes, rewritten_payload_bytes);
    assert_eq!(
        Sha256::digest(&new_body),
        expected_body_hash,
        "full published body matches the exact caller cut"
    );
    assert_eq!(
        tasty_terminal::disk_scrollback::serialize_lines(
            &tasty_terminal::disk_scrollback::deserialize_lines(&new_body).unwrap()
        ),
        new_body,
        "complete TSSB byte roundtrip"
    );
    assert_ne!(
        reference_a, reference_b,
        "a fresh generation is written even without changes"
    );
    if append == 0 {
        assert_eq!(old_body, new_body);
    }
    let old_lines = tasty_terminal::disk_scrollback::deserialize_lines(&old_body).unwrap();
    let new_lines = tasty_terminal::disk_scrollback::deserialize_lines(&new_body).unwrap();
    assert_eq!(old_lines.len(), rows + screen_rows);
    assert_eq!(new_lines.len(), rows + append + screen_rows);
    let history_prefix = tasty_terminal::disk_scrollback::serialize_lines(&old_lines[..rows]);
    assert_eq!(
        &new_body[..history_prefix.len()],
        history_prefix.as_slice(),
        "append preserves the exact old history prefix"
    );
    assert_eq!(
        tasty_terminal::disk_scrollback::serialize_lines(&old_lines[rows..]),
        tasty_terminal::disk_scrollback::serialize_lines(&new_lines[rows + append..]),
        "visible screen survives unchanged"
    );
    assert_eq!(baseline["dormant_payload"], after["dormant_payload"]);
    assert_eq!(baseline["dormant_view_hash"], after["dormant_view_hash"]);
    assert!(
        read_payload_len(home.path(), reference_a.0) > 0,
        "read lease survives maintenance"
    );
    assert!(
        after["maintenance_cut"].as_u64().unwrap() > baseline["maintenance_cut"].as_u64().unwrap()
    );
    let result = json!({
        "case": name, "repetition": repetition, "history_rows_before": rows,
        "history_rows_after": rows + append, "cells_per_history_row": width,
        "cut_a": cut_a, "cut_b": cut_b, "before_b": baseline, "after_b": after,
        "terminal_swap_a": swap_a, "terminal_swap": swap_files(),
        "append_us": mutation_us, "append_io": mutation_io, "stored_surface_bytes": stored_surface_bytes,
        "verified_body_sha256": format!("{expected_body_hash:x}"),
        "phase_probes": {"history_copy_us": copy_us, "copied_text_bytes": text_bytes,
            "copied_cell_length_bytes": cells * 2,
            "copied_default_attr_run_bytes": copied_rows * std::mem::size_of::<(u32, termwiz::cell::CellAttributes)>(),
            "copied_row_struct_bytes": copied_rows * std::mem::size_of::<tasty_terminal::ScrollbackLine>(), "copied_rows": copied_rows, "screen_rows": screen_rows,
            "copy_io": copy_io, "telemetry_only_io": telemetry_control, "tssb_serialize_us": serialize_us, "tssb_bytes": serialized_len,
            "surface_data_decode_copy_us": decode_copy_us, "surface_data_encode_copy_us": encode_copy_us,
            "full_capture_us": full_capture_us, "lookup_rewrite_us": lookup_rewrite_us, "rewritten_payload_bytes": rewritten_payload_bytes},
        "control": {"no_pty": true, "lazy_capture_count": 0, "read_lease_preserved": true,
            "dormant_ref_view_preserved": true, "full_roundtrip": true,
            "same_body_without_change": append == 0}
    });
    drop(lease);
    drop(worker);
    drop(session);
    result
}
