//! The hold's order, timing and result rules: pointer input behind a settling key and the frame
//! timeout.
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
