//! The hold's order, timing and result rules: pointer input behind a settling key, the frame
//! timeout, a paste that leaves the text as it was, a recheck for the owner, and the replay order.
use super::*;

/// Pointer input after a key that waits for earlier input queues behind it, so the earlier input's
/// pass holds only what came before the key and the edit lands where the user was.
#[test]
fn pointer_input_after_a_settling_key_waits() {
    let mut ui = Ui::new();
    ui.focus("field");
    let mut state = bound();
    let now = Instant::now();
    let mut pending = vec![egui::Event::Text("z".into())];
    assert_eq!(
        ui.judge(&mut state, &press("v"), &mut pending, now),
        EguiKey::Taken
    );
    assert_eq!(
        ui.judge(&mut state, &Ev::Pointer("click next"), &mut pending, now),
        EguiKey::Taken,
        "a click after the key waits behind it"
    );
    assert_eq!(pending.len(), 1, "the earlier input runs alone");
    ui.pass(&mut state, pending);
    assert!(ui.field.ends_with("zCLIP"), "pasted after z: {}", ui.field);
    let release = released(&mut state);
    assert!(release.replay.is_none());
    assert_eq!(release.queued, vec![Ev::Pointer("click next")]);
}

/// A settling key that then waits for a Wayland read keeps the pointer input queued behind it, and
/// later pointer input queues too instead of overtaking it.
#[test]
fn pointer_input_keeps_its_order_when_a_settled_key_waits_for_a_read() {
    let mut ui = Ui::new();
    ui.focus("field");
    ui.clipboard = ClipText::Pending;
    let mut state = bound();
    let now = Instant::now();
    let mut pending = vec![egui::Event::Text("z".into())];
    ui.judge(&mut state, &press("v"), &mut pending, now);
    assert_eq!(
        ui.judge(&mut state, &Ev::Pointer("first"), &mut pending, now),
        EguiKey::Taken
    );
    ui.pass(&mut state, pending);
    assert!(state.awaits_text(), "the settled paste waits for the read");
    let mut pending = Vec::new();
    assert_eq!(
        ui.judge(&mut state, &Ev::Pointer("second"), &mut pending, now),
        EguiKey::Taken,
        "a later click does not overtake the queued one"
    );
    state.text_arrived(Some("WL".into()), &ui.ctx, &mut pending, now);
    ui.pass(&mut state, pending);
    let release = released(&mut state);
    assert!(release.replay.is_none());
    assert_eq!(
        release.queued,
        vec![Ev::Pointer("first"), Ev::Pointer("second")]
    );
}

/// A window that still draws is never timed out: a frame that comes late runs the edit and its
/// result decides.
#[test]
fn a_late_frame_still_decides() {
    let mut ui = Ui::new();
    ui.focus("field");
    let mut state = bound();
    let mut pending = Vec::new();
    let now = Instant::now();
    ui.judge(&mut state, &press("v"), &mut pending, now);
    state.expire(now + Duration::from_secs(5), &mut pending, || {}, false);
    assert!(state.take_release(EditKeyOwner::EguiOnly).is_none());
    assert_eq!(pending, vec![egui::Event::Paste("CLIP".into())]);
    ui.pass(&mut state, pending);
    assert_eq!(ui.field, "helloCLIP");
    assert!(released(&mut state).replay.is_none());
}

/// The frame timeout counts from when the edit went in, not from the key: text a Wayland read
/// returned late gets the full limit before a stopped window gives it up.
#[test]
fn the_frame_timeout_counts_from_the_injection() {
    let mut ui = Ui::new();
    ui.focus("field");
    ui.clipboard = ClipText::Pending;
    let mut state = bound();
    let mut pending = Vec::new();
    let now = Instant::now();
    ui.judge(&mut state, &press("v"), &mut pending, now);
    let arrived = now + Duration::from_secs(2);
    state.text_arrived(Some("WL".into()), &ui.ctx, &mut pending, arrived);
    let almost = arrived + HOST_HOLD_LIMIT - Duration::from_millis(10);
    state.expire(almost, &mut pending, || {}, true);
    assert!(
        state.take_release(EditKeyOwner::EguiOnly).is_none(),
        "the read's own wait does not count"
    );
    state.expire(arrived + HOST_HOLD_LIMIT, &mut pending, || {}, true);
    assert!(pending.is_empty(), "the edit left the input");
    assert_eq!(
        released(&mut state).replay,
        Some((press("v"), EditKey::Paste))
    );
}

/// The same text pasted over itself is a paste that ran: the key is consumed, not replayed (which
/// would paste twice through egui-winit's own Ctrl+V or type `v` for Alt+V).
#[test]
fn the_same_text_pasted_over_itself_consumes_the_key() {
    let mut ui = Ui::new();
    ui.focus("field");
    ui.select("field", 0, 5);
    ui.clipboard = ClipText::Text("hello".into());
    let mut state = bound();
    let mut pending = Vec::new();
    assert_eq!(
        ui.judge(&mut state, &press("v"), &mut pending, Instant::now()),
        EguiKey::Taken
    );
    ui.pass(&mut state, pending);
    assert_eq!(ui.field, "hello");
    assert!(released(&mut state).replay.is_none());
}

/// A password replaced by text of the same length reads the same masked: the paste still ran.
#[test]
fn a_same_length_password_paste_consumes_the_key() {
    let mut ui = Ui::new();
    ui.focus("pw");
    ui.select("pw", 0, 4);
    let mut state = bound();
    let mut pending = Vec::new();
    assert_eq!(
        ui.judge(&mut state, &press("v"), &mut pending, Instant::now()),
        EguiKey::Taken
    );
    ui.pass(&mut state, pending);
    assert_eq!(ui.secret, "CLIP");
    assert!(released(&mut state).replay.is_none());
}

/// A key the owner cannot place yet waits for the earlier input's pass and then goes back to the
/// owner unjudged, first in line. With no earlier input, or no binding, it is free at once.
#[test]
fn an_unsettled_key_goes_back_to_its_owner_after_the_earlier_input() {
    let mut ui = Ui::new();
    ui.frame(Vec::new());
    let mut state = bound();
    let now = Instant::now();
    let judge = |ui: &mut Ui, state: &mut EditKeyState<Ev>, event: &Ev, pending: &mut Vec<_>| {
        let clip = ui.clipboard.clone();
        let mut read = |_tag: u64| clip.clone();
        state.judge_unsettled(
            event,
            EditKeyOwner::Window,
            now,
            KeyCx {
                ctx: &ui.ctx,
                events: pending,
                clipboard: &mut read,
            },
        )
    };
    let mut pending = Vec::new();
    assert_eq!(
        judge(&mut ui, &mut state, &press("v"), &mut pending),
        EguiKey::Free
    );
    let at = egui::pos2(20.0, 15.0);
    let click = |pressed| egui::Event::PointerButton {
        pos: at,
        button: egui::PointerButton::Primary,
        pressed,
        modifiers: egui::Modifiers::NONE,
    };
    let mut pending = vec![egui::Event::PointerMoved(at), click(true), click(false)];
    assert_eq!(
        judge(&mut ui, &mut state, &press("q"), &mut pending),
        EguiKey::Free
    );
    assert_eq!(
        judge(&mut ui, &mut state, &press("v"), &mut pending),
        EguiKey::Taken
    );
    assert_eq!(
        judge(&mut ui, &mut state, &press("q"), &mut pending),
        EguiKey::Taken
    );
    ui.pass(&mut state, pending);
    assert_eq!(ui.field, "hello", "the owner judges the key, not the hold");
    let release = state
        .take_release(EditKeyOwner::Window)
        .expect("the key goes back");
    assert!(release.replay.is_none());
    assert_eq!(release.queued, vec![press("v"), press("q")]);
}

/// A window replaying a finished hold gives the released key once, marked with its edit, then the
/// queued events unmarked, and stops when the window closes.
#[test]
fn a_release_replays_in_order_and_stops_when_the_window_closes() {
    let release = Release {
        replay: Some((press("c"), EditKey::Copy)),
        queued: vec![Ev::Other("escape"), Ev::Other("after")],
    };
    let mut seen = Vec::new();
    let closed = replay_release(
        release,
        |closed: &bool| *closed,
        |event, judged| {
            seen.push((event.clone(), judged));
            event == Ev::Other("escape")
        },
    );
    assert_eq!(closed, Some(true));
    assert_eq!(
        seen,
        vec![
            (press("c"), Some(EditKey::Copy)),
            (Ev::Other("escape"), None)
        ]
    );
}

/// Real window events keep their place behind a held key by their winit variant: a wheel or a
/// pinch after a queued pointer move waits too, so it cannot act where the pointer was before the
/// move. Window state that no result depends on stays free.
#[test]
fn a_wheel_after_a_queued_pointer_move_waits_too() {
    use winit::event::{DeviceId, MouseScrollDelta, TouchPhase};
    let ctx = egui::Context::default();
    let mut state = EditKeyState::<WindowEvent>::default();
    let now = Instant::now();
    let owner = EditKeyOwner::EguiOnly;
    let key = WindowEvent::RedrawRequested;
    state.hold_key(&key, EditKey::Paste, Phase::Settle, now, owner, false);
    let device_id = DeviceId::dummy();
    let moved = WindowEvent::CursorMoved {
        device_id,
        position: (40.0, 40.0).into(),
    };
    let wheel = WindowEvent::MouseWheel {
        device_id,
        delta: MouseScrollDelta::PixelDelta(winit::dpi::PhysicalPosition::new(0.0, -20.0)),
        phase: TouchPhase::Moved,
    };
    let pinch = WindowEvent::PinchGesture {
        device_id,
        delta: 0.1,
        phase: TouchPhase::Moved,
    };
    let resized = WindowEvent::Resized(winit::dpi::PhysicalSize::new(800, 600));
    let mut judged = Vec::new();
    for event in [&moved, &wheel, &pinch, &resized] {
        let mut events = Vec::new();
        let mut clipboard = |_tag: u64| ClipText::Empty;
        let cx = KeyCx {
            ctx: &ctx,
            events: &mut events,
            clipboard: &mut clipboard,
        };
        judged.push(state.judge(event, owner, now, cx));
    }
    assert_eq!(
        judged,
        [
            EguiKey::Taken,
            EguiKey::Taken,
            EguiKey::Taken,
            EguiKey::Free
        ]
    );
    state.resolve_now(Outcome::NotActed);
    let release = state.take_release(owner).expect("released");
    assert_eq!(release.queued, vec![moved, wheel, pinch]);
}
