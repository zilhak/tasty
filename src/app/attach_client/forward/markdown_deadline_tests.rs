//! 보낸 원문 요청이 마감을 기다리고, 원문 요청 처리 회차가 마감을 지난 요청을 끝내는 것을 실제 `App` 으로 잰다.
//! `App` 을 만드는 데 X11 디스플레이가 필요하고 winit `EventLoop` 는 프로세스에 하나뿐이라 새 프로세스에서 혼자 돈다.

use super::*;
use crate::app::attach_client::tests::test_session_with_frames;
use std::time::{Duration, Instant};
use tasty_remote::markdown_requests::MARKDOWN_CONTENT_REPLY_TIMEOUT;

const LOCAL: u32 = 300;

fn app_on(event_loop: &winit::event_loop::EventLoop<crate::AppEvent>) -> App {
    crate::app::preset_shutdown_tests::make_gui(event_loop.create_proxy())
}

fn request(request_id: u64) -> crate::core::PendingMarkdownContentForward {
    crate::core::PendingMarkdownContentForward {
        user_wait: false,
        admitted: Instant::now(),
        local_surface_id: LOCAL,
        request_id,
        agent_origin: false,
    }
}

/// Given mirror markdown 을 든 연결된 세션, When 원문 요청을 보내면, Then 그 요청은 회신을 기다리는 요청이고,
/// 마감을 지난 요청은 다음 원문 요청 처리 회차에 끝난다.
#[test]
fn a_sent_request_waits_and_an_overdue_one_ends_in_the_next_round() {
    if crate::test_support::rerun_alone(
        module_path!(),
        "a_sent_request_waits_and_an_overdue_one_ends_in_the_next_round",
    ) {
        return;
    }
    let _home = crate::test_support::IsolatedHome::new();
    let event_loop = crate::x11_test_loop::x11_event_loop();
    let mut app = app_on(&event_loop);
    let (mut session, frames) = test_session_with_frames(9_000, HashMap::from([(30, LOCAL)]));
    session.state.markdown_locals.insert(LOCAL);
    let overdue = Instant::now()
        .checked_sub(MARKDOWN_CONTENT_REPLY_TIMEOUT + Duration::from_secs(1))
        .expect("the clock reaches back past one deadline");
    session.state.markdown_requests.note(4, LOCAL, overdue);
    app.remote.sessions.push(session);

    app.send_markdown_content_request(&request(5))
        .expect("the connected session takes the request");
    assert!(frames.try_recv().is_ok(), "the request went out");
    // 받는 쪽을 버린 세션이라 그 문서로 가는 요청은 보내지 못한다.
    let mut closed =
        crate::app::attach_client::tests::test_session(9_001, HashMap::from([(31, LOCAL + 1)]));
    closed.state.markdown_locals.insert(LOCAL + 1);
    app.remote.sessions.push(closed);
    let unsent = crate::core::PendingMarkdownContentForward {
        local_surface_id: LOCAL + 1,
        ..request(6)
    };
    assert!(
        app.send_markdown_content_request(&unsent).is_err(),
        "the session's sender is closed"
    );
    app.dispatch_pending_markdown_content_forwards();

    let requests = &mut app.remote.sessions[0].state.markdown_requests;
    assert!(
        !requests.answer(4),
        "the overdue request no longer waits for its reply"
    );
    assert!(
        requests.answer(5),
        "the request just sent waits for its reply"
    );
    let unsent_requests = &mut app.remote.sessions[1].state.markdown_requests;
    assert!(
        !unsent_requests.answer(6),
        "a request that was not sent does not wait; it already failed"
    );
}
