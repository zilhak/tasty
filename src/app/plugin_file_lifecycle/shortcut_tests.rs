use super::*;
use crate::app::plugin_glue::shortcut::Draft;
use crate::plugin::registry_state::ShortcutOverride;
fn draft(command: &str, key: &str) -> Draft {
    Draft::from([(
        ("p".into(), command.into()),
        Some(ShortcutOverride::Key {
            value: vec![key.into()],
        }),
    )])
}
#[test]
fn pending_shortcuts_merge_latest_values_and_independent_commands() {
    let mut files = PluginFiles::default();
    let pending = std::rc::Rc::new(std::cell::RefCell::new(draft("a", "F1")));
    files.mark_shortcut_slot(
        pending.clone(),
        files
            .budget
            .reserve(crate::app::plugin_glue::shortcut::draft_bytes(
                &pending.borrow(),
            ))
            .unwrap(),
    );
    files.coalesce_shortcuts(draft("b", "F2")).unwrap();
    files.coalesce_shortcuts(draft("a", "F3")).unwrap();
    let mut expected = draft("a", "F3");
    expected.extend(draft("b", "F2"));
    assert_eq!(*pending.borrow(), expected);
}
#[test]
fn a_lifecycle_barrier_seals_the_pending_snapshot_slot() {
    let mut files = PluginFiles::default();
    let pending = std::rc::Rc::new(std::cell::RefCell::new(draft("a", "F1")));
    files.mark_shortcut_slot(
        pending.clone(),
        files
            .budget
            .reserve(crate::app::plugin_glue::shortcut::draft_bytes(
                &pending.borrow(),
            ))
            .unwrap(),
    );
    files.enqueue_sized(0, |_| {}).unwrap();
    assert!(files.shortcuts.is_none());
    assert_eq!(
        files.coalesce_shortcuts(draft("a", "F2")).unwrap_err(),
        draft("a", "F2")
    );
    assert_eq!(*pending.borrow(), draft("a", "F1"));
}
#[test]
fn older_success_preserves_newer_pending_draft_and_failures_keep_latest_dirty_values() {
    let mut files = PluginFiles::default();
    files.settle_shortcuts(draft("a", "F1"), false);
    files.settle_shortcuts(draft("b", "F2"), false);
    let pending = std::rc::Rc::new(std::cell::RefCell::new(draft("a", "F3")));
    files.mark_shortcut_slot(
        pending.clone(),
        files
            .budget
            .reserve(crate::app::plugin_glue::shortcut::draft_bytes(
                &pending.borrow(),
            ))
            .unwrap(),
    );
    files.settle_shortcuts(draft("a", "F1"), true);
    assert_eq!(*pending.borrow(), draft("a", "F3"));
    assert_eq!(files.failed_shortcuts, draft("b", "F2"));
    files.settle_shortcuts(draft("a", "F3"), false);
    files.settle_shortcuts(draft("a", "F4"), false);
    let mut expected = draft("a", "F4");
    expected.extend(draft("b", "F2"));
    assert_eq!(files.failed_shortcuts, expected);
}

#[test]
fn coalescing_refusal_keeps_the_admitted_draft_and_its_weight() {
    let mut files = PluginFiles::default();
    let (pending, held) = files.reserve_shortcuts(draft("a", "F1")).unwrap();
    let before = held.bytes();
    files.mark_shortcut_slot(pending.clone(), held.clone());
    let enormous = draft(
        "b",
        &"x".repeat(tasty_utils::batch_writer::Limits::default().queue_bytes),
    );
    assert!(files.coalesce_shortcuts(enormous).is_err());
    assert_eq!(*pending.borrow(), draft("a", "F1"));
    assert_eq!(held.bytes(), before);
    assert_eq!(files.budget.bytes(), before);
}
#[test]
fn failed_draft_stays_charged_and_only_its_successful_keys_are_released() {
    let mut files = PluginFiles::default();
    files.settle_shortcuts(draft("a", "F1"), false);
    files.settle_shortcuts(draft("b", "F2"), false);
    assert!(files.budget.bytes() > 0);
    files.settle_shortcuts(draft("a", "F1"), true);
    assert_eq!(files.failed_shortcuts, draft("b", "F2"));
    assert_eq!(
        files.budget.bytes(),
        crate::app::plugin_glue::shortcut::draft_bytes(&files.failed_shortcuts)
    );
    files.settle_shortcuts(draft("b", "F2"), true);
    assert_eq!(files.budget.bytes(), 0);
}

#[test]
fn byte_refused_shutdown_retry_keeps_the_already_accepted_failed_snapshot() {
    let mut files = PluginFiles::default();
    let original = draft(
        "a",
        &"x".repeat(tasty_utils::batch_writer::Limits::default().queue_bytes),
    );
    files.settle_shortcuts(original.clone(), false);
    let before = files.budget.bytes();
    assert!(
        files
            .reserve_shortcuts(files.failed_shortcuts.clone())
            .is_err()
    );
    assert_eq!(files.failed_shortcuts, original);
    assert_eq!(files.budget.bytes(), before);
}

#[test]
fn an_older_success_does_not_release_a_newer_failed_value_or_its_reservation() {
    let mut files = PluginFiles::default();
    files.settle_shortcuts(draft("a", "F2"), false);
    let retained = files.budget.bytes();
    files.settle_shortcuts(draft("a", "F1"), true);
    assert_eq!(files.failed_shortcuts, draft("a", "F2"));
    assert_eq!(files.budget.bytes(), retained);
}
