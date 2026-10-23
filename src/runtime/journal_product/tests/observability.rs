use super::*;

#[test]
fn actual_worker_queue_gate_rejections_and_consumption_do_not_fake_commits() {
    let home = tempfile::tempdir().unwrap();
    let mut worker = JournalWorker::spawn(home.path().to_owned(), Arc::new(|| {})).unwrap();
    assert!(matches!(
        receive(&worker),
        Completion::Ready { cut: None, .. }
    ));
    let before = worker.metrics_snapshot().store.unwrap();
    let estimate =
        crate::runtime::journal_product::request_size(&Work::ReadEngine("structure:slot-1".into()));
    for ticket in 0..QUEUE_CAPACITY as u64 {
        submit(&worker, ticket, Work::ReadEngine("structure:slot-1".into()));
    }
    assert_eq!(
        worker.submit(Request {
            ticket: 99,
            work: Work::ReadEngine("structure:slot-1".into())
        }),
        Err(SubmitError::Busy)
    );
    assert_eq!(
        worker.submit(Request {
            ticket: 100,
            work: Work::PutPayload(vec![0; MAX_QUEUED_BYTES + 1])
        }),
        Err(SubmitError::TooLarge)
    );
    assert_queued(&worker, estimate, before);
    worker.acknowledge(0, Ok(())).unwrap();
    for ticket in 0..QUEUE_CAPACITY as u64 {
        assert!(finished(&worker, ticket).is_ok());
    }
    assert_done(&worker, before);
    worker.stop();
    assert_eq!(
        worker.submit(Request {
            ticket: 101,
            work: Work::ReadEngine("structure:slot-1".into())
        }),
        Err(SubmitError::Stopped)
    );
    assert_eq!(worker.metrics_snapshot().work.rejected_stopped, 1);
}

fn assert_queued(
    worker: &JournalWorker,
    estimate: usize,
    before: tasty_event_store::StoreMetricsSnapshot,
) {
    let queued = worker.metrics_snapshot();
    assert_eq!(queued.work.admitted, 64);
    assert_eq!(queued.work.rejected_busy, 1);
    assert_eq!(queued.work.rejected_too_large, 1);
    assert_eq!(queued.work.queued_items, 64);
    assert_eq!(queued.work.queued_bytes_estimate, (estimate * 64) as u64);
    assert_eq!(queued.work.inflight_items, 0);
    assert_eq!(queued.store.unwrap(), before);
}

fn assert_done(worker: &JournalWorker, before: tasty_event_store::StoreMetricsSnapshot) {
    let done = worker.metrics_snapshot();
    assert_eq!(done.work.handled_ok, 64);
    assert_eq!(done.work.pending_items, 0);
    assert!(done.oldest_pending_age_us.is_none());
    assert_eq!(done.execution.samples, 64);
    assert_eq!(done.queue_wait.samples, 64);
    assert_eq!(done.completion.samples, 64);
    assert_eq!(
        done.store.unwrap(),
        before,
        "read execution is not physical SQL commit"
    );
    let public = crate::adapters::ipc::handler::journal_pressure::snapshot_json(Some(&done));
    assert_eq!(public["admitted"], 64);
    assert!(public["dirty_age_us"].is_null());
    assert_eq!(
        public["store"]["physical_commits"],
        before.commits.physical_commits
    );
}
