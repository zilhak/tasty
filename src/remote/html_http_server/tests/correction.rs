use super::*;
fn empty_poll(s: &mut Server, t: &OwnerToken) {
    assert!(
        s.poll(t, |_| true, MAX_FRAME_LEN as usize + 5)
            .unwrap()
            .is_none()
    );
}
fn rejected(s: &mut Server, t: &OwnerToken, request: wire::HttpRequest) -> wire::HttpReply {
    let error = handle(s, t, request.clone()).unwrap_err();
    let reply = rejection_reply(&request, &error);
    reply.validate().unwrap();
    reply
}
#[tokio::test]
async fn rejected_credit_preserves_accepted_window_and_same_post_completion() {
    let f = Fixture::new(1, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    start(&mut s, &t, 1, &url, &[]).await;
    let wire::HttpReply::Head { head } = reply(&mut s, &t).await else {
        panic!("head")
    };
    let request = wire::HttpRequest::Credit {
        response: head.id,
        offset: 0,
        max_bytes: 0,
        allow_large: false,
    };
    let rejection = rejected(&mut s, &t, request);
    assert!(
        matches!(rejection,wire::HttpReply::CommandRejected{command:wire::CommandKind::Credit,code:wire::FailureCode::Invalid,response:Some(r),..} if r==head.id)
    );
    grant(&mut s, &t, head.id, 0, false).unwrap();
    let duplicate = rejected(
        &mut s,
        &t,
        wire::HttpRequest::Credit {
            response: head.id,
            offset: 0,
            max_bytes: 1,
            allow_large: false,
        },
    );
    assert!(matches!(
        duplicate,
        wire::HttpReply::CommandRejected {
            code: wire::FailureCode::InProgress,
            ..
        }
    ));
    let owner = &s.documents[s.index(&t).unwrap()];
    let State::Running(r) = &owner.requests[&id(1)].state else {
        panic!("running")
    };
    assert!(r.outstanding.is_some());
    assert!(
        matches!(reply(&mut s,&t).await,wire::HttpReply::Chunk{response:r,offset:0,..} if r==head.id)
    );
    grant(&mut s, &t, head.id, 1, false).unwrap();
    assert!(
        matches!(reply(&mut s,&t).await,wire::HttpReply::Complete{response:r,..} if r==head.id)
    );
    assert_eq!(f.count(), 1);
    assert_eq!(s.transferred_bytes(&t).unwrap(), 1);
    handle(&mut s, &t, wire::HttpRequest::Reuse { response: head.id }).unwrap();
    assert!(matches!(reply(&mut s,&t).await,wire::HttpReply::Head{head:h} if h.id==head.id));
    assert!(
        matches!(reply(&mut s,&t).await,wire::HttpReply::Complete{response:r,..} if r==head.id)
    );
    assert_eq!(f.count(), 1);
    s.retire(&t);
    wait_workers(&b).await;
}
#[tokio::test]
async fn malformed_gzip_is_terminal_and_distinct_from_command_rejection() {
    let f = Fixture::bad_gzip();
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    start(&mut s, &t, 1, &url, &[]).await;
    let wire::HttpReply::Head { head } = reply(&mut s, &t).await else {
        panic!("head")
    };
    let rejection = rejected(
        &mut s,
        &t,
        wire::HttpRequest::Credit {
            response: head.id,
            offset: 0,
            max_bytes: 0,
            allow_large: false,
        },
    );
    grant(&mut s, &t, head.id, 0, false).unwrap();
    assert!(matches!(reply(&mut s,&t).await,wire::HttpReply::Chunk{response:r,..} if r==head.id));
    grant(&mut s, &t, head.id, 1, false).unwrap();
    let terminal = reply(&mut s, &t).await;
    assert!(
        matches!(terminal,wire::HttpReply::Failure{code:wire::FailureCode::Invalid,response:Some(r),..} if r==head.id)
    );
    assert_ne!(
        rejection.frame().unwrap().payload,
        terminal.frame().unwrap().payload
    );
    empty_poll(&mut s, &t);
    assert!(handle(&mut s, &t, wire::HttpRequest::Reuse { response: head.id }).is_err());
    assert!(matches!(
        rejected(&mut s, &t, begin(1, &url, &[])),
        wire::HttpReply::CommandRejected {
            command: wire::CommandKind::Begin,
            code: wire::FailureCode::AlreadyUsed,
            ..
        }
    ));
    empty_poll(&mut s, &t);
    assert_eq!(f.count(), 1);
    s.retire(&t);
    wait_workers(&b).await;
}
#[tokio::test]
async fn destructive_sha_commit_queues_exactly_one_terminal_failure() {
    let f = Fixture::new(1, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    handle(&mut s, &t, begin(1, &url, b"good")).unwrap();
    reply(&mut s, &t).await;
    handle(
        &mut s,
        &t,
        wire::HttpRequest::Upload {
            id: id(1),
            source_generation: 7,
            offset: 0,
            data_b64: encode(b"evil"),
        },
    )
    .unwrap();
    let commit = wire::HttpRequest::Commit {
        id: id(1),
        source_generation: 7,
    };
    assert!(matches!(
        rejected(&mut s, &t, commit.clone()),
        wire::HttpReply::CommandRejected {
            command: wire::CommandKind::Commit,
            code: wire::FailureCode::Invalid,
            response: None,
            ..
        }
    ));
    // Rejected duplicate commands cannot overwrite the original queued terminal result.
    assert!(matches!(
        rejected(&mut s, &t, commit),
        wire::HttpReply::CommandRejected {
            code: wire::FailureCode::AlreadyUsed,
            ..
        }
    ));
    assert!(matches!(
        rejected(&mut s, &t, begin(1, &url, b"good")),
        wire::HttpReply::CommandRejected {
            code: wire::FailureCode::AlreadyUsed,
            ..
        }
    ));
    assert!(
        matches!(reply(&mut s,&t).await,wire::HttpReply::Failure{id:r,source_generation:7,response:None,code:wire::FailureCode::Invalid,..} if r==id(1))
    );
    empty_poll(&mut s, &t);
    assert_eq!(f.count(), 0);
    assert_eq!(b.retained_payload_bytes(), 0);
    s.retire(&t);
    wait_workers(&b).await;
}
#[tokio::test]
async fn destructive_core_open_error_queues_failure_without_an_upstream_post() {
    let f = Fixture::new(0, false);
    let other = Fixture::new(0, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    start(&mut s, &t, 1, &url, &[]).await;
    let (entry, _, _, _) = collect(&mut s, &t).await;
    handle(&mut s, &t, begin(2, &other.url(), &[])).unwrap();
    reply(&mut s, &t).await;
    assert!(matches!(
        rejected(
            &mut s,
            &t,
            wire::HttpRequest::Commit {
                id: id(2),
                source_generation: 7
            }
        ),
        wire::HttpReply::CommandRejected {
            command: wire::CommandKind::Commit,
            code: wire::FailureCode::Invalid,
            ..
        }
    ));
    assert!(
        matches!(reply(&mut s,&t).await,wire::HttpReply::Failure{id:r,response:None,code:wire::FailureCode::Invalid,..} if r==id(2))
    );
    empty_poll(&mut s, &t);
    assert_eq!(other.count(), 0);
    assert_eq!(f.count(), 1);
    handle(&mut s, &t, wire::HttpRequest::Reuse { response: entry }).unwrap();
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::Head { .. }
    ));
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::Complete { .. }
    ));
    assert_eq!(f.count(), 1);
    s.retire(&t);
    wait_workers(&b).await;
}
#[tokio::test]
async fn duplicate_begin_rejection_preserves_both_upload_and_completed_response() {
    let f = Fixture::new(1, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    let original = begin(1, &url, &[]);
    handle(&mut s, &t, original.clone()).unwrap();
    assert!(matches!(
        rejected(&mut s, &t, original.clone()),
        wire::HttpReply::CommandRejected {
            command: wire::CommandKind::Begin,
            response: None,
            code: wire::FailureCode::InProgress,
            ..
        }
    ));
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::UploadReady { .. }
    ));
    handle(
        &mut s,
        &t,
        wire::HttpRequest::Commit {
            id: id(1),
            source_generation: 7,
        },
    )
    .unwrap();
    let (r, bytes, _, _) = collect(&mut s, &t).await;
    assert_eq!(bytes.len(), 1);
    assert!(matches!(
        rejected(&mut s, &t, original),
        wire::HttpReply::CommandRejected {
            code: wire::FailureCode::AlreadyUsed,
            ..
        }
    ));
    empty_poll(&mut s, &t);
    handle(&mut s, &t, wire::HttpRequest::Reuse { response: r }).unwrap();
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::Head { .. }
    ));
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::Complete { .. }
    ));
    assert_eq!(f.count(), 1);
    s.retire(&t);
    wait_workers(&b).await;
}
#[test]
fn rejection_helpers_correlate_only_supplied_nonzero_response_and_sanitize_all_codes() {
    use super::core::Error as E;
    use wire::FailureCode as F;
    let r = wire::ResponseId {
        request: id(1),
        source_generation: 7,
        serial: 99,
    };
    for request in [
        wire::HttpRequest::Credit {
            response: r,
            offset: 0,
            max_bytes: 0,
            allow_large: false,
        },
        wire::HttpRequest::Reuse { response: r },
        wire::HttpRequest::Release { response: r },
    ] {
        let reply = rejection_reply(&request, &E::Invalid("SECRET"));
        assert!(
            matches!(reply,wire::HttpReply::CommandRejected{id:i,source_generation:7,command,response:Some(actual),..} if i==id(1)&&actual==r&&command==request.command_kind())
        );
        reply.frame().unwrap();
    }
    let mut zero = r;
    zero.serial = 0;
    assert!(matches!(
        rejection_reply(
            &wire::HttpRequest::Credit {
                response: zero,
                offset: 0,
                max_bytes: 0,
                allow_large: false
            },
            &E::Invalid("SECRET")
        ),
        wire::HttpReply::CommandRejected { response: None, .. }
    ));
    let mut secret = begin(1, "http://SECRET.invalid/", b"SECRET");
    if let wire::HttpRequest::Begin { headers, .. } = &mut secret {
        headers.push(wire::Header {
            name: "authorization".into(),
            value: b"SECRET".to_vec(),
        });
    }
    for (error, code) in [
        (E::Closed, F::Closed),
        (E::Stale, F::Stale),
        (E::Capacity, F::Capacity),
        (E::InProgress, F::InProgress),
        (E::AlreadyUsed, F::AlreadyUsed),
        (E::DuplicateMismatch, F::DuplicateMismatch),
        (E::Invalid("SECRET"), F::Invalid),
        (E::Quota("SECRET"), F::Quota),
        (E::Network, F::Network),
        (E::Timeout, F::Timeout),
        (E::Cancelled, F::Cancelled),
        (E::UnsupportedScheme, F::UnsupportedScheme),
        (E::UnsupportedUpgrade, F::UnsupportedUpgrade),
        (E::UnsupportedStreaming, F::UnsupportedStreaming),
        (E::UnsupportedEncoding, F::UnsupportedEncoding),
        (E::Worker, F::Worker),
    ] {
        let rejection = rejection_reply(&secret, &error);
        assert!(
            matches!(rejection,wire::HttpReply::CommandRejected{code:c,command:wire::CommandKind::Begin,response:None,..} if c==code)
        );
        let serialized = rejection.frame().unwrap().payload;
        assert!(!std::str::from_utf8(&serialized).unwrap().contains("SECRET"));
    }
}
#[tokio::test]
async fn incomplete_commit_is_rejected_without_ending_the_upload() {
    let f = Fixture::new(1, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    handle(&mut s, &t, begin(1, &url, b"a")).unwrap();
    reply(&mut s, &t).await;
    assert!(matches!(
        rejected(
            &mut s,
            &t,
            wire::HttpRequest::Commit {
                id: id(1),
                source_generation: 7
            }
        ),
        wire::HttpReply::CommandRejected {
            command: wire::CommandKind::Commit,
            code: wire::FailureCode::Invalid,
            ..
        }
    ));
    empty_poll(&mut s, &t);
    assert_eq!(f.count(), 0);
    handle(
        &mut s,
        &t,
        wire::HttpRequest::Upload {
            id: id(1),
            source_generation: 7,
            offset: 0,
            data_b64: encode(b"a"),
        },
    )
    .unwrap();
    handle(
        &mut s,
        &t,
        wire::HttpRequest::Commit {
            id: id(1),
            source_generation: 7,
        },
    )
    .unwrap();
    let (_, _, _, _) = collect(&mut s, &t).await;
    assert_eq!(f.count(), 1);
    assert_eq!(f.body(), b"a");
    s.retire(&t);
    wait_workers(&b).await;
}
