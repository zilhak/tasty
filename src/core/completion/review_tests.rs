use super::*;
#[test]
fn reused_addresses_release_must_not_cancel_persisted_old_relation() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("j.db");
    let s = Completion::open(&path).unwrap();
    s.session(1, "codex", "old-parent").unwrap();
    s.session(2, "claude", "old-child").unwrap();
    let old = s.subscribe(1, 2, "claude", "spawn").unwrap();
    s.observe(2, "idle", "stop", "old result").unwrap();
    drop(s);
    let s = Completion::open(&path).unwrap();
    s.session(1, "codex", "new-parent").unwrap();
    s.session(2, "claude", "new-child").unwrap();
    let new = s.subscribe(1, 2, "claude", "spawn").unwrap();
    s.release(1, 2).unwrap();
    let j = s.snapshot().unwrap();
    tracing::info!(
        "old_active={} old_reason={} old_event={} new_active={}",
        j.subscriptions[&old].active,
        j.subscriptions[&old].reason,
        j.events.values().next().unwrap().phase,
        j.subscriptions[&new].active
    );
    assert!(
        j.subscriptions[&old].active,
        "new relation release cancelled old persisted logical relation"
    );
    assert!(!j.subscriptions[&new].active);
}
#[test]
fn same_session_resume_must_allow_waiting_tell_to_observe() {
    let s = Completion::memory().unwrap();
    s.session(1, "codex", "p").unwrap();
    s.session(2, "claude", "c").unwrap();
    let id = s.subscribe_wait(1, 2, "claude", "tell", true).unwrap();
    s.session(2, "claude", "c").unwrap();
    s.observe(2, "idle", "stop", "resumed result").unwrap();
    let j = s.snapshot().unwrap();
    tracing::info!(
        "await_session={} events={}",
        j.subscriptions[&id].await_session,
        j.events.len()
    );
    assert_eq!(
        j.events.len(),
        1,
        "same-id SessionStart leaves child-profile tell waiting forever"
    );
}
#[test]
fn unrelated_parent_cannot_unsubscribe_old_persisted_tell() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("j.db");
    let s = Completion::open(&path).unwrap();
    s.session(1, "codex", "old-parent").unwrap();
    s.session(2, "claude", "old-child").unwrap();
    let id = s.subscribe(1, 2, "claude", "tell").unwrap();
    drop(s);
    let s = Completion::open(&path).unwrap();
    s.session(1, "codex", "new-parent").unwrap();
    assert!(
        s.unsubscribe(id, 1).is_err(),
        "surface-only ownership accepted a different logical parent"
    );
}
#[test]
fn stale_error_callback_must_not_mutate_replacement_execution() {
    let s = Completion::memory().unwrap();
    s.session(1, "codex", "p").unwrap();
    s.session(2, "claude", "old").unwrap();
    s.subscribe(1, 2, "claude", "spawn").unwrap();
    // A claude.notify-error command already fired for old execution; no session is in its args.
    s.release(1, 2).unwrap();
    s.end_execution(2, "respawn").unwrap();
    s.session(2, "claude", "new").unwrap();
    let new = s.subscribe(1, 2, "claude", "spawn").unwrap();
    let before = s.snapshot().unwrap().events.len();
    s.observe_session(
        2,
        "stalled",
        "claude-error-stalled",
        "old queued callback",
        None,
    )
    .unwrap();
    let j = s.snapshot().unwrap();
    tracing::info!(
        "new_subscription={} resulting_state={} events_before={} after={}",
        new,
        j.sessions[&2].state,
        before,
        j.events.len()
    );
    assert_eq!(
        j.events.len(),
        before,
        "old callback was attributed to replacement child generation"
    );
}
#[test]
fn failed_sql_commit_does_not_publish_memory_transition() {
    let d = tempfile::tempdir().unwrap();
    let path = d.path().join("j.db");
    let s = Completion::open(&path).unwrap();
    s.session(1, "codex", "p").unwrap();
    s.subscribe(1, 2, "claude", "spawn").unwrap();
    s.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_write BEFORE UPDATE ON completion_journal BEGIN SELECT RAISE(ABORT, 'injected disk error'); END;").unwrap();
    assert!(s.observe(2, "idle", "stop", "result").is_err());
    assert!(s.snapshot().unwrap().events.is_empty());
    s.inner
        .lock()
        .unwrap()
        .0
        .execute_batch("DROP TRIGGER reject_write")
        .unwrap();
    s.observe(2, "idle", "stop", "result").unwrap();
    assert_eq!(s.snapshot().unwrap().events.len(), 1);
    drop(s);
    assert_eq!(
        Completion::open(&path)
            .unwrap()
            .snapshot()
            .unwrap()
            .events
            .len(),
        1
    );
}
#[test]
fn same_session_resume_after_session_end_restores_waiting_tell() {
    let s = Completion::memory().unwrap();
    s.session(1, "codex", "p").unwrap();
    s.session(2, "claude", "c").unwrap();
    let id = s.subscribe_wait(1, 2, "claude", "tell", true).unwrap();
    s.end_execution(2, "session-end").unwrap();
    s.session(2, "claude", "c").unwrap();
    s.observe(2, "idle", "stop", "new").unwrap();
    let j = s.snapshot().unwrap();
    assert!(!j.subscriptions[&id].await_session);
    assert_eq!(j.events.len(), 1);
}
#[test]
fn crash_writer_helper() {
    let Ok(path) = std::env::var("AUDIT_CRASH_DB") else {
        return;
    };
    let s = Completion::open(std::path::Path::new(&path)).unwrap();
    s.session(1, "codex", "p").unwrap();
    s.session(2, "claude", "c").unwrap();
    s.subscribe(1, 2, "claude", "spawn").unwrap();
    s.observe(2, "idle", "stop", "persisted").unwrap();
    s.change(|j| {
        j.events.values_mut().next().unwrap().phase = "in_flight".into();
        Ok(())
    })
    .unwrap();
    std::process::exit(73);
}
#[test]
fn process_death_after_claim_reopens_unknown_with_same_delivery_id() {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("crash.db");
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "core::completion::review_tests::crash_writer_helper",
            "--nocapture",
        ])
        .env("AUDIT_CRASH_DB", &db)
        .status()
        .unwrap();
    assert_eq!(status.code(), Some(73));
    let c = rusqlite::Connection::open(&db).unwrap();
    let body: String = c
        .query_row("SELECT body FROM completion_journal WHERE id=1", [], |r| {
            r.get(0)
        })
        .unwrap();
    let before: Journal = serde_json::from_str(&body).unwrap();
    assert_eq!(before.events.values().next().unwrap().phase, "in_flight");
    drop(c);
    let s = Completion::open(&db).unwrap();
    let after = s.snapshot().unwrap();
    let a = after.events.values().next().unwrap();
    assert_eq!(a.phase, "unknown");
    assert_eq!(
        a.delivery_id,
        before.events.values().next().unwrap().delivery_id
    );
    assert!(after.sessions.is_empty());
    assert!(s.retry(a.id, 1).is_err());
}
