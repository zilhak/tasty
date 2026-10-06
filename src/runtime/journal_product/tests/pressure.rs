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

#[test]
fn admissions_beyond_the_disk_credit_wait_for_a_released_credit_instead_of_failing() {
    let home = tempfile::tempdir().unwrap();
    let worker = start(home.path());
    let budget = tasty_event_store::AdmissionBudget::default();
    let submitted = budget.admission_ceiling() / budget.command_credit_bytes + 3;
    for ticket in 0..submitted {
        submit(
            &worker,
            ticket,
            Work::Admit(header(&format!("credit-{ticket}"))),
        );
    }
    // The worker handles requests in order, so once this read completes every admission above
    // has either finished or joined the credit wait. A quiet period would only guess that.
    let barrier = u64::MAX;
    submit(
        &worker,
        barrier,
        Work::ReadEngine("structure:credit-barrier".into()),
    );
    let mut admitted = Vec::new();
    loop {
        match receive(&worker) {
            Completion::Finished { ticket, .. } if ticket == barrier => break,
            Completion::Finished { ticket, result } => {
                assert!(
                    matches!(result, Ok(ResultValue::NeedsResolution)),
                    "ticket {ticket} must wait for credit instead of failing: {result:?}"
                );
                admitted.push(ticket);
            }
            other => panic!("{other:?}"),
        }
    }
    let waiting = submitted - admitted.len() as u64;
    assert!(
        waiting > 0,
        "the default credit must leave some admissions waiting"
    );
    assert_eq!(admitted, (0..admitted.len() as u64).collect::<Vec<_>>());

    submit(&worker, 0, Work::CancelAdmission);
    assert!(finished(&worker, 0).is_ok());
    assert!(matches!(
        finished(&worker, admitted.len() as u64),
        Ok(ResultValue::NeedsResolution)
    ));
}
