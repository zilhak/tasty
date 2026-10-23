use super::*;
#[tokio::test]
async fn authoritative_owner_generation_entry_policy_and_frame_bounds() {
    let f = Fixture::new(0, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    assert!(handle(&mut s, &t, begin(2, &url, &[])).is_err());
    assert!(handle(&mut s, &t, begin(1, "http://untrusted.invalid/", &[])).is_err());
    let mut changed = begin(1, &url, &[]);
    if let wire::HttpRequest::Begin {
        allow_remote_content,
        ..
    } = &mut changed
    {
        *allow_remote_content = true;
    }
    assert!(handle(&mut s, &t, changed).is_err());
    let mut stale = begin(1, &url, &[]);
    if let wire::HttpRequest::Begin {
        source_generation, ..
    } = &mut stale
    {
        *source_generation = 8;
    }
    assert_eq!(handle(&mut s, &t, stale), Err(core::Error::Stale));
    let (_, other, _) = setup(&url);
    assert_eq!(
        handle(&mut s, &other, begin(1, &url, &[])),
        Err(core::Error::Stale)
    );
    handle(&mut s, &t, begin(1, &url, &[])).unwrap();
    assert!(
        s.poll(&t, |_| true, MAX_FRAME_LEN as usize + 4)
            .unwrap()
            .is_none()
    );
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::UploadReady { .. }
    ));
    assert_eq!(f.count(), 0);
    s.retire(&t);
    wait_workers(&b).await;
}
#[tokio::test]
async fn failed_upload_sha_is_retired_and_cannot_start_post() {
    let f = Fixture::new(0, false);
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
    assert!(
        handle(
            &mut s,
            &t,
            wire::HttpRequest::Commit {
                id: id(1),
                source_generation: 7
            }
        )
        .is_err()
    );
    assert_eq!(
        handle(&mut s, &t, begin(1, &url, b"good")),
        Err(core::Error::AlreadyUsed)
    );
    assert_eq!(
        handle(&mut s, &t, begin(1, &url, b"changed")),
        Err(core::Error::DuplicateMismatch)
    );
    assert_eq!(f.count(), 0);
    assert_eq!(b.retained_payload_bytes(), 0);
}
#[tokio::test]
async fn upload_and_document_slots_are_bounded_before_allocation() {
    let f = Fixture::new(0, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    let mut huge = begin(1, &url, &[]);
    if let wire::HttpRequest::Begin { upload, .. } = &mut huge {
        upload.bytes = 1_000_001;
    }
    assert_eq!(
        handle(&mut s, &t, huge),
        Err(core::Error::Quota("upload bytes"))
    );
    assert_eq!(b.retained_payload_bytes(), 0);
    for n in 1..=16 {
        handle(&mut s, &t, begin(n, &url, &[])).unwrap();
    }
    assert_eq!(
        handle(&mut s, &t, begin(17, &url, &[])),
        Err(core::Error::Capacity)
    );
    let capture = || Capture {
        entry: id(1),
        source_generation: 7,
        entry_url: url.clone(),
        allow_remote_content: false,
    };
    s.admit(capture()).unwrap();
    assert!(matches!(s.admit(capture()), Err(core::Error::Capacity)));
    assert_eq!(f.count(), 0);
}
#[tokio::test]
async fn cancellation_and_authority_loss_discard_queued_native_data() {
    let f = Fixture::new(300000, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    start(&mut s, &t, 1, &url, &[]).await;
    let wire::HttpReply::Head { head } = reply(&mut s, &t).await else {
        panic!("head")
    };
    let mut wrong = head.id;
    wrong.serial += 1;
    assert_eq!(grant(&mut s, &t, wrong, 0, true), Err(core::Error::Stale));
    grant(&mut s, &t, head.id, 0, true).unwrap();
    assert_eq!(
        grant(&mut s, &t, head.id, 0, true),
        Err(core::Error::InProgress)
    );
    handle(
        &mut s,
        &t,
        wire::HttpRequest::Cancel {
            id: id(1),
            source_generation: 7,
        },
    )
    .unwrap();
    assert!(matches!(
        reply(&mut s, &t).await,
        wire::HttpReply::Cancelled { .. }
    ));
    assert!(
        s.poll(&t, |_| true, MAX_FRAME_LEN as usize + 5)
            .unwrap()
            .is_none()
    );
    assert!(grant(&mut s, &t, head.id, 0, true).is_err());
    wait_workers(&b).await;
    assert_eq!(
        handle(&mut s, &t, begin(1, &url, &[])),
        Err(core::Error::AlreadyUsed)
    );
    assert!(matches!(
        s.poll(&t, |_| false, MAX_FRAME_LEN as usize + 5),
        Err(core::Error::Stale)
    ));
    assert!(matches!(s.transferred_bytes(&t), Err(core::Error::Stale)));
}
#[tokio::test]
async fn current_owner_is_revalidated_even_when_outbound_budget_is_zero() {
    let f = Fixture::new(0, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    handle(&mut s, &t, begin(1, &url, b"pending")).unwrap();
    assert!(matches!(s.poll(&t, |_| false, 0), Err(core::Error::Stale)));
    assert_eq!(
        handle(
            &mut s,
            &t,
            wire::HttpRequest::Commit {
                id: id(1),
                source_generation: 7
            }
        ),
        Err(core::Error::Stale)
    );
    wait_workers(&b).await;
    assert_eq!(f.count(), 0);
    assert_eq!(b.retained_payload_bytes(), 0);
}
#[test]
fn terminal_error_mapping_is_exhaustive_and_never_echoes_sensitive_details() {
    use super::core::Error as E;
    use wire::FailureCode as F;
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
        let reply = failure_reply(id(1), 7, None, &error);
        if let wire::HttpReply::Failure {
            code: actual,
            reason,
            ..
        } = reply
        {
            assert_eq!(actual, code);
            assert!(!reason.contains("SECRET"));
        } else {
            panic!("failure");
        }
    }
    for bytes in [
        &b""[..],
        &b"a"[..],
        &b"ab"[..],
        &b"abc"[..],
        &[0, 255, 128][..],
    ] {
        assert_eq!(wire::decode_chunk(&encode(bytes)).unwrap(), bytes);
    }
}
#[tokio::test]
async fn gzip_wire_is_encoded_only_and_complete_binds_both_hashes() {
    let f = Fixture::gzip(200000);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    start(&mut s, &t, 1, &url, &[]).await;
    let (_, encoded, decoded, asked) = collect(&mut s, &t).await;
    assert!(encoded.len() < 100000);
    assert_eq!(asked, 0);
    assert_eq!(decoded.bytes, 200000);
    assert_eq!(
        decoded.sha256,
        <[u8; 32]>::from(Sha256::digest(vec![b'x'; 200000]))
    );
    assert_eq!(s.transferred_bytes(&t).unwrap(), encoded.len() as u64);
    assert_eq!(f.count(), 1);
    s.retire(&t);
    wait_workers(&b).await;
    assert_eq!(b.retained_payload_bytes(), 0);
}
#[tokio::test]
async fn all_response_identity_fields_are_checked_before_credit_or_release() {
    let f = Fixture::new(300000, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    start(&mut s, &t, 1, &url, &[]).await;
    let wire::HttpReply::Head { head } = reply(&mut s, &t).await else {
        panic!("head")
    };
    for field in 0..8 {
        let mut r = head.id;
        match field {
            0 => r.request.epoch += 1,
            1 => r.request.surface_id += 1,
            2 => r.request.binding += 1,
            3 => r.request.document += 1,
            4 => r.request.request += 1,
            5 => r.request.transfer += 1,
            6 => r.source_generation += 1,
            _ => r.serial += 1,
        };
        assert_eq!(grant(&mut s, &t, r, 0, true), Err(core::Error::Stale));
        assert!(handle(&mut s, &t, wire::HttpRequest::Release { response: r }).is_err());
    }
    // Known large responses require their own Allow; this is not aggregate Continue.
    assert!(grant(&mut s, &t, head.id, 0, false).is_err());
    assert_eq!(s.transferred_bytes(&t).unwrap(), 0);
    s.retire(&t);
    wait_workers(&b).await;
}
#[tokio::test]
async fn duplicate_begin_normalization_matches_the_public_executor() {
    let f = Fixture::new(300000, false);
    let url = f.url();
    let (mut s, t, b) = setup(&url);
    let original = begin(1, &url, &[]);
    handle(&mut s, &t, original.clone()).unwrap();
    let doc = b
        .document(
            core::DocumentKey {
                epoch: 1,
                surface_id: 2,
                binding: 3,
                document: 4,
                source_generation: 7,
            },
            &url,
            false,
        )
        .unwrap();
    let core_request = |v: &wire::HttpRequest| {
        let wire::HttpRequest::Begin {
            method,
            logical_url,
            headers,
            ..
        } = v
        else {
            panic!("begin")
        };
        core::Request {
            id: id(1),
            source_generation: 7,
            method: method.clone(),
            url: logical_url.clone(),
            headers: headers
                .iter()
                .map(|h| core::Header {
                    name: h.name.clone(),
                    value: h.value.clone(),
                })
                .collect(),
            upload: b.upload().finish(),
        }
    };
    let core::Opened::Fresh(op) = doc.open(core_request(&original)).unwrap() else {
        panic!("fresh")
    };
    let mut equivalent = original.clone();
    if let wire::HttpRequest::Begin {
        logical_url,
        headers,
        ..
    } = &mut equivalent
    {
        logical_url.push_str("#fragment");
        *headers = vec![
            wire::Header {
                name: "Host".into(),
                value: b"ignored.invalid".to_vec(),
            },
            wire::Header {
                name: "Content-Length".into(),
                value: b"0".to_vec(),
            },
            wire::Header {
                name: "Connection".into(),
                value: b"x-local".to_vec(),
            },
            wire::Header {
                name: "X-Local".into(),
                value: b"discarded".to_vec(),
            },
        ];
    }
    assert!(matches!(
        doc.open(core_request(&equivalent)),
        Err(core::Error::InProgress)
    ));
    assert_eq!(handle(&mut s, &t, equivalent), Err(core::Error::InProgress));
    let mut mismatch = original.clone();
    if let wire::HttpRequest::Begin { headers, .. } = &mut mismatch {
        headers.push(wire::Header {
            name: "cookie".into(),
            value: b"secret".to_vec(),
        });
    }
    assert!(matches!(
        doc.open(core_request(&mismatch)),
        Err(core::Error::DuplicateMismatch)
    ));
    assert_eq!(
        handle(&mut s, &t, mismatch),
        Err(core::Error::DuplicateMismatch)
    );
    op.cancel();
    assert!(matches!(op.join().await, Err(core::Error::Cancelled)));
    assert!(matches!(
        doc.open(core_request(&original)),
        Err(core::Error::AlreadyUsed)
    ));
    handle(
        &mut s,
        &t,
        wire::HttpRequest::Cancel {
            id: id(1),
            source_generation: 7,
        },
    )
    .unwrap();
    assert_eq!(handle(&mut s, &t, original), Err(core::Error::AlreadyUsed));
    s.retire(&t);
    wait_workers(&b).await;
}
