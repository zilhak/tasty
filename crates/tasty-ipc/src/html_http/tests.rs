use super::*;
fn id() -> TransferId {
    TransferId {
        epoch: 1,
        surface_id: 2,
        binding: 3,
        document: 4,
        request: 5,
        transfer: 6,
    }
}
fn response() -> ResponseId {
    ResponseId {
        request: id(),
        source_generation: 7,
        serial: 8,
    }
}
#[test]
fn ordered_binary_headers_and_full_identity_roundtrip() {
    let reply = HttpReply::Head {
        head: ResponseHead {
            id: response(),
            logical_url: "https://example.test/".into(),
            status: 200,
            headers: vec![
                Header {
                    name: "set-cookie".into(),
                    value: vec![128, 255],
                },
                Header {
                    name: "set-cookie".into(),
                    value: b"b=c".to_vec(),
                },
            ],
            encoded_length: None,
            has_body: true,
        },
    };
    let frame = reply.frame().unwrap();
    let round: HttpReply = serde_json::from_slice(&frame.payload).unwrap();
    assert!(round == reply);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&frame.payload).unwrap()["event"],
        "html_http_head"
    );
}
#[test]
fn malformed_headers_and_metadata_fail_closed() {
    let mut request = HttpRequest::Begin {
        id: id(),
        source_generation: 7,
        method: "POST".into(),
        logical_url: "https://example.test".into(),
        headers: vec![],
        upload: BodyIdentity {
            bytes: 0,
            sha256: [0; 32],
        },
        allow_remote_content: false,
    };
    assert!(request.validate().is_ok());
    if let HttpRequest::Begin { headers, .. } = &mut request {
        headers.push(Header {
            name: "Cookie".into(),
            value: b"secret\r\ninjected: yes".to_vec(),
        });
    }
    assert_eq!(request.validate(), Err("HTTP header bounds or syntax"));
    if let HttpRequest::Begin { headers, .. } = &mut request {
        *headers = vec![Header {
            name: "a".into(),
            value: vec![b'x'; HEADER_BYTES],
        }];
    }
    assert!(request.validate().is_err());
}
#[test]
fn strict_base64_and_decoded_window() {
    assert_eq!(decode_chunk("AP+A").unwrap(), [0, 255, 128]);
    assert_eq!(decode_chunk("YQ==").unwrap(), b"a");
    assert!(decode_chunk("").unwrap().is_empty());
    for bad in ["YR==", "YQ=+", "YQ==YQ==", "Zg=", "!!!!", "Zm9="] {
        assert!(decode_chunk(bad).is_err(), "{bad}");
    }
    assert_eq!(
        decode_chunk(&"AAAA".repeat(CHUNK_BYTES / 3)).unwrap().len(),
        CHUNK_BYTES
    );
    assert!(decode_chunk(&"AAAA".repeat(CHUNK_BYTES / 3 + 1)).is_err());
}
#[test]
fn checked_windows_and_optional_identity() {
    let mut r = response();
    r.serial = 0;
    assert!(
        HttpRequest::Credit {
            response: r,
            offset: 0,
            max_bytes: 1,
            allow_large: false
        }
        .validate()
        .is_err()
    );
    for (offset, max_bytes) in [(0, 0), (0, 49153), (u64::MAX, 1)] {
        assert!(
            HttpRequest::Credit {
                response: response(),
                offset,
                max_bytes,
                allow_large: false
            }
            .validate()
            .is_err()
        );
    }
    let mut r = response();
    r.source_generation += 1;
    assert!(
        HttpReply::Cancelled {
            id: id(),
            source_generation: 7,
            response: Some(r)
        }
        .validate()
        .is_err()
    );
}
#[test]
fn complete_envelope_and_reason_are_bounded() {
    let reply = HttpReply::Failure {
        id: id(),
        source_generation: 7,
        response: None,
        code: FailureCode::Invalid,
        reason: "x".repeat(REASON_BYTES + 1),
    };
    assert!(reply.frame().is_err());
    // Escape expansion is included by the same frame helper used by both directions.
    assert!(frame(&"\"".repeat(MAX_FRAME_LEN as usize / 2)).is_err());
    assert_eq!(crate::stream::STREAM_PROTO, 1);
}

#[test]
fn explicit_reload_envelope_is_finite_and_refuses_cross_scope_or_nonmonotone_lineage() {
    let old = response();
    let mut next = id();
    next.document += 1;
    next.request += 1;
    next.transfer += 1;
    let begin = HttpRequest::Begin {
        id: next,
        source_generation: 7,
        method: "POST".into(),
        logical_url: "https://example.test/final".into(),
        headers: vec![],
        upload: BodyIdentity {
            bytes: 3,
            sha256: [42; 32],
        },
        allow_remote_content: false,
    };
    let reload = begin.with_predecessor(old).unwrap();
    let frame = reload.frame().unwrap();
    assert!(frame.payload.len() <= MAX_FRAME_LEN as usize);
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&frame.payload).unwrap()["event"],
        "html_http_reload"
    );
    let round: HttpRequest = serde_json::from_slice(&frame.payload).unwrap();
    assert!(round == reload);
    for mode in 0..8 {
        let mut bad = reload.clone();
        let HttpRequest::Reload { previous, begin } = &mut bad else {
            panic!("reload")
        };
        match mode {
            0 => previous.serial = 0,
            1 => begin.id.epoch += 1,
            2 => begin.id.surface_id += 1,
            3 => begin.id.binding += 1,
            4 => begin.source_generation += 1,
            5 => begin.id.document = old.request.document,
            6 => begin.id.request = old.request.request,
            7 => begin.logical_url = "x".repeat(URL_BYTES + 1),
            _ => unreachable!(),
        }
        assert!(bad.validate().is_err(), "mode={mode}");
    }
}
