use super::{tests::*, *};
use tasty_ipc::html_http::{CommandKind, FailureCode};
#[test]
fn running_verifier_cancel_keeps_worker_lease_until_real_return() {
    use std::time::{Duration, Instant};
    let p = PendingBudget::new(40_000_000);
    let b = BodyBudget::new(40_000_000);
    let mut d = model_with(p.clone(), b.clone());
    let bytes = vec![b'x'; 2_500_000];
    start(&mut d, 1, Some(bytes.len() as u64));
    d.allow(id(1));
    d.continue_receiving();
    let mut at = 0;
    while at < bytes.len() {
        let (r, o, max, _) = credit(&mut d);
        let end = (at + max as usize).min(bytes.len());
        chunk(&mut d, r, o, &bytes[at..end]);
        at = end;
    }
    complete(&mut d, 1, &bytes);
    let job = d.take_verify_job().unwrap();
    let finished = Arc::new(AtomicBool::new(false));
    let done = finished.clone();
    let worker = std::thread::spawn(move || {
        job.run();
        done.store(true, Ordering::Release);
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    while b.used() == 0 && !finished.load(Ordering::Acquire) && Instant::now() < deadline {
        std::thread::yield_now();
    }
    assert!(
        b.used() > 0,
        "actual verifier never observed holding BodyBudget before termination"
    );
    assert!(
        !finished.load(Ordering::Acquire),
        "worker already ended before cancellation observation"
    );
    d.close();
    drop(d);
    // The worker owns the buffers now. No engine-loop join is part of the product model.
    worker.join().unwrap();
    assert_eq!(b.used(), 0);
    assert_eq!(p.used(), 0);
}
#[test]
fn verifier_slots_remain_bounded_until_results_or_abandoned_jobs_drop() {
    let mut d = model();
    for n in 1..=3 {
        start(&mut d, n, Some(1));
        let (r, o, _, _) = credit(&mut d);
        chunk(&mut d, r, o, b"x");
        complete(&mut d, n, b"x");
    }
    assert_eq!(d.state(id(3)), Some(ReceiveState::Failed));
    let first = d.take_verify_job().unwrap();
    let second = d.take_verify_job().unwrap();
    assert!(d.take_verify_job().is_none());
    drop(first);
    assert!(d.poll_verification());
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
    second.run();
    d.poll_verification();
    assert!(d.verified(id(2)).is_some());
}
#[test]
fn invalid_upload_sequence_and_hash_are_local_not_retries() {
    for (offset, max) in [(1, 1), (0, 0), (0, 49_153)] {
        let mut d = model();
        d.admit(begin(1, b"abc"), Arc::new(b"abc".to_vec()))
            .unwrap_or_else(|_| panic!());
        ready(&mut d, 1, offset, max);
        assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
        assert!(d.next_command().is_none());
    }
    let mut d = model();
    d.admit(begin(1, b"abc"), Arc::new(b"abc".to_vec()))
        .unwrap_or_else(|_| panic!());
    ready(&mut d, 1, 0, 3);
    ready(&mut d, 1, 0, 3);
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
}
#[test]
fn new_begin_rejection_and_prehead_terminal_serial_resolve_once() {
    let mut d = model();
    d.admit(begin(1, b""), Arc::new(vec![]))
        .unwrap_or_else(|_| panic!());
    assert!(d.receive(HttpReply::CommandRejected {
        id: id(1),
        source_generation: 1,
        command: CommandKind::Begin,
        response: None,
        code: FailureCode::Quota,
        reason: "quota".into()
    }));
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
    assert!(!d.receive(HttpReply::UploadReady {
        id: id(1),
        source_generation: 1,
        offset: 0,
        max_bytes: 1
    }));
    admitted(&mut d, 2);
    assert!(d.receive(HttpReply::Failure {
        id: id(2),
        source_generation: 1,
        response: Some(response(2)),
        code: FailureCode::Network,
        reason: "network".into()
    }));
    assert!(!d.receive(HttpReply::Head {
        head: head(2, Some(0))
    }));
    assert!(d.next_command().is_none());
}
#[test]
fn replay_mismatch_and_duplicate_completion_cannot_fall_back() {
    for kind in 0..3 {
        let mut d = model();
        verified_body(&mut d, 1, b"abc");
        d.reuse(response(1));
        match kind {
            0 => {
                let mut h = head(1, Some(3));
                h.status = 201;
                d.receive(HttpReply::Head { head: h });
            }
            1 => {
                complete(&mut d, 1, b"abc");
            }
            _ => {
                d.receive(HttpReply::Head {
                    head: head(1, Some(3)),
                });
                complete(&mut d, 1, b"bad");
            }
        }
        assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
        assert!(d.next_command().is_none());
        assert!(d.reuse(response(1)).is_none());
    }
}
#[test]
fn stop_resolves_notices_uploads_and_jobs_but_preserves_completed_entry() {
    let mut d = model();
    verified_body(&mut d, 1, b"entry");
    let entry = d.verified(id(1)).unwrap();
    start(&mut d, 2, Some(100_001));
    d.admit(begin(3, b"pending"), Arc::new(b"pending".to_vec()))
        .unwrap_or_else(|_| panic!());
    assert!(d.stop().is_some());
    assert!(d.snapshot().notices.is_empty());
    assert!(!d.snapshot().receiving);
    assert_eq!(d.snapshot().skipped, 2);
    assert!(d.next_command().is_none());
    assert!(Arc::ptr_eq(&entry, &d.verified(id(1)).unwrap()));
    assert!(d.stop().is_none());
    assert_eq!(
        d.admit(begin(4, b""), Arc::new(vec![])).err(),
        Some(ClientError::Closed)
    );
}
#[test]
fn provisional_quota_is_shared_released_on_failure_and_does_not_expand_with_consent() {
    let capacity = 2 * (16 * std::mem::size_of::<Attempt>() + 128) + 40_000;
    let p = PendingBudget::new(capacity);
    let b = BodyBudget::new(32_000_000);
    let mut d = model_with(p.clone(), b.clone());
    let baseline = p.used();
    let upload = Arc::new(vec![0; 30_000]);
    d.admit(begin(1, &upload), upload)
        .unwrap_or_else(|_| panic!());
    let mut other = model_with(p.clone(), b);
    assert_eq!(
        other
            .admit(begin(1, &vec![0; 30_000]), Arc::new(vec![0; 30_000]))
            .err(),
        Some(ClientError::Quota)
    );
    d.receive(HttpReply::Failure {
        id: id(1),
        source_generation: 1,
        response: None,
        code: FailureCode::Invalid,
        reason: "invalid".into(),
    });
    assert!(p.used() < baseline + 30_000);
    drop(d);
    drop(other);
    assert_eq!(p.used(), 0);
    let mut d = model_with(PendingBudget::new(20_000), BodyBudget::new(32_000_000));
    start(&mut d, 1, Some(100_001));
    d.continue_receiving();
    d.allow(id(1));
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, b"x");
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
    assert_eq!(d.snapshot().reserved, 0);
}
#[test]
fn shared_verified_quota_stays_with_native_arc_after_model_release() {
    let p = PendingBudget::new(1_000_000);
    let b = BodyBudget::new(2_000);
    let mut one = model_with(p.clone(), b.clone());
    let mut two = model_with(p, b.clone());
    let bytes = vec![b'a'; 1_200];
    verified_body(&mut one, 1, &bytes);
    let native = one.verified(id(1)).unwrap();
    start(&mut two, 1, Some(1_200));
    let (r, o, _, _) = credit(&mut two);
    chunk(&mut two, r, o, &bytes);
    complete(&mut two, 1, &bytes);
    two.take_verify_job().unwrap().run();
    two.poll_verification();
    assert_eq!(two.state(id(1)), Some(ReceiveState::Failed));
    one.release(response(1));
    assert!(b.used() >= 1_200);
    drop(native);
    assert_eq!(b.used(), 0);
    verified_body(&mut two, 2, &bytes);
}
#[test]
fn cumulative_plus_one_needs_continue_and_preserves_per_response_allow() {
    let mut d = model();
    let size = 2_000_001usize;
    let bytes = vec![b'x'; size];
    start(&mut d, 1, Some(size as u64));
    d.allow(id(1));
    let mut at = 0;
    while let Some(command) = d.next_command() {
        let HttpRequest::Credit {
            response,
            offset,
            max_bytes,
            ..
        } = command
        else {
            panic!()
        };
        let end = (at + max_bytes as usize).min(size);
        chunk(&mut d, response, offset, &bytes[at..end]);
        at = end;
    }
    assert!(d.snapshot().paused);
    assert!(at < size);
    assert!(d.snapshot().received <= 2_000_000);
    assert!(d.continue_receiving());
    let (r, o, max, allow) = credit(&mut d);
    assert!(allow);
    assert_eq!(size - at, max as usize);
    chunk(&mut d, r, o, &bytes[at..]);
    assert_eq!(d.snapshot().received, size as u64);
    let (_, _, _, _) = credit(&mut d);
    assert_eq!(d.snapshot().reserved, 0);
    complete(&mut d, 1, &bytes);
    d.take_verify_job().unwrap().run();
    d.poll_verification();
    assert!(d.verified(id(1)).is_some());
}
#[test]
fn encoded_chunks_are_the_only_accounted_bytes_and_real_decode_scan_precedes_publication() {
    use std::io::Write;
    let decoded = b"<html><script>window.example=1;</script></html>";
    let mut gzip = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    gzip.write_all(decoded).unwrap();
    let encoded = gzip.finish().unwrap();
    let mut d = model();
    admitted(&mut d, 1);
    let mut h = head(1, Some(encoded.len() as u64));
    h.headers.push(tasty_ipc::html_http::Header {
        name: "content-encoding".into(),
        value: b"gzip".to_vec(),
    });
    assert!(d.receive(HttpReply::Head { head: h }));
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, &encoded);
    assert!(d.receive(HttpReply::Complete {
        response: r,
        encoded: identity(&encoded),
        decoded: identity(decoded)
    }));
    assert!(d.verified(id(1)).is_none());
    d.take_verify_job().unwrap().run();
    assert!(d.verified(id(1)).is_none());
    d.poll_verification();
    let verified = d.verified(id(1)).unwrap();
    assert_eq!(verified.encoded(), encoded);
    assert_eq!(verified.decoded(), decoded);
    assert!(verified.scan().detection.has_scripts());
    assert_eq!(d.snapshot().received, encoded.len() as u64);
    assert_eq!(verified.decoded_identity(), identity(decoded));
}
#[test]
fn captured_document_entry_policy_and_both_cursor_failures_are_atomic() {
    let mut d = model();
    for kind in 0..3 {
        let mut bad = begin(1, b"");
        if let HttpRequest::Begin {
            logical_url,
            allow_remote_content,
            id,
            ..
        } = &mut bad
        {
            match kind {
                0 => *logical_url = "http://other.test/".into(),
                1 => *allow_remote_content = true,
                _ => id.binding += 1,
            }
        }
        assert!(d.admit(bad, Arc::new(vec![])).is_err());
    }
    admitted(&mut d, 1);
    let mut old_request = begin(1, b"");
    if let HttpRequest::Begin { id, .. } = &mut old_request {
        id.transfer = 3;
    }
    assert_eq!(
        d.admit(old_request, Arc::new(vec![])).err(),
        Some(ClientError::AlreadyUsed)
    );
    admitted(&mut d, 2);
    let mut closed = model();
    closed.send_failed();
    assert_eq!(
        closed.admit(begin(1, b""), Arc::new(vec![])).err(),
        Some(ClientError::Closed)
    );
    assert!(closed.next_command().is_none());
    assert!(closed.close().is_none());
}
#[test]
fn retained_metadata_charge_reconciles_clone_capacity_and_terminal_release() {
    let p = PendingBudget::new(100_000);
    let mut d = model_with(p.clone(), BodyBudget::new(100_000));
    let baseline = p.used();
    let mut request = begin(1, b"");
    if let HttpRequest::Begin {
        method,
        logical_url,
        headers,
        ..
    } = &mut request
    {
        method.reserve_exact(2_000);
        logical_url.reserve_exact(2_000);
        headers.reserve_exact(100);
        headers.push(tasty_ipc::html_http::Header {
            name: "x-test".into(),
            value: b"value".to_vec(),
        });
    }
    d.admit(request, Arc::new(vec![]))
        .unwrap_or_else(|_| panic!());
    let HttpRequest::Begin {
        method,
        logical_url,
        headers,
        ..
    } = &d.attempts[0].request.as_ref().unwrap().begin
    else {
        panic!("retained Begin");
    };
    let names_and_values: usize = headers
        .iter()
        .map(|h| h.name.capacity() + h.value.capacity())
        .sum();
    let retained = method.capacity()
        + logical_url.capacity()
        + headers.capacity() * std::mem::size_of::<tasty_ipc::html_http::Header>()
        + names_and_values;
    assert!(retained > 0);
    assert_eq!(p.used(), baseline + retained);
    d.receive(HttpReply::Failure {
        id: id(1),
        source_generation: 1,
        response: None,
        code: FailureCode::Invalid,
        reason: "invalid".into(),
    });
    assert_eq!(p.used(), baseline);
    drop(d);
    assert_eq!(p.used(), 0);
}
#[test]
fn consumed_result_cannot_release_slot_before_worker_run_returns() {
    let mut d = model();
    let entered = Arc::new(std::sync::Barrier::new(2));
    let leave = Arc::new(std::sync::Barrier::new(2));
    let (worker_entered, worker_leave) = (entered.clone(), leave.clone());
    d.wake = Arc::new(move || {
        worker_entered.wait();
        worker_leave.wait();
    });
    start(&mut d, 1, Some(3));
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, b"abc");
    complete(&mut d, 1, b"abc");
    let job = d.take_verify_job().unwrap();
    let worker = std::thread::spawn(move || job.run());
    entered.wait();
    let changed = d.poll_verification();
    let held = d.slots.used();
    leave.wait();
    worker.join().unwrap();
    assert!(changed);
    assert_eq!(
        held, 1,
        "result consumption must not release a worker still inside run"
    );
    assert_eq!(d.slots.used(), 0);
    assert_eq!(d.verified(id(1)).unwrap().encoded(), b"abc");
}
