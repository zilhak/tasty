//! `lib_tests` 단위 테스트.

#![cfg(test)]

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
