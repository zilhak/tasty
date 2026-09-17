use super::*;
use serde_json::json;
fn binding() -> Binding {
    serde_json::from_value(json!({"generation":0,"key":"ws://127.0.0.1:9#p","surface":1,"hook_session":"p","endpoint":"ws://127.0.0.1:9","auth_env":null,"thread_id":"p","session_id":"tree","phase":"verifying","diagnostic":"","server":"","cli_version":"","codex_home":"","expected_home":null,"daemon_pid":null,"endpoint_identity":"","connection_generation":0,"probe_attempts":0,"probe_after":0,"history_mode":""})).unwrap()
}
fn seed(s: &Completion) -> u64 {
    s.session(1, "codex", "p").unwrap();
    s.session(2, "claude", "c").unwrap();
    s.subscribe(1, 2, "claude", "spawn").unwrap()
}
fn reject(s: &Completion) {
    s.inner.lock().unwrap().0.execute_batch("CREATE TRIGGER reject_exit BEFORE UPDATE ON completion_journal WHEN EXISTS(SELECT 1 FROM json_each(NEW.body,'$.subscriptions') WHERE json_extract(value,'$.reason')='target_exited') BEGIN SELECT RAISE(ABORT,'injected'); END;").unwrap();
}
fn heal(s: &Completion) {
    let mut g = s.inner.lock().unwrap();
    g.0.execute_batch("DROP TRIGGER reject_exit").unwrap();
    for t in g.1.pending_closes.values_mut() {
        t.retry_at = 0;
    }
}
#[test]
fn recovered_new_exit_uses_late_binding_without_requeueing_accepted_results() {
    let s = Completion::memory().unwrap();
    let id = seed(&s);
    for summary in ["accepted", "unknown"] {
        s.observe(2, "active", "prompt-submit", "").unwrap();
        s.observe(2, "idle", "stop", summary).unwrap();
    }
    s.change(|j| {
        for e in j.events.values_mut() {
            e.phase = e.summary.clone();
        }
        Ok(())
    })
    .unwrap();
    reject(&s);
    assert!(s.exited(2, "surface_closed").is_err());
    s.bind(binding()).unwrap();
    heal(&s);
    s.retry_pending_closes().unwrap();
    let j = s.snapshot().unwrap();
    assert!(j.pending_closes.is_empty());
    let exits: Vec<_> = j
        .events
        .values()
        .filter(|e| e.subscription == id && e.state == "exited")
        .collect();
    assert_eq!(exits.len(), 1);
    assert_eq!(exits[0].phase, "pending");
    assert_eq!(exits[0].binding.as_deref(), Some("ws://127.0.0.1:9#p"));
    for event in j.events.values().filter(|e| e.state == "idle") {
        assert_eq!(event.phase, event.summary);
    }
    s.retry_pending_closes().unwrap();
    assert_eq!(s.snapshot().unwrap().events.len(), j.events.len());
}
