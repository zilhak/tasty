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
    }
}

#[test]
fn id_generate_format() {
    let id = ApprovalId::generate();
    assert!(id.as_str().starts_with("req_"));
    assert_eq!(id.as_str().len(), 4 + 12);
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

/// 락이 poison 되면 **상태 변경은 거절**하고 **읽기는 계속 된다**.
///
/// 두 갈래인 이유는 `error-handling.md` "락 poison" 의 두 질문에 답이 다르기 때문이다.
/// `respond` 의 임계구역은 `state` → `history` → `waiters` 를 순서대로 갱신하므로
/// 중간 상태가 남을 수 있고, 승인은 에이전트 행동의 관문이라 그 위에서 진행하면 안 된다.
/// 반면 `get`/`list` 는 표시용 읽기이고 호출자에게 에러 채널도 없다.
///
/// 어느 쪽이든 **패닉하지 않는 것**이 핵심이다 — 이 store 는 승인 popup(메인 스레드)이
/// 함께 쓰므로, 패닉하면 실행 중인 모든 창의 터미널 세션이 사라진다.
#[test]
fn a_poisoned_store_refuses_writes_but_still_answers_reads() {
    let store = ApprovalStore::new();
    store
        .request(req("a1", Requester::User))
        .expect("fresh store accepts the request");

    // 락을 든 채 패닉시켜 poison 을 만든다.
    let inner = Arc::clone(&store.inner);
    let joined = thread::spawn(move || {
        let _guard = inner.lock().expect("fresh mutex");
        panic!("a thread dies while holding the approval store");
    })
    .join();
    assert!(joined.is_err(), "그 스레드는 패닉했어야 한다");

    // 읽기 — 복구해서 답한다.
    assert!(store.get(&ApprovalId("a1".to_string())).is_some());
    assert_eq!(store.list().len(), 1);

    // 상태 변경 — 거절한다(패닉이 아니라 에러로).
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
