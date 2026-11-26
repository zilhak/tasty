use super::*;
use crate::app::attach_client::tests::test_session;
use crate::state::attach_pending::{Query, QueryConsumer};
use std::collections::HashMap;

const LOCAL: u32 = 300;

/// 마감이 지난 요청 하나(request 4)와 아직 기다리는 요청 하나(request 5)를 든 연결된 세션.
fn session_with_requests(t0: Instant) -> AttachClientSession {
    let mut sess = test_session(9_000, HashMap::from([(30, LOCAL)]));
    sess.state.markdown_locals.insert(LOCAL);
    sess.state.agent_requests.note_markdown(true, 4);
    sess.state.markdown_requests.note(4, LOCAL, t0);
    sess.state
        .markdown_requests
        .note(5, LOCAL, t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT / 2);
    sess
}

/// Given 원격이 답하지 않는 원문 요청, When 마감이 지나면, Then 그 요청만 plugin 에 같은 request_id 의
/// 실패로 끝난다.
#[test]
fn an_unanswered_request_ends_as_a_failure_for_its_own_request_id() {
    let t0 = Instant::now();
    let mut sessions = vec![session_with_requests(t0)];
    let before = take_expired(&mut sessions, t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT / 4);
    assert!(before.is_empty(), "nothing fails before its deadline");
    let expired = take_expired(&mut sessions, t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT);
    let failures = settle_expired(&expired, std::iter::empty());
    assert_eq!(failures.len(), 1, "only the request past its deadline ends");
    let failure = &failures[0];
    assert_eq!(failure["surface_id"], LOCAL);
    assert_eq!(failure["request_id"], 4);
    assert_eq!(failure["ok"], false);
    assert!(
        failure["reason"]
            .as_str()
            .is_some_and(|reason| !reason.is_empty()),
        "the plugin shows the reason under its failed state: {failure}"
    );
    assert!(
        sessions[0].state.markdown_requests.answer(5),
        "the request still within its deadline keeps waiting"
    );
    assert!(
        !sessions[0].state.agent_requests.take_markdown(4),
        "the expired agent request leaves no mark that a reply would consume"
    );
}

/// Given 원문을 기다리는 사용자 대기, When 그 요청이 마감으로 끝나면, Then 그 대기만 내려가고 같은 surface 의
/// 다른 요청 대기는 남는다.
#[test]
fn an_expired_request_takes_down_only_its_own_wait() {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let engine = engine_session.borrow_mut();
    let surface = engine
        .workspace_at(0)
        .expect("workspace index is valid")
        .all_surface_ids()[0];
    let target = crate::runtime::surface_binding::SurfaceBinding::capture(&engine.read(), surface)
        .expect("the surface exists");
    let t0 = Instant::now();
    let mut sessions = vec![session_with_requests(t0)];
    let epoch = sessions[0].transport.frame_tx.epoch();
    for request in [4, 5] {
        state.attach_pending.queries.push(Query {
            consumer: QueryConsumer::Markdown(LOCAL),
            request,
            workspace: 9_000,
            admitted: t0,
            epoch: epoch.clone(),
            target: target.clone(),
        });
    }
    let expired = take_expired(&mut sessions, t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT);
    settle_expired(&expired, std::iter::once(&mut state.attach_pending));
    let left: Vec<u64> = state
        .attach_pending
        .queries
        .iter()
        .map(|query| query.request)
        .collect();
    assert_eq!(left, vec![5]);
}

/// Given 끊겨 재연결 중인 세션, When 마감이 지나도, Then 끊김 경로가 이미 취소했으므로 실패를 다시 보내지 않고
/// 기다리던 요청을 버린다.
#[test]
fn a_session_without_a_connection_forgets_its_requests_without_failing_them() {
    let t0 = Instant::now();
    let mut sessions = vec![session_with_requests(t0)];
    sessions[0].state.phase = SessionState::Reconnecting;
    assert!(take_expired(&mut sessions, t0 + MARKDOWN_CONTENT_REPLY_TIMEOUT * 2).is_empty());
    assert!(!sessions[0].state.markdown_requests.answer(4));
    assert!(!sessions[0].state.markdown_requests.answer(5));
}

const OWNER: &str = "com.test.markdown-owner";

/// `LOCAL` 문서를 소유한 plugin stub. 요청 큐는 `capacity` 개만 받는다.
fn owned_by_plugin(
    capacity: usize,
) -> (
    Option<crate::plugin::PluginManager>,
    tasty_host_plugin::manager::CallerStub,
) {
    let mut manager = crate::plugin::PluginManager::with_registries(
        std::sync::Arc::new(tasty_terminal::waker_factory::NoopWakerFactory),
        std::sync::Arc::new(crate::file::format::FileFormatRegistry::new()),
        std::sync::Arc::new(crate::file::handler::FileHandlerRegistry::new()),
    );
    let stub = manager.attach_surface_owner_stub_for_test(OWNER, LOCAL, capacity);
    (Some(manager), stub)
}

/// plugin 이 받은 이벤트를 받은 순서대로 (key, payload) 로 꺼낸다.
fn received(stub: &tasty_host_plugin::manager::CallerStub) -> Vec<(String, Value)> {
    stub.drain_requests()
        .into_iter()
        .map(|(_, params)| {
            let envelope = &params["envelope"];
            let key = envelope["key"].as_str().unwrap_or_default().to_owned();
            (key, envelope["payload"].clone())
        })
        .collect()
}

/// Given 옛 연결로 보낸 원문 요청 둘(4·5)을 든 세션, When 연결이 바뀌면, Then plugin 은 두 요청의 같은
/// request_id 실패를 받은 뒤에 그 문서의 변경 신호를 받는다.
#[test]
fn a_replaced_connection_fails_its_sent_requests_before_the_change_signal() {
    let (mut manager, stub) = owned_by_plugin(16);
    let mut sess = session_with_requests(Instant::now());
    end_replaced_connection_requests(&mut manager, &mut sess.state);
    let events = received(&stub);
    let order: Vec<(&str, Option<u64>)> = events
        .iter()
        .map(|(key, payload)| (key.as_str(), payload["request_id"].as_u64()))
        .collect();
    assert_eq!(
        order,
        vec![
            ("markdown_mirror.content_result", Some(4)),
            ("markdown_mirror.content_result", Some(5)),
            ("markdown_mirror.changed", None),
        ]
    );
    for (_, payload) in &events[..2] {
        assert_eq!(payload["surface_id"], LOCAL);
        assert_eq!(payload["ok"], false, "{payload}");
    }
    assert_eq!(events[2].1["surface_id"], LOCAL);
    assert!(
        !sess.state.markdown_requests.answer(4) && !sess.state.markdown_requests.answer(5),
        "a reply from the old connection no longer answers anything"
    );
}
