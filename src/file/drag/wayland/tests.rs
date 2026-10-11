//! Uses an explicitly owned compositor; never connects to the user's display.
use super::*;
use winit::{event_loop::EventLoop, platform::wayland::EventLoopBuilderExtWayland};

#[test]
#[ignore = "requires TASTY_TEST_WAYLAND_DISPLAY on an isolated compositor"]
fn idle_context_does_not_bind_a_data_device() {
    let display = std::env::var("TASTY_TEST_WAYLAND_DISPLAY").expect("owned display required");
    assert_eq!(std::env::var("WAYLAND_DISPLAY").unwrap(), display);
    assert!(std::env::var_os("DISPLAY").is_none());
    let events = EventLoop::builder()
        .with_wayland()
        .with_any_thread(true)
        .build()
        .unwrap();
    // reason: This fixture creates no event handler and only inspects the initial context.
    #[allow(deprecated)]
    let window = Arc::new(events.create_window(Window::default_attributes()).unwrap());
    let mut context = Context::new(window, Some(events.owned_display_handle()))
        .unwrap()
        .unwrap();
    context.poll().unwrap();
    let owner = context.0.borrow();
    assert!(owner.state.manager.is_some());
    assert!(!owner.state.seats.is_empty());
    assert!(owner.state.seats.iter().any(|seat| seat.pointer.is_some()));
    assert!(owner.state.seats.iter().all(|seat| seat.device.is_none()));
    assert!(owner.state.active.is_none());
}
