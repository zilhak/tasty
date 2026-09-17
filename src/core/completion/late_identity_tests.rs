use super::*;
fn restored_case(legacy: bool, delayed_child: bool, second_watch: bool, phase: &str) -> bool {
    let d = tempfile::tempdir().unwrap();
    let db = d.path().join("restore.db");
    let s = Completion::open(&db).unwrap();
    s.session(1, "codex", "p").unwrap();
    s.session(2, "claude", "c").unwrap();
    let old = s.subscribe(1, 2, "claude", "spawn").unwrap();
    let tell = s.subscribe(1, 2, "claude", "tell").unwrap();
    s.observe(2, "idle", "stop", "saved").unwrap();
    s.change(|j| {
        for e in j.events.values_mut().filter(|e| e.subscription == old) {
            e.phase = phase.into();
        }
        Ok(())
    })
    .unwrap();
    drop(s);
    if legacy {
        let c = rusqlite::Connection::open(&db).unwrap();
        let raw: String = c
            .query_row("SELECT body FROM completion_journal", [], |r| r.get(0))
            .unwrap();
        let mut v: serde_json::Value = serde_json::from_str(&raw).unwrap();
        for sub in v["subscriptions"].as_object_mut().unwrap().values_mut() {
            sub.as_object_mut().unwrap().remove("relation_generation");
        }
        c.execute("UPDATE completion_journal SET body=?1", [v.to_string()])
            .unwrap();
    }
    let s = Completion::open(&db).unwrap();
    if delayed_child {
        s.session(1, "codex", "p").unwrap();
    } else {
        s.session(2, "claude", "c").unwrap();
    }
    let new = s.subscribe(1, 2, "claude", "spawn").unwrap();
    if delayed_child {
        s.session(2, "claude", "c").unwrap();
    } else {
        s.session(1, "codex", "p").unwrap();
    }
    if second_watch {
        s.subscribe(1, 2, "claude", "spawn").unwrap();
    }
    let before = s
        .snapshot()
        .unwrap()
        .events
        .values()
        .filter(|e| e.subscription == old)
        .count();
    s.release(1, 2).unwrap();
    s.observe(2, "active", "prompt-submit", "").unwrap();
    s.observe(2, "idle", "stop", "after release").unwrap();
    let j = s.snapshot().unwrap();
    let after = j.events.values().filter(|e| e.subscription == old).count();
    let old_phase = &j
        .events
        .values()
        .find(|e| e.subscription == old)
        .unwrap()
        .phase;
    let expected = if phase == "unbound" {
        "cancelled"
    } else {
        phase
    };
    let pass = !j.subscriptions[&old].active
        && !j.subscriptions[&new].active
        && j.subscriptions[&tell].active
        && before == after
        && old_phase == expected;
    tracing::info!(
        "legacy={legacy} delayed_child={delayed_child} second_watch={second_watch} seed={phase} old_active={} new_active={} old_events={before}->{after} seed_after={old_phase} pass={pass}",
        j.subscriptions[&old].active,
        j.subscriptions[&new].active
    );
    pass
}
#[test]
fn delayed_restored_identity_then_release_without_extra_watch_closes_old_spawn() {
    let mut failures = 0;
    for legacy in [false, true] {
        for child in [false, true] {
            for phase in ["unbound", "accepted", "unknown"] {
                if !restored_case(legacy, child, false, phase) {
                    failures += 1;
                }
            }
        }
    }
    assert_eq!(
        failures, 0,
        "confirmed restored identities must suffice at release without another subscribe"
    );
}
#[test]
fn delayed_restored_identity_with_extra_watch_positive_control() {
    let mut failures = 0;
    for legacy in [false, true] {
        for child in [false, true] {
            for phase in ["unbound", "accepted", "unknown"] {
                if !restored_case(legacy, child, true, phase) {
                    failures += 1;
                }
            }
        }
    }
    assert_eq!(failures, 0);
}
#[test]
fn failed_release_keeps_map_and_subscription_then_retry_commits() {
    let s = Completion::memory().unwrap();
    s.begin_relation(1, 2).unwrap();
    s.session(2, "claude", "c").unwrap();
    let id = s.subscribe(1, 2, "claude", "spawn").unwrap();
    s.observe(2, "idle", "stop", "pending").unwrap();
    let before = serde_json::to_value(s.snapshot().unwrap()).unwrap();
    s.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_write BEFORE UPDATE ON completion_journal BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
    assert!(s.release(1, 2).is_err());
    assert!(s.begin_relation(1, 2).is_err());
    assert_eq!(serde_json::to_value(s.snapshot().unwrap()).unwrap(), before);
    assert_eq!(s.snapshot().unwrap().live_relations.len(), 1);
    s.inner
        .lock()
        .unwrap()
        .0
        .execute_batch("DROP TRIGGER reject_write")
        .unwrap();
    s.release(1, 2).unwrap();
    let j = s.snapshot().unwrap();
    assert!(!j.subscriptions[&id].active);
    assert!(j.live_relations.is_empty());
    assert!(j.events.values().all(|e| e.phase == "cancelled"));
}
#[test]
fn unregistered_release_racing_idle_stays_closed() {
    for _ in 0..32 {
        let s = Completion::memory().unwrap();
        s.session(2, "claude", "c").unwrap();
        let id = s.subscribe(1, 2, "claude", "spawn").unwrap();
        let b = std::sync::Arc::new(std::sync::Barrier::new(3));
        let a = s.clone();
        let g = b.clone();
        let t = std::thread::spawn(move || {
            g.wait();
            a.release(1, 2).unwrap();
        });
        let a = s.clone();
        let g = b.clone();
        let u = std::thread::spawn(move || {
            g.wait();
            a.observe(2, "idle", "stop", "").unwrap();
        });
        b.wait();
        t.join().unwrap();
        u.join().unwrap();
        s.session(1, "codex", "p").unwrap();
        s.observe(2, "active", "prompt-submit", "").unwrap();
        s.observe(2, "idle", "stop", "").unwrap();
        let j = s.snapshot().unwrap();
        assert!(!j.subscriptions[&id].active);
        assert!(j.events.values().all(|e| e.phase == "cancelled"));
    }
}

#[test]
fn failed_restored_release_does_not_publish_identity_reconciliation() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("atomic.db");
    let service = Completion::open(&path).unwrap();
    service.session(1, "codex", "parent").unwrap();
    service.session(2, "claude", "child").unwrap();
    let old = service.subscribe(1, 2, "claude", "spawn").unwrap();
    drop(service);
    let service = Completion::open(&path).unwrap();
    service.session(2, "claude", "child").unwrap();
    let new = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.session(1, "codex", "parent").unwrap();
    let before = service.snapshot().unwrap();
    assert_ne!(
        before.subscriptions[&old].relation_generation,
        before.subscriptions[&new].relation_generation
    );
    service.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_write BEFORE UPDATE ON completion_journal BEGIN SELECT RAISE(ABORT,'owned release fault'); END;").unwrap();
    assert!(service.release(1, 2).is_err());
    let after = service.snapshot().unwrap();
    assert_eq!(
        serde_json::to_value(&before).unwrap(),
        serde_json::to_value(&after).unwrap()
    );
    assert_eq!(
        before.live_relations[&(1, 2)].generation,
        after.live_relations[&(1, 2)].generation
    );
    service
        .inner
        .lock()
        .unwrap()
        .0
        .execute_batch("DROP TRIGGER reject_write")
        .unwrap();
    service.release(1, 2).unwrap();
    let after = service.snapshot().unwrap();
    assert!(!after.subscriptions[&old].active && !after.subscriptions[&new].active);
}
