use super::*;

#[test]
fn error_lease_binds_first_execution_and_rejects_replacement_and_release() {
    let service = Completion::memory().unwrap();
    service.session(1, "codex", "parent").unwrap();
    service.subscribe(1, 2, "claude", "spawn").unwrap();
    let old = service.watch_error(1, 2).unwrap();
    service.session(2, "claude", "first").unwrap();
    assert!(service.observe_error(old, 1, 2, "current error").unwrap());
    service.release(1, 2).unwrap();
    let before = service.snapshot().unwrap().events.len();
    assert!(!service.observe_error(old, 1, 2, "after release").unwrap());
    service.session(2, "claude", "replacement").unwrap();
    service.subscribe(1, 2, "claude", "tell").unwrap();
    assert!(!service.observe_error(old, 1, 2, "late old error").unwrap());
    assert_eq!(service.snapshot().unwrap().events.len(), before);
    let new = service.watch_error(1, 2).unwrap();
    assert!(service.observe_error(new, 1, 2, "new error").unwrap());
    assert_eq!(service.snapshot().unwrap().events.len(), before + 1);
}

#[test]
fn planned_same_id_resume_expires_old_error_lease_without_session_end() {
    let service = Completion::memory().unwrap();
    service.session(1, "codex", "parent").unwrap();
    service.session(2, "claude", "child").unwrap();
    service.subscribe(1, 2, "claude", "spawn").unwrap();
    let old = service.watch_error(1, 2).unwrap();
    service
        .subscribe_wait(1, 2, "claude", "tell", true)
        .unwrap();
    let new = service.watch_error(1, 2).unwrap();
    service.session(2, "claude", "child").unwrap();
    assert!(!service.observe_error(old, 1, 2, "old execution").unwrap());
    assert!(
        service
            .observe_error(new, 1, 2, "resumed execution")
            .unwrap()
    );
}

#[test]
fn overlapping_error_watches_notify_once_and_release_keeps_tell() {
    let service = Completion::memory().unwrap();
    service.session(1, "claude", "parent").unwrap();
    service.session(2, "claude", "child").unwrap();
    service.subscribe(1, 2, "claude", "spawn").unwrap();
    let spawn = service.watch_error(1, 2).unwrap();
    service.subscribe(1, 2, "claude", "tell").unwrap();
    let tell = service.watch_error(1, 2).unwrap();
    assert!(service.observe_error(spawn, 1, 2, "first").unwrap());
    assert!(!service.observe_error(tell, 1, 2, "duplicate").unwrap());
    service.session(3, "claude", "other-parent").unwrap();
    service.subscribe(3, 2, "claude", "tell").unwrap();
    let other = service.watch_error(3, 2).unwrap();
    assert!(service.observe_error(other, 3, 2, "other parent").unwrap());
    service.release(1, 2).unwrap();
    service.observe(2, "active", "prompt-submit", "").unwrap();
    assert!(!service.observe_error(spawn, 1, 2, "released").unwrap());
    assert!(service.observe_error(tell, 1, 2, "next").unwrap());
}
