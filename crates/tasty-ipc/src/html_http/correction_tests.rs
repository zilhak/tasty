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
fn rejected(reason: String) -> HttpReply {
    HttpReply::CommandRejected {
        id: id(),
        source_generation: 7,
        command: CommandKind::Credit,
        response: Some(response()),
        code: FailureCode::Invalid,
        reason,
    }
}
#[test]
fn command_kind_tags_cover_all_requests_without_inspecting_payloads() {
    let identity = BodyIdentity {
        bytes: 0,
        sha256: [0; 32],
    };
    let requests = [
        HttpRequest::Begin {
            id: id(),
            source_generation: 7,
            method: "POST".into(),
            logical_url: "http://example.test/".into(),
            headers: vec![],
            upload: identity,
            allow_remote_content: false,
        },
        HttpRequest::Upload {
            id: id(),
            source_generation: 7,
            offset: 0,
            data_b64: String::new(),
        },
        HttpRequest::Commit {
            id: id(),
            source_generation: 7,
        },
        HttpRequest::Credit {
            response: response(),
            offset: 0,
            max_bytes: 1,
            allow_large: false,
        },
        HttpRequest::Cancel {
            id: id(),
            source_generation: 7,
        },
        HttpRequest::Reuse {
            response: response(),
        },
        HttpRequest::Release {
            response: response(),
        },
        HttpRequest::Stop {
            entry: id(),
            source_generation: 7,
        },
        HttpRequest::Close {
            entry: id(),
            source_generation: 7,
        },
    ];
    let kinds = [
        CommandKind::Begin,
        CommandKind::Upload,
        CommandKind::Commit,
        CommandKind::Credit,
        CommandKind::Cancel,
        CommandKind::Reuse,
        CommandKind::Release,
        CommandKind::Stop,
        CommandKind::Close,
    ];
    let tags = [
        "begin", "upload", "commit", "credit", "cancel", "reuse", "release", "stop", "close",
    ];
    for ((request, kind), tag) in requests.into_iter().zip(kinds).zip(tags) {
        assert_eq!(request.command_kind(), kind);
        let value = serde_json::to_value(kind).unwrap();
        assert_eq!(value, tag);
        assert_eq!(serde_json::from_value::<CommandKind>(value).unwrap(), kind);
    }
}
#[test]
fn command_rejection_roundtrip_identity_and_reason_bounds() {
    let reply = rejected("fixed local reason".into());
    let frame = reply.frame().unwrap();
    assert_eq!(reply.id(), id());
    let value: serde_json::Value = serde_json::from_slice(&frame.payload).unwrap();
    assert_eq!(value["event"], "html_http_command_rejected");
    assert_eq!(value["command"], "credit");
    assert!(serde_json::from_slice::<HttpReply>(&frame.payload).unwrap() == reply);
    for reason in ["x".repeat(REASON_BYTES + 1), "unsafe\nreason".into()] {
        assert!(rejected(reason).frame().is_err());
    }
    let mut wrong = response();
    wrong.source_generation += 1;
    let mut value = rejected("fixed".into());
    if let HttpReply::CommandRejected { response, .. } = &mut value {
        *response = Some(wrong);
    }
    assert!(value.validate().is_err());
    wrong = response();
    wrong.request.binding += 1;
    if let HttpReply::CommandRejected { response, .. } = &mut value {
        *response = Some(wrong);
    }
    assert!(value.validate().is_err());
    wrong = response();
    wrong.serial = 0;
    if let HttpReply::CommandRejected { response, .. } = &mut value {
        *response = Some(wrong);
    }
    assert!(value.validate().is_err());
}
#[test]
fn retained_capacity_counts_spare_header_slots_and_initialized_buffers() {
    let mut url = String::with_capacity(1024);
    url.push_str("http://example.test/");
    let mut name = String::with_capacity(128);
    name.push('x');
    let mut bytes = Vec::with_capacity(256);
    bytes.push(255);
    let mut headers = Vec::with_capacity(9);
    headers.push(Header { name, value: bytes });
    let expected = url.capacity()
        + headers.capacity() * std::mem::size_of::<Header>()
        + headers[0].name.capacity()
        + headers[0].value.capacity();
    let reply = HttpReply::Head {
        head: ResponseHead {
            id: response(),
            logical_url: url,
            status: 200,
            headers,
            encoded_length: None,
            has_body: true,
        },
    };
    assert_eq!(reply.retained_bytes(), expected);
    assert_eq!(reply.retained_bytes(), expected);
    let HttpReply::Head { head } = &reply else {
        panic!("head")
    };
    assert_eq!(head.headers.len(), 1);
    assert!(head.headers.capacity() > head.headers.len());
}
#[test]
fn retained_capacity_is_nonvalidating_and_accounts_each_dynamic_variant() {
    fn reserved() -> String {
        let mut s = String::with_capacity(4096);
        s.push_str("not canonical base64 or a short bounded reason");
        s
    }
    let data = reserved();
    let cap = data.capacity();
    let chunk = HttpReply::Chunk {
        response: response(),
        offset: 0,
        data_b64: data,
    };
    assert!(chunk.validate().is_err());
    assert_eq!(chunk.retained_bytes(), cap);
    let reason = reserved();
    let cap = reason.capacity();
    let failure = HttpReply::Failure {
        id: id(),
        source_generation: 7,
        response: None,
        code: FailureCode::Invalid,
        reason,
    };
    assert_eq!(failure.retained_bytes(), cap);
    let reason = reserved();
    let cap = reason.capacity();
    assert_eq!(rejected(reason).retained_bytes(), cap);
    let identity = BodyIdentity {
        bytes: 0,
        sha256: [0; 32],
    };
    for reply in [
        HttpReply::UploadReady {
            id: id(),
            source_generation: 7,
            offset: 0,
            max_bytes: 1,
        },
        HttpReply::AllowanceRequired {
            response: response(),
            encoded_at_least: 100001,
            known_length: None,
        },
        HttpReply::Complete {
            response: response(),
            encoded: identity,
            decoded: identity,
        },
        HttpReply::Cancelled {
            id: id(),
            source_generation: 7,
            response: Some(response()),
        },
    ] {
        assert_eq!(reply.retained_bytes(), 0);
    }
}
