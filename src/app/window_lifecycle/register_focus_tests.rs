//! 창 등록 후 포커스는 요청 주체에 따라 결정한다.

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

#[test]
fn an_agent_window_is_created_hidden_and_inactive() {
    let attrs = super::origin_window_attributes(
        winit::window::WindowAttributes::default(),
        WindowRequestOrigin::Agent,
    );
    assert!(!attrs.visible);
    assert!(!attrs.active);
}

#[test]
fn a_user_window_is_created_visible_and_active() {
    let attrs = super::origin_window_attributes(
        winit::window::WindowAttributes::default(),
        WindowRequestOrigin::User,
    );
    assert!(attrs.visible);
    assert!(attrs.active);
}
