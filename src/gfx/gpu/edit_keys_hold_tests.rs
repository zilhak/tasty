//! The hold's order, timing and result rules: pointer input behind a settling key.
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
