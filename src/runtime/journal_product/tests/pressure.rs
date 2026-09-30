use super::*;

#[test]
fn journal_queue_byte_budget_backpressures_before_its_count_limit_and_releases_on_stop() {
    let home = tempfile::tempdir().unwrap();
    let mut worker = JournalWorker::spawn(home.path().to_owned(), Arc::new(|| {})).unwrap();
    assert!(matches!(receive(&worker), Completion::Ready { .. }));
    let mut held = 0;
    for ticket in 0..9 {
        let mut admission = header(&format!("bytes-{ticket}"));
        admission.original_digest = vec![b'x'; 7 * 1024 * 1024];
        let work = Work::Admit(admission);
        held += request_size(&work);
        submit(&worker, ticket, work);
    }
    assert_eq!(worker.queued_bytes.load(Ordering::Acquire), held);
    assert!(held < MAX_QUEUED_BYTES);
    let mut admission = header("too-many-bytes");
    admission.original_digest = vec![b'x'; 7 * 1024 * 1024];
    assert_eq!(
        worker.submit(Request {
            ticket: 10,
            work: Work::Admit(admission)
        }),
        Err(SubmitError::Busy)
    );
    assert_eq!(
        worker.queued_bytes.load(Ordering::Acquire),
        held,
        "rejected send consumes no budget"
    );
    let queued_bytes = worker.queued_bytes.clone();
    worker.stop();
    // stop disconnects asynchronously; Drop joins before reclamation is complete.
    drop(worker);
    assert_eq!(queued_bytes.load(Ordering::Acquire), 0);
}
