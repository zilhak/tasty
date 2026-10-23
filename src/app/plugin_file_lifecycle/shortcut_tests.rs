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
    files.shortcuts = Some(pending.clone());
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
    files.shortcuts = Some(pending.clone());
    files.enqueue(|_| {}).unwrap();
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
    files.shortcuts = Some(pending.clone());
    files.settle_shortcuts(draft("a", "F1"), true);
    assert_eq!(*pending.borrow(), draft("a", "F3"));
    assert_eq!(files.failed_shortcuts, draft("b", "F2"));
    files.settle_shortcuts(draft("a", "F3"), false);
    files.settle_shortcuts(draft("a", "F4"), false);
    let mut expected = draft("a", "F4");
    expected.extend(draft("b", "F2"));
    assert_eq!(files.failed_shortcuts, expected);
}
