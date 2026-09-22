//! `focus_after_register` — 창 등록이 `focused_view_id` 를 옮기는지는 발화 주체가 정한다.

use winit::window::WindowId;

use super::focus_after_register;
use crate::app::event::WindowRequestOrigin;

const USER_WINDOW: u64 = 1;
const NEW_WINDOW: u64 = 2;

#[test]
fn an_agent_window_leaves_the_focused_window_alone() {
    let focused = Some(WindowId::from(USER_WINDOW));
    let after = focus_after_register(
        focused,
        WindowId::from(NEW_WINDOW),
        WindowRequestOrigin::Agent,
    );
    assert_eq!(after, focused);
}

#[test]
fn a_user_window_takes_the_focus() {
    let after = focus_after_register(
        Some(WindowId::from(USER_WINDOW)),
        WindowId::from(NEW_WINDOW),
        WindowRequestOrigin::User,
    );
    assert_eq!(after, Some(WindowId::from(NEW_WINDOW)));
}

#[test]
fn an_agent_window_takes_the_focus_when_no_window_had_it() {
    let after = focus_after_register(None, WindowId::from(NEW_WINDOW), WindowRequestOrigin::Agent);
    assert_eq!(after, Some(WindowId::from(NEW_WINDOW)));
}
