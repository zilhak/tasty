use super::tests::*;
use super::*;
use tasty_ipc::html_http::HttpReply;
#[test]
fn known_automatic_boundaries_and_zero_body_eof_probe() {
    for size in [99_999usize, 100_000, 100_001] {
        let mut d = model();
        start(&mut d, 1, Some(size as u64));
        if size > 100_000 {
            assert!(d.next_command().is_none());
            assert!(d.allow(id(1)));
        }
        let bytes = vec![b'a'; size];
        let mut at = 0;
        while at < size {
            let (r, o, max, _) = credit(&mut d);
            let end = (at + max as usize).min(size);
            chunk(&mut d, r, o, &bytes[at..end]);
            at = end;
        }
        let (_, offset, max, _) = credit(&mut d);
        assert_eq!((offset, max, d.snapshot().reserved), (size as u64, 1, 0));
        complete(&mut d, 1, &bytes);
        d.take_verify_job().unwrap().run();
        d.poll_verification();
        assert!(d.verified(id(1)).is_some());
        assert_eq!(d.snapshot().received, size as u64);
    }
}
#[test]
fn unknown_threshold_probe_eof_or_allowance_never_byte_100001() {
    for length in [99_999usize, 100_000, 100_001] {
        let mut d = model();
        start(&mut d, 1, None);
        let bytes = vec![b'a'; length];
        let mut at = 0;
        while at < length.min(100_000) {
            let (r, o, max, _) = credit(&mut d);
            let end = (at + max as usize).min(length.min(100_000));
            chunk(&mut d, r, o, &bytes[at..end]);
            at = end;
        }
        if length == 100_001 {
            let (r, o, max, allow) = credit(&mut d);
            assert_eq!((o, max, allow), (100_000, 1, false));
            assert_eq!(d.snapshot().reserved, 1);
            assert!(d.receive(HttpReply::AllowanceRequired {
                response: r,
                encoded_at_least: 100_001,
                known_length: None
            }));
            assert_eq!(d.snapshot().reserved, 0);
            assert!(d.allow(id(1)));
            let (r, o, _, allow) = credit(&mut d);
            assert!(allow);
            chunk(&mut d, r, o, b"a");
        } else {
            let (_, _, _, _) = credit(&mut d);
        }
        complete(&mut d, 1, &bytes);
        d.take_verify_job().unwrap().run();
        d.poll_verification();
        assert!(d.verified(id(1)).is_some());
        assert_eq!(d.snapshot().received, length as u64);
    }
    let mut d = model();
    start(&mut d, 1, None);
    let mut at = 0;
    while at < 100_000 {
        let (r, o, max, _) = credit(&mut d);
        chunk(&mut d, r, o, &vec![b'x'; max as usize]);
        at += max;
    }
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, b"x");
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
    assert_eq!(d.snapshot().received, 100_000);
}
#[test]
fn known_late_allowance_preserves_accepted_credit_and_allow() {
    let mut d = model();
    start(&mut d, 1, Some(100_001));
    assert!(d.allow(id(1)));
    let (r, o, max, _) = credit(&mut d);
    assert!(!d.receive(HttpReply::AllowanceRequired {
        response: r,
        encoded_at_least: 100_001,
        known_length: Some(100_001)
    }));
    assert_eq!(d.snapshot().reserved, max as u64);
    assert_eq!(d.state(id(1)), Some(ReceiveState::Receiving));
    assert!(d.next_command().is_none());
    chunk(&mut d, r, o, b"a");
    assert_eq!(d.snapshot().received, 1);
}
fn fill(d: &mut ClientDocument, n: u64, size: usize) {
    start(d, n, Some(size as u64));
    if size > 100_000 {
        assert!(d.allow(id(n)));
    }
    let mut at = 0;
    while at < size {
        let (r, o, max, _) = credit(d);
        let end = (at + max as usize).min(size);
        chunk(d, r, o, &vec![b'x'; end - at]);
        at = end;
    }
}
#[test]
fn aggregate_exact_eof_and_continue_independent_of_allow() {
    for size in [1_999_999usize, 2_000_000] {
        let mut d = model();
        fill(&mut d, 1, size);
        let (_, o, max, _) = credit(&mut d);
        assert_eq!((o, max, d.snapshot().reserved), (size as u64, 1, 0));
        complete(&mut d, 1, &vec![b'x'; size]);
        // A second response's possible window cannot oversubscribe the cumulative limit.
        start(&mut d, 2, Some(100_001));
        assert!(d.allow(id(2)));
        assert!(d.next_command().is_none());
        assert!(d.snapshot().paused);
        assert!(d.continue_receiving());
        assert!(matches!(
            d.next_command(),
            Some(HttpRequest::Credit {
                allow_large: true,
                ..
            })
        ));
    }
    let mut d = model();
    start(&mut d, 1, Some(2_000_001));
    assert!(d.continue_receiving());
    assert!(d.next_command().is_none());
    assert!(d.allow(id(1)));
    assert!(d.next_command().is_some());
}
#[test]
fn simultaneous_windows_and_unknown_probe_conservatively_pause() {
    let mut d = model();
    fill(&mut d, 1, 1_900_000);
    complete(&mut d, 1, &vec![b'x'; 1_900_000]);
    start(&mut d, 2, Some(100_000));
    start(&mut d, 3, Some(100_000));
    let (r, o, max, _) = credit(&mut d);
    assert_eq!(r, response(2));
    let (r3, _, max3, _) = credit(&mut d);
    assert_eq!(r3, response(3));
    assert_eq!(d.snapshot().reserved, (max + max3) as u64);
    chunk(&mut d, r, o, &vec![b'x'; max as usize]);
    assert!(d.next_command().is_none());
    assert!(d.snapshot().paused);
    let mut d = model();
    fill(&mut d, 1, 1_900_000);
    complete(&mut d, 1, &vec![b'x'; 1_900_000]);
    start(&mut d, 2, None);
    for _ in 0..3 {
        let (r, o, max, _) = credit(&mut d);
        chunk(&mut d, r, o, &vec![b'x'; max as usize]);
    }
    assert_eq!(d.snapshot().received, 2_000_000);
    assert!(d.next_command().is_none());
    assert!(d.snapshot().paused);
    assert!(d.continue_receiving());
    let (r, _, max, allow) = credit(&mut d);
    assert_eq!((max, allow), (1, false));
    assert!(d.receive(HttpReply::AllowanceRequired {
        response: r,
        encoded_at_least: 100_001,
        known_length: None
    }));
    assert_eq!(d.snapshot().reserved, 0);
    assert!(d.allow(id(2)));
    let (r, o, _, allow) = credit(&mut d);
    assert!(allow);
    chunk(&mut d, r, o, b"x");
    assert_eq!(d.snapshot().received, 2_000_001);
}
#[test]
fn known_eof_cannot_authorize_positive_bytes() {
    let mut d = model();
    start(&mut d, 1, Some(0));
    let (r, o, _, _) = credit(&mut d);
    assert_eq!(d.snapshot().reserved, 0);
    chunk(&mut d, r, o, b"x");
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
    assert_eq!(d.snapshot().received, 0);
}
#[test]
fn exact_same_arc_reuse_requires_head_complete_and_forbids_chunks_or_post_fallback() {
    let mut d = model();
    verified_body(&mut d, 1, b"abc");
    let body = d.verified(id(1)).unwrap();
    let received = d.snapshot().received;
    assert!(d.reuse(response(1)).is_some());
    assert!(d.verified(id(1)).is_none());
    assert!(d.next_command().is_none());
    assert!(d.receive(HttpReply::Head {
        head: head(1, Some(3))
    }));
    complete(&mut d, 1, b"abc");
    assert!(Arc::ptr_eq(&body, &d.verified(id(1)).unwrap()));
    assert!(d.take_verify_job().is_none());
    assert_eq!(d.snapshot().received, received);
    assert!(d.stop().is_some());
    assert!(Arc::ptr_eq(&body, &d.verified(id(1)).unwrap()));
    assert!(d.release(response(1)).is_some());
    assert!(d.reuse(response(1)).is_none());
    assert_eq!(body.encoded(), b"abc");
    let mut d = model();
    verified_body(&mut d, 1, b"abc");
    d.reuse(response(1));
    chunk(&mut d, response(1), 0, b"abc");
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
    assert!(d.next_command().is_none());
}
#[test]
fn tamper_or_decode_failure_never_publishes_and_job_abandonment_resolves() {
    for bad in [false, true] {
        let mut d = model();
        start(&mut d, 1, Some(3));
        let (r, o, _, _) = credit(&mut d);
        chunk(&mut d, r, o, b"abc");
        let expected = if bad {
            identity(b"bad")
        } else {
            identity(b"abc")
        };
        assert!(d.receive(HttpReply::Complete {
            response: r,
            encoded: expected,
            decoded: expected
        }));
        let job = d.take_verify_job().unwrap();
        if bad {
            job.run();
        } else {
            drop(job);
        }
        assert!(d.poll_verification());
        assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
        assert!(d.verified(id(1)).is_none());
    }
    let mut d = model();
    admitted(&mut d, 1);
    let mut h = head(1, Some(3));
    h.headers.push(tasty_ipc::html_http::Header {
        name: "content-encoding".into(),
        value: b"gzip".to_vec(),
    });
    d.receive(HttpReply::Head { head: h });
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, b"bad");
    complete(&mut d, 1, b"bad");
    d.take_verify_job().unwrap().run();
    assert!(d.poll_verification());
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
}
#[test]
fn queued_old_result_and_job_leases_survive_until_actual_last_owner() {
    let p = PendingBudget::new(32_000_000);
    let b = BodyBudget::new(32_000_000);
    let mut d = model_with(p.clone(), b.clone());
    start(&mut d, 1, Some(3));
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, b"abc");
    complete(&mut d, 1, b"abc");
    let job = d.take_verify_job().unwrap();
    let before = p.used();
    assert!(before > 0);
    d.close();
    drop(d);
    assert!(p.used() > 0);
    job.run();
    assert_eq!(p.used(), 0);
    assert_eq!(b.used(), 0);
    let mut d = model_with(p.clone(), b.clone());
    verified_body(&mut d, 1, b"abc");
    let native = d.verified(id(1)).unwrap();
    d.close();
    drop(d);
    assert!(b.used() > 0);
    drop(native);
    assert_eq!(b.used(), 0);
    assert_eq!(p.used(), 0);
    let mut d = model();
    start(&mut d, 1, Some(1));
    let (r, o, _, _) = credit(&mut d);
    chunk(&mut d, r, o, b"x");
    complete(&mut d, 1, b"x");
    d.take_verify_job().unwrap().run();
    assert!(d.close().is_some());
    assert!(!d.poll_verification());
    assert!(d.verified(id(1)).is_none());
}
#[test]
fn shared_budgets_and_document_close_do_not_retire_connection() {
    let p = PendingBudget::new(32_000_000);
    let b = BodyBudget::new(32_000_000);
    let (sender, _queue) = crate::connection::channel();
    let epoch = sender.epoch();
    let mut a = model_on(epoch.clone(), p.clone(), b.clone());
    let mut other = model_on(epoch.clone(), p.clone(), b.clone());
    verified_body(&mut a, 1, b"one");
    verified_body(&mut other, 1, b"two");
    let used = b.used();
    assert!(a.close().is_some());
    assert!(epoch.is_active());
    assert!(b.used() < used);
    assert_eq!(other.verified(id(1)).unwrap().encoded(), b"two");
    drop(a);
    drop(other);
    assert_eq!(p.used(), 0);
    assert_eq!(b.used(), 0);
    let mut d = model_with(PendingBudget::new(20_000), BodyBudget::new(1));
    verified_failure(&mut d);
}
fn verified_failure(d: &mut ClientDocument) {
    start(d, 1, Some(0));
    complete(d, 1, b"");
    d.take_verify_job().unwrap().run();
    d.poll_verification();
    assert!(d.verified(id(1)).is_none());
    assert_eq!(d.state(id(1)), Some(ReceiveState::Failed));
}
