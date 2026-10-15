//! `lib_tests` 단위 테스트.

use super::*;
use std::thread;

fn req(id: &str, requester: Requester) -> ApprovalRequest {
    ApprovalRequest {
        id: ApprovalId(id.to_string()),
        requester,
        workspace_id: None,
        surface_id: None,
        title: "test".to_string(),
        body: None,
        choices: Vec::new(),
        default_choice: None,
        timeout_ms: None,
        severity: Severity::Info,
        created_at: 0,
        metadata: serde_json::Value::Null,
        host_elevation: None,
    }
}

#[test]
fn id_generate_format() {
    let id = ApprovalId::generate();
    assert!(id.as_str().starts_with("req_"));
    assert_eq!(id.as_str().len(), 4 + 12);
}

/// 생성 ID는 항상 소문자 표기가 있고, 형식 밖 ID는 대소문자만 다른 ID와 겹칠 수 있어 없다.
#[test]
fn lowercase_token_only_for_the_generated_form() {
    for _ in 0..64 {
        let id = ApprovalId::generate();
        let token = id.lowercase_token().expect("generated ids have a token");
        assert_eq!(token, id.as_str().to_ascii_lowercase());
    }
    assert_eq!(
        ApprovalId("req_0123456789AB".into())
            .lowercase_token()
            .as_deref(),
        Some("req_0123456789ab")
    );
    for other in [
        "req_0123456789ab",
        "req_0123456789A",
        "req_0123456789ABC",
        "REQ_0123456789AB",
        "req_0123456789AI",
        "summary",
        "test-1",
        "",
    ] {
        assert_eq!(ApprovalId(other.into()).lowercase_token(), None, "{other}");
    }
}

#[test]
fn request_default_choices() {
    let s = ApprovalStore::new();
    let ch = s
        .request(req("req_a", Requester::Plugin { id: "x".into() }))
        .unwrap();
    assert_eq!(ch.record.request.choices.len(), 2);
    assert_eq!(ch.record.request.choices[0].key, "approve");
    assert_eq!(ch.record.request.choices[1].key, "deny");
}

#[test]
fn request_duplicate_id_rejected() {
    let s = ApprovalStore::new();
    s.request(req("req_a", Requester::User)).unwrap();
    let e = s.request(req("req_a", Requester::User)).unwrap_err();
    assert!(matches!(e, ApprovalError::InvalidRequest(_)));
}

#[test]
fn respond_resolves_pending_waiter() {
    let s = Arc::new(ApprovalStore::new());
    s.request(req("req_a", Requester::User)).unwrap();
    let s2 = s.clone();
    let h = thread::spawn(move || {
        s2.await_response(&ApprovalId("req_a".into()), Some(2000))
            .unwrap()
    });
    thread::sleep(Duration::from_millis(50));
    s.respond(
        &ApprovalId("req_a".into()),
        "approve".into(),
        Responder::User,
        None,
    )
    .unwrap();
    let out = h.join().unwrap();
    assert!(matches!(out, WaitOutcome::Responded { ref choice, .. } if choice == "approve"));
}

#[test]
fn respond_twice_rejected() {
    let s = ApprovalStore::new();
    s.request(req("req_a", Requester::User)).unwrap();
    s.respond(
        &ApprovalId("req_a".into()),
        "approve".into(),
        Responder::User,
        None,
    )
    .unwrap();
    let e = s
        .respond(
            &ApprovalId("req_a".into()),
            "deny".into(),
            Responder::User,
            None,
        )
        .unwrap_err();
    assert!(matches!(e, ApprovalError::AlreadyResponded(_)));
}

#[test]
fn self_response_blocked() {
    let s = ApprovalStore::new();
    s.request(req("req_a", Requester::Plugin { id: "alice".into() }))
        .unwrap();
    // 같은 caller (Agent.id == Plugin.id) 가 본인 요청에 응답하면 거부.
    let e = s
        .respond(
            &ApprovalId("req_a".into()),
            "approve".into(),
            Responder::Agent { id: "alice".into() },
            None,
        )
        .unwrap_err();
    assert!(matches!(e, ApprovalError::SelfResponse));
}

#[test]
fn user_responder_always_allowed_even_if_requester_user() {
    let s = ApprovalStore::new();
    s.request(req("req_a", Requester::User)).unwrap();
    s.respond(
        &ApprovalId("req_a".into()),
        "approve".into(),
        Responder::User,
        None,
    )
    .unwrap();
}

#[test]
fn timeout_transitions_to_timed_out() {
    let s = ApprovalStore::new();
    let mut r = req("req_a", Requester::User);
    r.default_choice = Some("deny".into());
    s.request(r).unwrap();
    let out = s
        .await_response(&ApprovalId("req_a".into()), Some(60))
        .unwrap();
    assert!(matches!(out, WaitOutcome::TimedOut { default_choice: Some(ref d) } if d == "deny"));
    let rec = s.get(&ApprovalId("req_a".into())).unwrap();
    assert!(matches!(rec.state, ApprovalState::TimedOut { .. }));
}

/// 여러 대기자가 같은 요청의 만료를 기다려도 만료 전이를 적용한 호출 하나만 변경을 받는다.
#[test]
fn only_the_waiter_that_applies_the_timeout_gets_the_change() {
    let s = Arc::new(ApprovalStore::new());
    s.request(req("req_a", Requester::User)).unwrap();
    let waiters: Vec<_> = (0..4)
        .map(|_| {
            let s = s.clone();
            thread::spawn(move || {
                s.await_transition(&ApprovalId("req_a".into()), Some(40))
                    .unwrap()
            })
        })
        .collect();
    let awaited: Vec<Awaited> = waiters.into_iter().map(|h| h.join().unwrap()).collect();
    assert!(
        awaited
            .iter()
            .all(|a| matches!(a.outcome, WaitOutcome::TimedOut { .. }))
    );
    let changes: Vec<_> = awaited.iter().filter_map(|a| a.change.as_ref()).collect();
    assert_eq!(changes.len(), 1);
    assert!(matches!(changes[0].kind, ChangeKind::TimedOut));
    assert!(matches!(
        changes[0].record.state,
        ApprovalState::TimedOut { .. }
    ));
}

/// 응답이 만료보다 먼저 오면 대기자는 변경을 받지 않는다. 기록은 응답한 쪽 몫이다.
#[test]
fn a_waiter_released_by_a_response_has_no_change() {
    let s = Arc::new(ApprovalStore::new());
    s.request(req("req_a", Requester::User)).unwrap();
    let s2 = s.clone();
    let h = thread::spawn(move || {
        s2.await_transition(&ApprovalId("req_a".into()), Some(5000))
            .unwrap()
    });
    thread::sleep(Duration::from_millis(30));
    s.respond(
        &ApprovalId("req_a".into()),
        "approve".into(),
        Responder::User,
        None,
    )
    .unwrap();
    let awaited = h.join().unwrap();
    assert!(matches!(awaited.outcome, WaitOutcome::Responded { .. }));
    assert!(awaited.change.is_none());
    let again = s
        .await_transition(&ApprovalId("req_a".into()), Some(10))
        .unwrap();
    assert!(again.change.is_none());
}

#[test]
fn cancel_resolves_waiters() {
    let s = Arc::new(ApprovalStore::new());
    s.request(req("req_a", Requester::User)).unwrap();
    let s2 = s.clone();
    let h = thread::spawn(move || {
        s2.await_response(&ApprovalId("req_a".into()), Some(2000))
            .unwrap()
    });
    thread::sleep(Duration::from_millis(30));
    s.cancel(&ApprovalId("req_a".into())).unwrap();
    let out = h.join().unwrap();
    assert!(matches!(out, WaitOutcome::Cancelled));
}

#[test]
fn await_on_already_responded() {
    let s = ApprovalStore::new();
    s.request(req("req_a", Requester::User)).unwrap();
    s.respond(
        &ApprovalId("req_a".into()),
        "approve".into(),
        Responder::User,
        None,
    )
    .unwrap();
    let out = s
        .await_response(&ApprovalId("req_a".into()), Some(50))
        .unwrap();
    assert!(matches!(out, WaitOutcome::Responded { ref choice, .. } if choice == "approve"));
}

#[test]
fn invalid_choice_rejected() {
    let s = ApprovalStore::new();
    s.request(req("req_a", Requester::User)).unwrap();
    let e = s
        .respond(
            &ApprovalId("req_a".into()),
            "maybe".into(),
            Responder::User,
            None,
        )
        .unwrap_err();
    assert!(matches!(e, ApprovalError::InvalidChoice(_)));
}

/// poison 이후 표시용 조회는 허용하고 상태 변경은 오류로 거절한다.
#[test]
fn a_poisoned_store_refuses_writes_but_still_answers_reads() {
    let store = ApprovalStore::new();
    store
        .request(req("a1", Requester::User))
        .expect("fresh store accepts the request");

    let inner = Arc::clone(&store.inner);
    let joined = thread::spawn(move || {
        let _guard = inner.lock().expect("fresh mutex");
        panic!("a thread dies while holding the approval store");
    })
    .join();
    assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");

    assert!(store.get(&ApprovalId("a1".to_string())).is_some());
    assert_eq!(store.list().len(), 1);

    let e = store
        .respond(
            &ApprovalId("a1".to_string()),
            "approve".to_string(),
            Responder::User,
            None,
        )
        .unwrap_err();
    assert!(matches!(e, ApprovalError::StorePoisoned), "got {e:?}");

    let e = store.cancel(&ApprovalId("a1".to_string())).unwrap_err();
    assert!(matches!(e, ApprovalError::StorePoisoned), "got {e:?}");

    let e = store.request(req("a2", Requester::User)).unwrap_err();
    assert!(matches!(e, ApprovalError::StorePoisoned), "got {e:?}");
}

/// 호스트가 발행한 권한 상승 요청은 사용자 응답만 받는다. 다른 agent 의 응답은 상태를 바꾸지 않는다.
#[test]
fn host_elevation_accepts_only_the_user() {
    let store = ApprovalStore::new();
    let mut request = req(
        "req_ELEVATION01",
        Requester::Plugin {
            id: "target".into(),
        },
    );
    request.choices = vec![ApprovalChoice::approve(), ApprovalChoice::deny()];
    request.host_elevation = Some(HostElevation {
        agent_id: "target".into(),
        permission: "fs.write".into(),
        grant_ttl_secs: Some(3600),
    });
    let id = store.request(request).unwrap().record.request.id;

    for choice in ["approve", "deny"] {
        let err = store
            .respond(
                &id,
                choice.into(),
                Responder::Agent { id: "other".into() },
                None,
            )
            .unwrap_err();
        assert!(matches!(err, ApprovalError::UserResponseRequired(ref e) if *e == id));
    }
    assert_eq!(store.get(&id).unwrap().state, ApprovalState::Pending);

    store
        .respond(&id, "approve".into(), Responder::User, None)
        .expect("the user answers");
}
