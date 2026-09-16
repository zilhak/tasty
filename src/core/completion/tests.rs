use super::*;
use serde_json::json;
fn binding(endpoint: String) -> Binding {
    Binding {
        generation: 0,
        key: format!("{endpoint}#thread"),
        surface: 1,
        hook_session: "thread".into(),
        endpoint,
        auth_env: None,
        thread_id: "thread".into(),
        session_id: "tree".into(),
        phase: "verifying".into(),
        diagnostic: String::new(),
        server: String::new(),
        cli_version: String::new(),
        codex_home: String::new(),
        expected_home: None,
        daemon_pid: None,
        endpoint_identity: String::new(),
        connection_generation: 0,
        probe_attempts: 0,
        probe_after: 0,
        history_mode: String::new(),
    }
}
fn parent(service: &Completion) {
    service.session(1, "codex", "thread").unwrap();
}
#[test]
fn release_preserves_tell_and_cancels_only_unsent_spawn() {
    let service = Completion::memory().unwrap();
    parent(&service);
    let spawn = service.subscribe(1, 2, "claude", "spawn").unwrap();
    let tell = service.subscribe(1, 2, "claude", "tell").unwrap();
    service.observe(2, "idle", "stop", "result").unwrap();
    service.release(1, 2).unwrap();
    service.observe(2, "active", "prompt-submit", "").unwrap();
    service.observe(2, "idle", "stop", "later result").unwrap();
    let j = service.snapshot().unwrap();
    assert!(!j.subscriptions[&spawn].active);
    assert!(j.subscriptions[&tell].active);
    assert_eq!(
        j.events
            .values()
            .filter(|e| e.subscription == spawn)
            .count(),
        1
    );
    assert_eq!(
        j.events
            .values()
            .find(|e| e.subscription == spawn)
            .unwrap()
            .phase,
        "cancelled"
    );
    assert_eq!(
        j.events.values().filter(|e| e.subscription == tell).count(),
        2
    );
    service.unsubscribe(tell, 1).unwrap();
    service
        .observe(2, "needs_input", "permission-request", "")
        .unwrap();
    assert_eq!(service.snapshot().unwrap().events.len(), 3);
}
#[test]
fn restart_marks_partial_send_unknown_and_requires_rebinding() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("outbox.db");
    let service = Completion::open(&path).unwrap();
    parent(&service);
    service
        .bind(binding(format!(
            "unix://{}",
            dir.path().join("missing.sock").display()
        )))
        .unwrap();
    service.subscribe(1, 2, "codex", "spawn").unwrap();
    service.observe(2, "idle", "stop", "result").unwrap();
    service
        .change(|j| {
            j.events.values_mut().next().unwrap().phase = "in_flight".into();
            Ok(())
        })
        .unwrap();
    drop(service);
    let service = Completion::open(&path).unwrap();
    let j = service.snapshot().unwrap();
    assert_eq!(j.events.values().next().unwrap().phase, "unknown");
    assert_eq!(j.bindings.values().next().unwrap().phase, "unbound");
    assert!(j.sessions.is_empty());
    assert!(service.retry(*j.events.keys().next().unwrap(), 1).is_err());
}
#[test]
fn duplicate_hook_does_not_drop_a_later_epoch() {
    let service = Completion::memory().unwrap();
    parent(&service);
    service.subscribe(1, 2, "codex", "spawn").unwrap();
    for _ in 0..2 {
        service.observe(2, "idle", "stop", "result").unwrap();
    }
    service.observe(2, "active", "prompt-submit", "").unwrap();
    service.observe(2, "idle", "stop", "new").unwrap();
    assert_eq!(service.snapshot().unwrap().events.len(), 2);
}
#[test]
fn release_does_not_claim_to_recall_accepted_or_unknown_events() {
    for phase in ["in_flight", "unknown", "accepted", "recorded"] {
        let service = Completion::memory().unwrap();
        parent(&service);
        service.subscribe(1, 2, "codex", "spawn").unwrap();
        service.observe(2, "idle", "stop", "").unwrap();
        service
            .change(|j| {
                j.events.values_mut().next().unwrap().phase = phase.into();
                Ok(())
            })
            .unwrap();
        service.release(1, 2).unwrap();
        assert_eq!(
            service
                .snapshot()
                .unwrap()
                .events
                .values()
                .next()
                .unwrap()
                .phase,
            phase
        );
    }
}
#[test]
fn claude_parent_has_no_app_server_outbox() {
    let service = Completion::memory().unwrap();
    service.session(1, "claude", "parent").unwrap();
    for kind in ["claude", "codex"] {
        service.subscribe(1, 2, kind, "spawn").unwrap();
    }
    service.observe(2, "idle", "stop", "").unwrap();
    assert!(service.snapshot().unwrap().events.is_empty());
}
#[test]
fn reconnect_remaps_by_session_and_explicit_thread_not_old_surface() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("journal.db");
    let endpoint = format!("unix://{}", dir.path().join("socket").display());
    let service = Completion::open(&path).unwrap();
    parent(&service);
    service.session(2, "claude", "child").unwrap();
    service.bind(binding(endpoint.clone())).unwrap();
    let sub = service.subscribe(1, 2, "claude", "spawn").unwrap();
    service.observe(2, "idle", "stop", "").unwrap();
    drop(service);
    let service = Completion::open(&path).unwrap();
    service.session(10, "codex", "thread").unwrap();
    service.session(20, "claude", "child").unwrap();
    let mut b = binding(endpoint);
    b.surface = 10;
    service.bind(b).unwrap();
    let j = service.snapshot().unwrap();
    assert_eq!(j.subscriptions[&sub].parent, 10);
    assert_eq!(j.subscriptions[&sub].child, 20);
    assert_eq!(j.events.values().next().unwrap().phase, "pending");
}
#[test]
fn restored_child_generation_does_not_collapse_a_new_completion() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("generation.db");
    let service = Completion::open(&path).unwrap();
    parent(&service);
    service.session(2, "claude", "child").unwrap();
    service.subscribe(1, 2, "claude", "spawn").unwrap();
    service
        .observe(2, "idle", "stop", "before restart")
        .unwrap();
    drop(service);
    let service = Completion::open(&path).unwrap();
    service.session(10, "codex", "thread").unwrap();
    service.session(20, "claude", "child").unwrap();
    service
        .observe(20, "idle", "stop", "after restart")
        .unwrap();
    let j = service.snapshot().unwrap();
    assert_eq!(j.events.len(), 2);
    let generations: std::collections::BTreeSet<_> =
        j.events.values().map(|e| e.child_generation).collect();
    assert_eq!(generations.len(), 2);
}
#[test]
fn invalid_explicit_binding_cannot_partially_register_a_session() {
    let service = Completion::memory().unwrap();
    let mut b = binding("ws://127.0.0.1:12345".into());
    b.thread_id.clear();
    assert!(service.bind_register(b, true).is_err());
    assert!(service.snapshot().unwrap().sessions.is_empty());
    service
        .bind_register(binding("ws://127.0.0.1:12345".into()), true)
        .unwrap();
    assert!(
        service.snapshot().unwrap().sessions[&1]
            .registration
            .starts_with("explicit_bind:")
    );
}
#[test]
fn exit_preserves_a_released_subscription_reason() {
    let service = Completion::memory().unwrap();
    parent(&service);
    let id = service.subscribe(1, 2, "codex", "spawn").unwrap();
    service.release(1, 2).unwrap();
    service.exited(2, "process-exit").unwrap();
    assert_eq!(
        service.snapshot().unwrap().subscriptions[&id].reason,
        "relation_released"
    );
}
#[test]
fn released_relation_cannot_resume_after_adoption_or_retry() {
    let service = Completion::memory().unwrap();
    parent(&service);
    service.session(2, "codex", "child").unwrap();
    let old = service.subscribe(1, 2, "codex", "spawn").unwrap();
    service.observe(2, "idle", "interrupt", "").unwrap();
    service
        .change(|j| {
            j.events.values_mut().next().unwrap().phase = "blocked".into();
            Ok(())
        })
        .unwrap();
    let event = *service.snapshot().unwrap().events.keys().next().unwrap();
    service.release(1, 2).unwrap();
    service.session(3, "codex", "other-parent").unwrap();
    let new = service.subscribe(3, 2, "codex", "spawn").unwrap();
    service.observe(2, "active", "prompt-submit", "").unwrap();
    service
        .observe(2, "needs_input", "permission-request", "")
        .unwrap();
    let j = service.snapshot().unwrap();
    assert_eq!(j.events[&event].phase, "cancelled");
    assert!(service.retry(event, 1).is_err());
    assert_eq!(
        j.events.values().filter(|e| e.subscription == old).count(),
        1
    );
    assert_eq!(
        j.events.values().filter(|e| e.subscription == new).count(),
        1
    );
}
#[test]
fn unknown_parent_preserves_event_without_guessing_a_channel() {
    let service = Completion::memory().unwrap();
    service.subscribe(1, 2, "claude", "tell").unwrap();
    service
        .observe(2, "needs_input", "permission-request", "")
        .unwrap();
    let j = service.snapshot().unwrap();
    assert_eq!(j.events.values().next().unwrap().phase, "unbound");
    assert!(j.bindings.is_empty());
}
#[cfg(unix)]
mod wire {
    use super::*;
    use std::os::unix::net::UnixListener;
    use std::sync::{Arc, Mutex};
    struct Server {
        endpoint: String,
        requests: Arc<Mutex<Vec<serde_json::Value>>>,
        thread: Option<std::thread::JoinHandle<()>>,
        _dir: tempfile::TempDir,
    }
    impl Server {
        fn new(drop_ack: bool, loaded: bool, connections: usize) -> Self {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("server.sock");
            let listener = UnixListener::bind(&path).unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let seen = requests.clone();
            let thread = std::thread::spawn(move || {
                let mut history = Vec::new();
                for _ in 0..connections {
                    let (stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut socket = tungstenite::accept(stream).unwrap();
                    while let Ok(message) = socket.read() {
                        let Ok(text) = message.to_text() else {
                            continue;
                        };
                        let request: serde_json::Value = serde_json::from_str(text).unwrap();
                        seen.lock().unwrap().push(request.clone());
                        let method = request["method"].as_str().unwrap();
                        if method == "initialized" {
                            continue;
                        }
                        let t = json!({"id":"thread","sessionId":"tree","historyMode":"paginated"});
                        let result = match method {
                            "initialize" => {
                                json!({"userAgent":"codex/0.154.0","codexHome":"/fixture"})
                            }
                            "server/diagnostics" => json!({"process":{"id":7}}),
                            "thread/loaded/list" => {
                                json!({"data":if loaded{vec!["system","thread"]}else{vec!["system"]}})
                            }
                            "thread/read" | "thread/resume" => json!({"thread":t}),
                            "turn/start" => {
                                assert_eq!(request["params"]["input"], json!([]));
                                assert!(request["params"].get("model").is_none());
                                assert!(request["params"].get("sandboxPolicy").is_none());
                                let mut output = request["params"]["toolOutput"].clone();
                                output["type"] = json!("functionCallOutput");
                                history.push(output);
                                if drop_ack {
                                    break;
                                }
                                json!({"turn":{"id":"turn","status":"inProgress","items":[]}})
                            }
                            "thread/items/list" => {
                                if request["params"]["cursor"].is_null() {
                                    json!({"data":[],"nextCursor":"second"})
                                } else {
                                    json!({"data":history,"nextCursor":null})
                                }
                            }
                            _ => panic!("unexpected method {method}"),
                        };
                        socket
                            .send(tungstenite::Message::Text(
                                json!({"id":request["id"],"result":result})
                                    .to_string()
                                    .into(),
                            ))
                            .unwrap();
                    }
                }
            });
            Self {
                endpoint: format!("unix://{}", path.display()),
                requests,
                thread: Some(thread),
                _dir: dir,
            }
        }
        fn finish(mut self) -> Vec<serde_json::Value> {
            self.thread.take().unwrap().join().unwrap();
            self.requests.lock().unwrap().clone()
        }
    }
    #[test]
    fn actual_websocket_transport_recovers_lost_ack_without_resending() {
        let server = Server::new(true, true, 2);
        let service = Completion::memory().unwrap();
        parent(&service);
        service.bind(binding(server.endpoint.clone())).unwrap();
        service.subscribe(1, 2, "claude", "spawn").unwrap();
        service.observe(2, "idle", "stop", "result").unwrap();
        worker::tick(&service).unwrap();
        assert_eq!(
            service
                .snapshot()
                .unwrap()
                .events
                .values()
                .next()
                .unwrap()
                .phase,
            "unknown"
        );
        worker::tick(&service).unwrap();
        assert_eq!(
            service
                .snapshot()
                .unwrap()
                .events
                .values()
                .next()
                .unwrap()
                .phase,
            "recorded"
        );
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|v| v["method"] == "turn/start")
                .count(),
            1
        );
        assert!(requests.iter().any(|v| v["params"]["cursor"] == "second"));
    }
    #[test]
    fn concurrent_sender_claims_do_not_duplicate_a_request() {
        let server = Server::new(false, true, 2);
        let service = Completion::memory().unwrap();
        parent(&service);
        service.bind(binding(server.endpoint.clone())).unwrap();
        service.subscribe(1, 2, "claude", "spawn").unwrap();
        service.observe(2, "idle", "stop", "result").unwrap();
        let second = service.clone();
        let first = std::thread::spawn(move || worker::tick(&second).unwrap());
        worker::tick(&service).unwrap();
        first.join().unwrap();
        let requests = server.finish();
        assert_eq!(
            requests
                .iter()
                .filter(|v| v["method"] == "turn/start")
                .count(),
            1
        );
    }
    #[test]
    fn unowned_disk_thread_is_not_resumed_or_sent_to() {
        let server = Server::new(false, false, 1);
        let service = Completion::memory().unwrap();
        parent(&service);
        service.bind(binding(server.endpoint.clone())).unwrap();
        service.subscribe(1, 2, "codex", "spawn").unwrap();
        service.observe(2, "idle", "stop", "").unwrap();
        worker::tick(&service).unwrap();
        assert_eq!(
            service
                .snapshot()
                .unwrap()
                .events
                .values()
                .next()
                .unwrap()
                .phase,
            "pending"
        );
        let requests = server.finish();
        assert!(
            !requests
                .iter()
                .any(|v| v["method"] == "thread/resume" || v["method"] == "turn/start")
        );
    }
}
