use super::*;

#[test]
fn static_lock_parser_keeps_public_locks_and_excludes_owned_state_and_literals() {
    let source = r#"
        struct Registry { state: Mutex<State> }
        static FIRST: Mutex<()> = Mutex::new(());
        pub(super) static SECOND: RwLock<()> = RwLock::new(());
        // static COMMENT: Mutex<()> = Mutex::new(());
        const TEXT: &str = "static LITERAL: Mutex<()> = Mutex::new(());";
        static POISON: AtomicBool = AtomicBool::new(false);
    "#;
    assert_eq!(
        declared_locks(&mask_non_code(source)),
        ["FIRST".into(), "SECOND".into()].into()
    );
}

#[test]
fn a_same_count_lock_swap_reports_both_new_and_stale_members() {
    let listed: BTreeSet<(String, String)> = [
        ("a.rs".into(), "FIRST".into()),
        ("b.rs".into(), "SECOND".into()),
    ]
    .into();
    let mut actual: BTreeSet<_> = listed.clone();
    assert!(lock_drift(&actual, &listed).is_empty());
    actual.remove(&("a.rs".into(), "FIRST".into()));
    actual.insert(("c.rs".into(), "NEW".into()));
    assert_eq!(actual.len(), listed.len());
    assert_eq!(
        lock_drift(&actual, &listed),
        ["unlisted: c.rs::NEW", "stale: a.rs::FIRST"]
    );
}

#[test]
fn a_missing_lock_is_not_hidden_by_surviving_declarations() {
    let listed: BTreeSet<(String, String)> = [
        ("a.rs".into(), "FIRST".into()),
        ("b.rs".into(), "SECOND".into()),
    ]
    .into();
    let actual = [("b.rs".into(), "SECOND".into())].into();
    assert_eq!(lock_drift(&actual, &listed), ["stale: a.rs::FIRST"]);
}

#[test]
fn parsed_global_access_requires_the_lock_and_uses_whole_identifiers() {
    let source = "#[test] fn unguarded() { touch_global(); }
#[test] fn guarded() { let guard = LOCK.lock(); touch_global(); }
#[test] fn unrelated() { touch_global_suffix(); }";
    let found = test_fns(&mask_non_code(source));
    assert_eq!(found.len(), 3);
    let violations: Vec<_> = found
        .iter()
        .filter(|(_, body)| mentions(body, &["touch_global"]) && !mentions(body, &["LOCK"]))
        .map(|(name, _)| name.as_str())
        .collect();
    assert_eq!(violations, ["unguarded"]);
}
