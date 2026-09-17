use super::*;

#[test]
fn release_before_parent_session_registration_must_close_spawn() {
    let service = Completion::memory().unwrap();
    service.session(2, "claude", "child").unwrap();
    let sub = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.release(1, 2).unwrap();
    service.session(1, "codex", "parent-later").unwrap();
    service.observe(2, "idle", "stop", "after release").unwrap();
    let journal = service.snapshot().unwrap();
    assert!(!journal.subscriptions[&sub].active);
    assert_eq!(journal.subscriptions[&sub].reason, "relation_released");
    assert!(journal.events.is_empty());
}

#[test]
fn unregistered_release_cancels_pending_spawn_but_preserves_tell_and_new_adoption() {
    let service = Completion::memory().unwrap();
    service.begin_relation(1, 2).unwrap();
    service.session(2, "claude", "child").unwrap();
    let spawn = service.subscribe(1, 2, "claude", "spawn").unwrap();
    let tell = service.subscribe(1, 2, "claude", "tell").unwrap();
    service
        .observe(2, "idle", "stop", "before release")
        .unwrap();
    service.release(1, 2).unwrap();
    service.session(1, "codex", "parent-later").unwrap();
    service.begin_relation(1, 2).unwrap();
    let adopted = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.observe(2, "active", "prompt-submit", "").unwrap();
    service.observe(2, "idle", "stop", "new relation").unwrap();
    let j = service.snapshot().unwrap();
    assert!(!j.subscriptions[&spawn].active);
    assert!(j.subscriptions[&tell].active);
    assert_ne!(
        j.subscriptions[&spawn].relation_generation,
        j.subscriptions[&adopted].relation_generation
    );
    let old: Vec<_> = j
        .events
        .values()
        .filter(|e| e.subscription == spawn)
        .collect();
    assert_eq!(old.len(), 1);
    assert_eq!(old[0].phase, "cancelled");
    assert!(j.events.values().any(|e| e.subscription == adopted));
}

#[test]
fn new_unregistered_relation_cannot_cancel_a_restored_logical_subscription() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let service = Completion::open(&path).unwrap();
    service.session(1, "codex", "old-parent").unwrap();
    service.session(2, "claude", "old-child").unwrap();
    let old = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.observe(2, "idle", "stop", "old result").unwrap();
    drop(service);
    let service = Completion::open(&path).unwrap();
    assert!(service.snapshot().unwrap().live_relations.is_empty());
    service.begin_relation(1, 2).unwrap();
    service.session(2, "claude", "new-child").unwrap();
    let new = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.release(1, 2).unwrap();
    service.session(1, "codex", "new-parent").unwrap();
    service.observe(2, "idle", "stop", "released work").unwrap();
    let j = service.snapshot().unwrap();
    assert!(j.subscriptions[&old].active);
    assert!(!j.subscriptions[&new].active);
    assert_eq!(j.events.len(), 1);
    assert_ne!(j.events.values().next().unwrap().phase, "cancelled");
}

#[test]
fn restored_pair_new_watches_release_all_spawn_but_preserve_tell_and_acceptance() {
    for legacy in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("restored.db");
        let service = Completion::open(&path).unwrap();
        service.session(1, "codex", "parent").unwrap();
        service.session(2, "claude", "child").unwrap();
        let mut ids = Vec::new();
        for _ in 0..3 {
            ids.push(service.subscribe(1, 2, "claude", "spawn").unwrap());
        }
        let tell = service.subscribe(1, 2, "claude", "tell").unwrap();
        service.observe(2, "idle", "stop", "saved").unwrap();
        service
            .change(|j| {
                for event in j.events.values_mut() {
                    if event.subscription == ids[1] {
                        event.phase = "accepted".into();
                    }
                    if event.subscription == ids[2] {
                        event.phase = "unknown".into();
                    }
                }
                Ok(())
            })
            .unwrap();
        drop(service);
        if legacy {
            let db = rusqlite::Connection::open(&path).unwrap();
            let body: String = db
                .query_row("SELECT body FROM completion_journal", [], |r| r.get(0))
                .unwrap();
            let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
            for sub in value["subscriptions"].as_object_mut().unwrap().values_mut() {
                sub.as_object_mut().unwrap().remove("relation_generation");
            }
            db.execute("UPDATE completion_journal SET body=?", [value.to_string()])
                .unwrap();
        }
        let service = Completion::open(&path).unwrap();
        service.session(1, "codex", "parent").unwrap();
        service.session(2, "claude", "child").unwrap();
        for _ in 0..2 {
            ids.push(service.subscribe(1, 2, "claude", "spawn").unwrap());
        }
        let before = service.snapshot().unwrap().events.len();
        service.release(1, 2).unwrap();
        service.observe(2, "active", "prompt-submit", "").unwrap();
        service.observe(2, "idle", "stop", "later").unwrap();
        let j = service.snapshot().unwrap();
        assert!(ids.iter().all(|id| !j.subscriptions[id].active));
        assert!(j.subscriptions[&tell].active);
        assert_eq!(j.events.len(), before + 1);
        for (id, phase) in ids.iter().zip(["cancelled", "accepted", "unknown"]) {
            assert_eq!(
                j.events
                    .values()
                    .find(|e| e.subscription == *id)
                    .unwrap()
                    .phase,
                phase
            );
        }
    }
}

#[test]
fn explicit_new_relation_never_imports_even_matching_restored_sessions() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fresh.db");
    let service = Completion::open(&path).unwrap();
    service.session(1, "codex", "parent").unwrap();
    service.session(2, "claude", "child").unwrap();
    let old = service.subscribe(1, 2, "claude", "spawn").unwrap();
    drop(service);
    let service = Completion::open(&path).unwrap();
    service.session(1, "codex", "parent").unwrap();
    service.session(2, "claude", "child").unwrap();
    service.begin_relation(1, 2).unwrap();
    let new = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.release(1, 2).unwrap();
    let j = service.snapshot().unwrap();
    assert!(j.subscriptions[&old].active);
    assert!(!j.subscriptions[&new].active);
}
