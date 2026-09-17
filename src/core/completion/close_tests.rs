use super::*;

fn populated(service: &Completion) -> (u64, u64) {
    service.session(1, "codex", "parent").unwrap();
    service.session(2, "claude", "child").unwrap();
    let spawn = service.subscribe(1, 2, "claude", "spawn").unwrap();
    let tell = service.subscribe(1, 2, "claude", "tell").unwrap();
    service.observe(2, "idle", "stop", "saved result").unwrap();
    (spawn, tell)
}
fn reject_exit(service: &Completion) {
    service.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_exit BEFORE UPDATE ON completion_journal WHEN EXISTS(SELECT 1 FROM json_each(NEW.body,'$.subscriptions') WHERE json_extract(value,'$.reason')='target_exited') BEGIN SELECT RAISE(ABORT,'owned close fault'); END;").unwrap();
}
fn heal(service: &Completion) {
    service
        .inner
        .lock()
        .unwrap()
        .0
        .execute_batch("DROP TRIGGER reject_exit")
        .unwrap();
    // Advance only the owned test's scheduling state, not wall time or production backoff.
    for task in service.inner.lock().unwrap().1.pending_closes.values_mut() {
        task.retry_at = 0;
    }
}
#[test]
fn close_failure_is_durable_visible_and_retried_without_changing_acceptance() {
    let service = Completion::memory().unwrap();
    let (spawn, tell) = populated(&service);
    service
        .change(|j| {
            for e in j.events.values_mut() {
                e.phase = if e.subscription == spawn {
                    "accepted"
                } else {
                    "unknown"
                }
                .into();
            }
            Ok(())
        })
        .unwrap();
    reject_exit(&service);
    assert!(service.exited(2, "surface_closed").is_err());
    let pending = service.snapshot().unwrap();
    assert_eq!(pending.pending_closes.len(), 1);
    assert!(
        pending
            .pending_closes
            .values()
            .all(|t| t.durable && !t.error.is_empty())
    );
    assert!(!pending.subscriptions[&spawn].active && !pending.subscriptions[&tell].active);
    assert_eq!(
        pending.subscriptions[&tell].reason,
        "close_persistence_pending"
    );
    assert!(
        !service
            .observe_session(2, "idle", "stop", "late", Some("child"))
            .unwrap()
    );
    assert!(service.subscribe(1, 2, "claude", "tell").is_err());
    heal(&service);
    service.retry_pending_closes().unwrap();
    let fixed = service.snapshot().unwrap();
    assert!(fixed.pending_closes.is_empty());
    assert!(!fixed.subscriptions[&spawn].active && !fixed.subscriptions[&tell].active);
    assert_eq!(fixed.subscriptions[&tell].reason, "target_exited");
    for (id, phase) in [(spawn, "accepted"), (tell, "unknown")] {
        assert_eq!(
            fixed
                .events
                .values()
                .find(|e| e.subscription == id && e.state == "idle")
                .unwrap()
                .phase,
            phase
        );
    }
    let count = fixed.events.len();
    service.retry_pending_closes().unwrap();
    assert_eq!(service.snapshot().unwrap().events.len(), count);
}
#[test]
fn pending_close_survives_restart_and_never_closes_reused_surface_identity() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("close.db");
    let service = Completion::open(&path).unwrap();
    let (old, _) = populated(&service);
    reject_exit(&service);
    assert!(service.exited(2, "surface_closed").is_err());
    drop(service);
    let service = Completion::open(&path).unwrap();
    assert_eq!(service.snapshot().unwrap().pending_closes.len(), 1);
    service.session(2, "claude", "new-child").unwrap();
    service.session(3, "codex", "new-parent").unwrap();
    let new = service.subscribe(3, 2, "claude", "tell").unwrap();
    heal(&service);
    service.retry_pending_closes().unwrap();
    let j = service.snapshot().unwrap();
    assert!(!j.subscriptions[&old].active);
    assert!(j.subscriptions[&new].active);
    assert_eq!(j.sessions[&2].hook_session, "new-child");
    assert!(j.pending_closes.is_empty());
}
#[test]
fn absence_from_another_windows_live_set_does_not_close_its_child() {
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    let mut first = crate::core::CoreState::new(80, 24, waker.clone()).unwrap();
    let mut second = crate::core::CoreState::new(80, 24, waker).unwrap();
    let service = Completion::memory().unwrap();
    first.completion = service.clone();
    second.completion = service.clone();
    let pane = second.workspaces[0].pane_layout().all_pane_ids()[0];
    for id in [50001, 50002] {
        let tab = crate::model::Tab::new_with_surface(
            second.next_ids.next_tab(),
            "other-window".into(),
            Box::new(crate::model::TerminalSurface { id }),
        );
        second.workspaces[0]
            .pane_layout_mut()
            .find_pane_mut(pane)
            .unwrap()
            .tabs
            .push(tab);
    }
    service.session(50001, "claude", "other-parent").unwrap();
    service.session(50002, "claude", "other-child").unwrap();
    let sub = service.subscribe(50001, 50002, "claude", "tell").unwrap();
    assert!(!first.live_surface_ids().contains(&50002));
    assert!(second.live_surface_ids().contains(&50002));
    second.publish_completion_ownership().unwrap();
    first.publish_completion_ownership().unwrap();
    assert!(service.target_live(50002).unwrap());
    let live_sub = service
        .subscribe_live(50001, 50002, "claude", "tell", false)
        .unwrap();
    first.reconcile_completions();
    service.exited(1, "surface_closed").unwrap();
    first.reconcile_completions();
    let j = service.snapshot().unwrap();
    assert!(j.subscriptions[&sub].active);
    assert!(j.subscriptions[&live_sub].active);
    assert!(j.pending_closes.is_empty());
    assert_eq!(j.sessions[&50002].hook_session, "other-child");
    drop(second);
    assert!(!service.target_live(50002).unwrap());
    assert!(
        service
            .subscribe_live(50001, 50002, "claude", "tell", false)
            .is_err()
    );
}

#[test]
fn intent_store_failure_remains_explicit_until_it_can_be_persisted() {
    let service = Completion::memory().unwrap();
    let (sub, _) = populated(&service);
    service.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_intent BEFORE INSERT ON completion_closures BEGIN SELECT RAISE(ABORT,'owned intent-store fault'); END;").unwrap();
    assert!(service.exited(2, "surface_closed").is_err());
    let j = service.snapshot().unwrap();
    assert!(!j.subscriptions[&sub].active);
    assert!(
        j.pending_closes
            .values()
            .all(|task| !task.durable && task.error.contains("owned intent-store fault"))
    );
    service
        .inner
        .lock()
        .unwrap()
        .0
        .execute_batch("DROP TRIGGER reject_intent")
        .unwrap();
    for task in service.inner.lock().unwrap().1.pending_closes.values_mut() {
        task.retry_at = 0;
    }
    service.retry_pending_closes().unwrap();
    assert!(service.snapshot().unwrap().pending_closes.is_empty());
}

#[test]
fn queued_close_in_one_window_does_not_stop_another_windows_child() {
    let (mut state, mut first) = crate::state::tests::test_state();
    let waker: tasty_terminal::Waker = std::sync::Arc::new(|| {});
    let mut second = crate::core::CoreState::new(80, 24, waker).unwrap();
    let service = Completion::memory().unwrap();
    first.completion = service.clone();
    second.completion = service.clone();
    for (engine, ids) in [
        (&mut first, vec![40001, 40002]),
        (&mut second, vec![50001, 50002]),
    ] {
        let pane = engine.workspaces[0].pane_layout().all_pane_ids()[0];
        for id in ids {
            let tab = crate::model::Tab::new_with_surface(
                engine.next_ids.next_tab(),
                "owned-window".into(),
                Box::new(crate::model::TerminalSurface { id }),
            );
            engine.workspaces[0]
                .pane_layout_mut()
                .find_pane_mut(pane)
                .unwrap()
                .tabs
                .push(tab);
        }
    }
    for (surface, kind, id) in [
        (40001, "codex", "first-parent"),
        (40002, "claude", "first-child"),
        (50001, "claude", "second-parent"),
        (50002, "claude", "second-child"),
    ] {
        service.session(surface, kind, id).unwrap();
    }
    let first_sub = service.subscribe(40001, 40002, "claude", "tell").unwrap();
    let other_sub = service.subscribe(50001, 50002, "claude", "tell").unwrap();
    reject_exit(&service);
    let pane = first.workspaces[0].pane_layout().all_pane_ids()[0];
    first.workspaces[0]
        .pane_layout_mut()
        .find_pane_mut(pane)
        .unwrap()
        .tabs
        .retain(|tab| !tab.all_surface_ids().contains(&40002));
    // The normal close path has already removed topology when it invokes cleanup.
    state.cleanup_surface(&mut first, 40002, None);
    assert_eq!(service.snapshot().unwrap().pending_closes.len(), 1);
    assert!(first.find_surface_by_id(40002).is_none());
    assert!(second.find_surface_by_id(50002).is_some());
    first.reconcile_completions();
    service
        .observe(50002, "idle", "stop", "other window continues")
        .unwrap();
    let j = service.snapshot().unwrap();
    assert!(!j.subscriptions[&first_sub].active);
    assert!(j.subscriptions[&other_sub].active);
    assert_eq!(j.sessions[&50002].state, "idle");
    heal(&service);
    service.retry_pending_closes().unwrap();
    assert!(service.snapshot().unwrap().subscriptions[&other_sub].active);
}

#[test]
fn close_commit_and_queue_ack_are_one_transaction() {
    let service = Completion::memory().unwrap();
    let (sub, _) = populated(&service);
    service.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_ack BEFORE DELETE ON completion_closures BEGIN SELECT RAISE(ABORT,'owned ack fault'); END;").unwrap();
    assert!(service.exited(2, "surface_closed").is_err());
    {
        let guard = service.inner.lock().unwrap();
        let body: String = guard
            .0
            .query_row("SELECT body FROM completion_journal", [], |r| r.get(0))
            .unwrap();
        let stored: Journal = serde_json::from_str(&body).unwrap();
        assert!(stored.subscriptions[&sub].active);
        assert!(!stored.events.values().any(|e| e.state == "exited"));
    }
    service
        .inner
        .lock()
        .unwrap()
        .0
        .execute_batch("DROP TRIGGER reject_ack")
        .unwrap();
    for task in service.inner.lock().unwrap().1.pending_closes.values_mut() {
        task.retry_at = 0;
    }
    service.retry_pending_closes().unwrap();
    let j = service.snapshot().unwrap();
    assert!(!j.subscriptions[&sub].active);
    assert!(j.pending_closes.is_empty());
    assert_eq!(
        j.events
            .values()
            .filter(|e| e.subscription == sub && e.state == "exited")
            .count(),
        1
    );
}

#[test]
fn live_subscription_racing_explicit_close_never_remains_active() {
    for _ in 0..16 {
        let service = Completion::memory().unwrap();
        service.session(1, "codex", "parent").unwrap();
        service.session(2, "claude", "child").unwrap();
        let owner = std::sync::Arc::new(());
        service
            .publish_owner(&owner, [1, 2].into_iter().collect())
            .unwrap();
        let barrier = std::sync::Arc::new(std::sync::Barrier::new(3));
        let s = service.clone();
        let b = barrier.clone();
        let subscribe = std::thread::spawn(move || {
            b.wait();
            s.subscribe_live(1, 2, "claude", "spawn", false)
        });
        let s = service.clone();
        let b = barrier.clone();
        let close = std::thread::spawn(move || {
            b.wait();
            s.exited(2, "surface_closed").unwrap()
        });
        barrier.wait();
        let result = subscribe.join().unwrap();
        close.join().unwrap();
        let j = service.snapshot().unwrap();
        if let Ok(id) = result {
            assert!(!j.subscriptions[&id].active);
        }
        assert!(!service.target_live(2).unwrap());
    }
}
