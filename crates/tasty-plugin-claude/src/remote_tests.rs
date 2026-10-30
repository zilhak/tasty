//! attach 원격 자식의 client·서버 분기. 모델과 host 는 흉내 낸다.

use std::cell::RefCell;

use super::*;
use crate::codex_return::{ReturnStatus, TurnEnd};
use tasty_plugin_sdk::PluginError;

const THREAD: &str = "0193a6f2-7c1e-7b3d-9a40-1f2e3d4c5b6a";
const RECEIVER: &str = "0123456789abcdef0123456789abcdef";

type Answer = Box<dyn Fn(&str, &Value) -> Result<Value, PluginError>>;

struct Host {
    seen: RefCell<Vec<(String, Value)>>,
    answer: Answer,
}

impl HostCall for Host {
    fn call(&self, method: &str, params: Value) -> Result<Value, PluginError> {
        let out = (self.answer)(method, &params);
        self.seen.borrow_mut().push((method.to_string(), params));
        out
    }
}

impl Host {
    fn new(answer: impl Fn(&str, &Value) -> Result<Value, PluginError> + 'static) -> Self {
        Self {
            seen: RefCell::new(Vec::new()),
            answer: Box::new(answer),
        }
    }

    fn methods(&self) -> Vec<String> {
        self.seen.borrow().iter().map(|(m, _)| m.clone()).collect()
    }

    fn params_of(&self, method: &str) -> Vec<Value> {
        self.seen
            .borrow()
            .iter()
            .filter(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
            .collect()
    }
}

fn fail(code: i32, message: &str) -> Result<Value, PluginError> {
    Err(PluginError::HostCall {
        method: "m".into(),
        message: message.into(),
        code: Some(code),
    })
}

fn tr() -> Translator {
    Translator::load(
        &std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lang"),
        "en",
    )
}

fn store() -> (tempfile::TempDir, Returns) {
    let dir = tempfile::tempdir().unwrap();
    let returns = Returns::new(Some(dir.path()));
    (dir, returns)
}

fn local_binding() -> Binding {
    Binding {
        receiver_id: RECEIVER.into(),
        parent_external: None,
        attach: false,
    }
}

fn scanner() -> Arc<Mutex<ErrorScanner>> {
    Arc::new(Mutex::new(ErrorScanner::new()))
}

/// client host: workspace 2 는 mirror(anchor 12), mirror 자식 L=15 는 서버 R=99 다.
fn client_host(capable: bool, call: fn() -> Result<Value, PluginError>) -> Host {
    Host::new(move |method, params| match method {
        "attach.agent_target" => {
            if params.get("workspace") == Some(&json!(2))
                || params.get("surface_id") == Some(&json!(15))
            {
                Ok(json!({
                    "remote": true, "capable": capable, "workspace": 2,
                    "anchor_surface_id": 12,
                    "child": params.get("surface_id") == Some(&json!(15)),
                }))
            } else {
                Ok(json!({ "remote": false }))
            }
        }
        "attach.agent_call" => call(),
        "attach.agent_commit" => Ok(json!({ "committed": 1 })),
        "codex.parent" => Ok(json!({
            "receiver_id": RECEIVER,
            "terminal_parent": { "parent_external": format!("codex:{THREAD}") },
        })),
        "codex.deliver" => Ok(json!({ "event_id": "ev", "delivery_state": "captured" })),
        "terminal.children" => Ok(json!({ "children": [{ "surface_id": 15, "index": 2 }] })),
        "hook.list" => Ok(json!([])),
        _ => Ok(json!({})),
    })
}

fn spawned() -> Result<Value, PluginError> {
    Ok(json!({
        "call_id": 7,
        "result": { "child_surface_id": 99, "child_index": 0, "requested_session_options": {} },
        "children": [{ "surface_id": 15, "remote_surface_id": 99, "index": 2 }],
    }))
}

fn spawn_params(extra: Value) -> Value {
    let mut p = json!({
        "surface": 3,
        "workspace": "2",
        "prompt": "do it",
        "role": "worker",
        "caller_surface": 3,
        "_tasty_context": { "provider": "cli" },
    });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

/// 물리 부모: 서버로는 허용 인자만 가고, 알림 훅을 mirror 자식에 건 뒤 commit 한다. 이 Tasty 에 터미널을 만들지 않는다.
#[test]
fn a_mirror_spawn_runs_on_the_server_and_binds_hooks_before_commit() {
    let (_d, returns) = store();
    let host = client_host(true, spawned);
    let out = crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &spawn_params(json!({})),
        None,
        &tr(),
    )
    .unwrap();
    assert_eq!(out["child_surface_id"], json!(15));
    assert_eq!(out["child_index"], json!(2));
    assert_eq!(out["remote"], json!(true));
    assert_eq!(out["remote_child_surface_id"], json!(99));
    assert_eq!(out["parent_surface_id"], json!(3));
    let methods = host.methods();
    assert!(
        !methods
            .iter()
            .any(|m| m == "terminal.spawn" || m == "surface.send")
    );
    let call = &host.params_of("attach.agent_call")[0];
    assert_eq!(call["method"], json!("spawn"));
    assert_eq!(call["anchor_surface_id"], json!(12));
    assert_eq!(call["parent"], json!({ "surface": 3 }));
    assert_eq!(call["role"], json!("worker"));
    let sent = call["params"].as_object().unwrap();
    for forbidden in [
        "_tasty_context",
        "surface",
        "parent",
        "workspace",
        "caller_surface",
        "role",
    ] {
        assert!(
            !sent.contains_key(forbidden),
            "{forbidden} leaked: {sent:?}"
        );
    }
    assert_eq!(sent["prompt"], json!("do it"));
    assert_eq!(sent[RETURN_TURNS_KEY], json!(false));
    let last_hook = methods
        .iter()
        .rposition(|m| m == "hook.set")
        .expect("notify hooks");
    let commit = methods
        .iter()
        .position(|m| m == "attach.agent_commit")
        .expect("commit");
    assert!(last_hook < commit, "{methods:?}");
    assert!(
        host.params_of("hook.set")
            .iter()
            .all(|p| p["surface_id"] == json!(15))
    );
}

/// Codex 대화 부모: 결과 경로를 mirror 자식에 묶고 서버에 턴 결과를 요청한다. 알림 훅은 걸지 않는다.
#[test]
fn a_codex_parent_binds_its_route_on_the_mirror_child_and_asks_for_turns() {
    let (_d, returns) = store();
    let host = client_host(true, spawned);
    let mut params =
        spawn_params(json!({ "_tasty_context": { "provider": "codex", "thread_id": THREAD } }));
    params.as_object_mut().unwrap().remove("surface");
    let out =
        crate::handlers::handle_spawn(&scanner(), &returns, &host, &params, None, &tr()).unwrap();
    assert_eq!(out["parent_external"], json!(format!("codex:{THREAD}")));
    let call = &host.params_of("attach.agent_call")[0];
    assert_eq!(
        call["parent"],
        json!({ "external": format!("codex:{THREAD}") })
    );
    assert_eq!(call["params"][RETURN_TURNS_KEY], json!(true));
    assert!(host.params_of("hook.set").is_empty());
    assert_eq!(host.params_of("attach.agent_commit").len(), 1);
    // 서버에서 온 결과는 이 경로의 수신처로 기록된다.
    let r = returns
        .remote_receive(
            &host,
            &json!({
                "surface_id": 15, "remote_surface_id": 99, "key": "s:1",
                "payload": { "completion": {
                    "completion_key": "claude:abc:t1", "provider": "claude", "child_surface": 99,
                    "child_session": "sess", "status": "turn_ended", "body": "done",
                } },
            }),
        )
        .unwrap();
    assert_eq!(r, json!({ "status": "recorded" }));
    let delivered = &host.params_of("codex.deliver")[0];
    assert_eq!(delivered["receiver_id"], json!(RECEIVER));
    assert_eq!(delivered["completion_key"], json!("r.claude:abc:t1"));
    assert_eq!(delivered["child_surface"], json!(15));
    assert!(delivered.get("attach").is_none());
}

#[test]
fn an_old_server_is_refused_before_any_call() {
    let (_d, returns) = store();
    let host = client_host(false, spawned);
    let err = crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &spawn_params(json!({})),
        None,
        &tr(),
    )
    .unwrap_err();
    assert!(err.message.contains("does not support"), "{}", err.message);
    let methods = host.methods();
    assert!(
        !methods
            .iter()
            .any(|m| m.starts_with("attach.agent_call") || m == "terminal.spawn")
    );
}

/// 결과를 모르면 성공으로도 실패로도 단정하지 않고, 경로·훅·commit 을 하지 않는다.
#[test]
fn an_unknown_outcome_is_reported_as_unknown() {
    let (_d, returns) = store();
    let host = client_host(true, || {
        fail(
            remote_attach::OUTCOME_UNKNOWN_CODE,
            "call 7 outcome unknown",
        )
    });
    let err = crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &spawn_params(json!({})),
        None,
        &tr(),
    )
    .unwrap_err();
    assert!(err.message.contains("unknown"), "{}", err.message);
    assert!(err.message.contains("call 7"), "{}", err.message);
    let methods = host.methods();
    assert!(
        !methods
            .iter()
            .any(|m| m == "hook.set" || m == "attach.agent_commit")
    );
}

/// 사용자가 명시한 --profile-file 경로는 그대로 서버로 가고, 서버가 자기 파일시스템에서 확인한다.
#[test]
fn an_explicit_profile_file_is_checked_on_the_server() {
    let (_d, returns) = store();
    let host = client_host(true, spawned);
    crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &spawn_params(json!({ "profile_file": "/srv/profiles/p.json" })),
        None,
        &tr(),
    )
    .unwrap();
    assert_eq!(
        host.params_of("attach.agent_call")[0]["params"]["profile_file"],
        json!("/srv/profiles/p.json")
    );

    let server = server_host("pending");
    let err = crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &server,
        &origin_params(json!({ "profile_file": "/nonexistent/tasty-remote-profile.json" })),
        None,
        &tr(),
    )
    .unwrap_err();
    assert!(
        err.message.contains("does not exist on the server"),
        "{}",
        err.message
    );
    assert!(server.params_of("terminal.spawn").is_empty());
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("p.json");
    std::fs::write(&file, "{}").unwrap();
    crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &server,
        &origin_params(json!({ "profile_file": file.to_string_lossy() })),
        None,
        &tr(),
    )
    .unwrap();
    assert_eq!(server.params_of("terminal.spawn").len(), 1);
}

/// 경로를 묶지 못하면 commit 하지 않는다(첫 완료를 풀지 않는다). 받을 곳 없는 원격 자식은 닫는다.
#[test]
fn a_child_whose_route_cannot_be_bound_is_not_committed_and_is_closed() {
    let dir = tempfile::tempdir().unwrap();
    let blocked = dir.path().join("data");
    std::fs::write(&blocked, "not a directory").unwrap();
    let returns = Returns::new(Some(&blocked));
    let host = client_host(true, spawned);
    let mut params =
        spawn_params(json!({ "_tasty_context": { "provider": "codex", "thread_id": THREAD } }));
    params.as_object_mut().unwrap().remove("surface");
    let err = crate::handlers::handle_spawn(&scanner(), &returns, &host, &params, None, &tr())
        .unwrap_err();
    assert!(err.message.contains("15"), "{}", err.message);
    assert!(
        host.params_of("attach.agent_commit").is_empty(),
        "must not release notices"
    );
    let calls = host.params_of("attach.agent_call");
    assert_eq!(calls.len(), 2, "{calls:?}");
    assert_eq!(calls[1]["method"], json!("kill"));
    assert_eq!(calls[1]["target_surface_id"], json!(15));
}

/// mirror 가 아닌 workspace 는 기존 로컬 spawn 그대로다.
#[test]
fn a_local_workspace_keeps_the_local_spawn() {
    let (_d, returns) = store();
    let host = Host::new(|method, _| match method {
        "attach.agent_target" => Ok(json!({ "remote": false })),
        "terminal.spawn" => Ok(json!({ "child_surface_id": 50, "child_index": 0 })),
        "hook.list" => Ok(json!([])),
        _ => Ok(json!({})),
    });
    let out = crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &spawn_params(json!({ "workspace": "8" })),
        None,
        &tr(),
    )
    .unwrap();
    assert_eq!(out["child_surface_id"], json!(50));
    assert!(host.params_of("attach.agent_call").is_empty());
    assert!(host.methods().iter().any(|m| m == "terminal.spawn"));
}

/// 경로 먼저, 입력 나중. 원격 자식의 입력은 서버로 가고 이 Tasty 의 terminal.tell 은 쓰지 않는다.
#[test]
fn a_tell_to_a_mirror_child_goes_to_the_server_after_the_route() {
    let (_d, returns) = store();
    let host = client_host(true, || {
        Ok(json!({ "call_id": 8, "result": { "ok": true }, "children": [] }))
    });
    let params = json!({
        "surface": 15, "message": "next",
        "_tasty_context": { "provider": "codex", "thread_id": THREAD },
    });
    crate::handlers::handle_tell(&returns, &host, &params, &tr()).unwrap();
    let methods = host.methods();
    assert!(!methods.iter().any(|m| m == "terminal.tell"));
    let call = &host.params_of("attach.agent_call")[0];
    assert_eq!(call["method"], json!("tell"));
    assert_eq!(call["target_surface_id"], json!(15));
    assert_eq!(call["params"]["message"], json!("next"));
    assert_eq!(call["params"][RETURN_TURNS_KEY], json!(true));
    // 경로가 저장돼 있어 이후 결과가 이 대화로 간다.
    let r = returns
        .remote_receive(
            &host,
            &json!({
                "surface_id": 15, "key": "k",
                "payload": { "completion": { "completion_key": "claude:x:t2", "provider": "claude",
                    "child_surface": 99, "status": "turn_ended" } },
            }),
        )
        .unwrap();
    assert_eq!(r["status"], json!("recorded"));
}

/// kill 뒤에도 서버에 남은 결과는 기록된다. 경로는 끝난 경로로만 남는다.
#[test]
fn a_killed_mirror_child_still_records_late_results() {
    let (_d, returns) = store();
    returns.bind_new_child(15, &local_binding(), false).unwrap();
    let host = client_host(true, || {
        Ok(json!({ "call_id": 9, "result": { "killed": true }, "children": [] }))
    });
    let out = crate::handlers::handle_kill(
        &scanner(),
        &returns,
        &host,
        &json!({ "surface": 3, "child_index": 2 }),
        &tr(),
    )
    .unwrap();
    assert_eq!(out["killed"], json!(true));
    assert_eq!(out["killed_surface_id"], json!(15));
    assert!(!host.methods().iter().any(|m| m == "terminal.kill"));
    assert_eq!(
        host.params_of("attach.agent_call")[0]["method"],
        json!("kill")
    );
    let r = returns
        .remote_receive(
            &host,
            &json!({
                "surface_id": 15, "key": "k",
                "payload": { "completion": { "completion_key": "claude:x:t3", "provider": "claude",
                    "child_surface": 99, "status": "exited" } },
            }),
        )
        .unwrap();
    assert_eq!(r["status"], json!("recorded"));
}

#[test]
fn a_result_without_a_route_is_rejected_and_a_failed_record_is_retried() {
    let (_d, returns) = store();
    let host = Host::new(|method, _| match method {
        "codex.deliver" => fail(-32000, "receiver offline"),
        _ => Ok(json!({})),
    });
    let payload = json!({ "completion": { "completion_key": "claude:x:t1", "provider": "claude",
        "child_surface": 1, "status": "turn_ended" } });
    let r = returns
        .remote_receive(
            &host,
            &json!({ "surface_id": 15, "key": "k", "payload": payload }),
        )
        .unwrap();
    assert_eq!(r["status"], json!("rejected"));
    returns.bind_new_child(15, &local_binding(), false).unwrap();
    assert!(
        returns
            .remote_receive(
                &host,
                &json!({ "surface_id": 15, "key": "k", "payload": payload })
            )
            .is_err(),
        "a temporary failure must make the host retry"
    );
    let dup = Host::new(|method, _| match method {
        "codex.deliver" => {
            Ok(json!({ "event_id": "ev", "delivery_state": "sent", "duplicate": true }))
        }
        _ => Ok(json!({})),
    });
    let r = returns
        .remote_receive(
            &dup,
            &json!({ "surface_id": 15, "key": "k", "payload": payload }),
        )
        .unwrap();
    assert_eq!(r["status"], json!("duplicate"));
}

fn origin_params(extra: Value) -> Value {
    let mut p = json!({
        "parent_external": "attach:o3",
        "attach_context": "ctx",
        "workspace": "5",
        "pane": 6,
    });
    for (k, v) in extra.as_object().unwrap() {
        p[k] = v.clone();
    }
    p
}

fn server_host(notice: &'static str) -> Host {
    Host::new(move |method, params| match method {
        // surface 40 은 attach 원격 부모의 자식이고 7 은 이 서버의 일반 자식이다.
        "terminal.parent" if params["surface"] == json!(40) => {
            Ok(json!({ "parent_external": "attach:o3", "status": "active" }))
        }
        "terminal.parent" => Ok(json!({ "parent_surface_id": 5, "status": "active" })),
        "terminal.spawn" => Ok(json!({ "child_surface_id": 40, "child_index": 0, "pane_id": 6 })),
        "attach.child_notice" => Ok(json!({ "status": notice })),
        "terminal.children" => Ok(json!({ "children": [{ "surface_id": 40, "index": 0 }] })),
        "terminal.kill" => Ok(json!({ "killed_surface_id": 40, "child_index": 0 })),
        _ => Ok(json!({})),
    })
}

/// 서버: 원격 출처 부모에게는 알림 훅·Codex 부모 해석을 쓰지 않고, 턴 결과는 attach.child_notice 로 넘긴다.
/// client 가 기록을 확인할 때까지 영속 요청을 지우지 않는다.
#[test]
fn the_server_runs_the_child_here_and_returns_turns_through_the_attach_parent() {
    let (_d, returns) = store();
    let host = server_host("pending");
    let params = origin_params(json!({ "prompt": "p", "cwd": "/srv", RETURN_TURNS_KEY: true }));
    let out =
        crate::handlers::handle_spawn(&scanner(), &returns, &host, &params, None, &tr()).unwrap();
    assert_eq!(out["parent_external"], json!("attach:o3"));
    let spawn = &host.params_of("terminal.spawn")[0];
    for key in [
        "parent_external",
        "attach_context",
        "workspace",
        "pane",
        "cwd",
    ] {
        assert!(spawn.get(key).is_some(), "{key} missing in {spawn}");
    }
    let methods = host.methods();
    assert!(
        !methods
            .iter()
            .any(|m| m == "hook.set" || m == "codex.parent" || m.starts_with("attach.agent"))
    );

    returns.observe("session-start", 40, Some("sA"), None);
    returns.turn_end(TurnEnd {
        surface: 40,
        session: Some("sA"),
        status: ReturnStatus::TurnEnded,
        body: Some("done"),
        attempt_id: None,
        turn: None,
    });
    returns.pump_once(&host, 0);
    let notice = &host.params_of("attach.child_notice")[0];
    assert_eq!(notice["surface_id"], json!(40));
    let key = notice["key"].as_str().unwrap().to_string();
    assert!(key.starts_with("claude:"), "{key}");
    assert_eq!(notice["payload"]["completion"]["body"], json!("done"));
    assert!(notice["payload"]["completion"].get("receiver_id").is_none());
    assert!(host.params_of("codex.deliver").is_empty());
    assert_eq!(
        returns.pending().unwrap().len(),
        1,
        "pending keeps the record"
    );

    // 같은 키로 다시 묻고, delivered 를 받은 뒤에야 지운다.
    let done = server_host("delivered");
    returns.pump_once(&done, u64::MAX / 2);
    assert_eq!(done.params_of("attach.child_notice")[0]["key"], json!(key));
    assert!(returns.pending().unwrap().is_empty());
}

/// 물리 부모의 원격 자식에는 턴 결과 경로를 두지 않는다(상태·이벤트는 host 가 중계한다).
#[test]
fn a_physical_attach_parent_gets_no_turn_results() {
    let (_d, returns) = store();
    let host = server_host("pending");
    crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &origin_params(json!({ "prompt": "p" })),
        None,
        &tr(),
    )
    .unwrap();
    returns.observe("session-start", 40, Some("sA"), None);
    returns.turn_end(TurnEnd {
        surface: 40,
        session: Some("sA"),
        status: ReturnStatus::TurnEnded,
        body: Some("done"),
        attempt_id: None,
        turn: None,
    });
    returns.pump_once(&host, 0);
    assert!(host.params_of("attach.child_notice").is_empty());
}

#[test]
fn the_server_tells_and_kills_only_the_child_the_host_named() {
    let (_d, returns) = store();
    let host = server_host("pending");
    let err = crate::handlers::handle_tell(
        &returns,
        &host,
        &origin_params(json!({ "message": "m" })),
        &tr(),
    )
    .unwrap_err();
    assert!(err.message.contains("did not name"), "{}", err.message);
    crate::handlers::handle_tell(
        &returns,
        &host,
        &origin_params(json!({ "message": "m", "attach_target_surface": 40 })),
        &tr(),
    )
    .unwrap();
    assert_eq!(
        host.params_of("terminal.tell")[0],
        json!({ "surface": 40, "text": "m" })
    );
    assert!(host.params_of("hook.set").is_empty());

    let out = crate::handlers::handle_kill(
        &scanner(),
        &returns,
        &host,
        &origin_params(json!({ "attach_target_surface": 40 })),
        &tr(),
    )
    .unwrap();
    assert_eq!(out["killed"], json!(true));
    let kill = &host.params_of("terminal.kill")[0];
    assert_eq!(kill["parent_external"], json!("attach:o3"));
    assert_eq!(kill["child"], json!(0));
}

/// 문맥 토큰 없이 `attach:` 부모만 온 호출은 원격 출처로 다루지 않는다.
#[test]
fn a_parent_key_without_the_host_context_is_not_an_attach_origin() {
    let (_d, returns) = store();
    let host = server_host("pending");
    let mut p = origin_params(json!({ "prompt": "p" }));
    p.as_object_mut().unwrap().remove("attach_context");
    let err =
        crate::handlers::handle_spawn(&scanner(), &returns, &host, &p, None, &tr()).unwrap_err();
    assert!(
        host.params_of("terminal.spawn").is_empty(),
        "{}",
        err.message
    );
}

/// 매니페스트가 원격 실행 메서드·수신 메서드와 부모 알림 문구가 읽는 메타를 선언한다.
#[test]
fn the_manifest_declares_the_remote_attach_contract() {
    let text = std::fs::read_to_string(
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tasty-plugin.toml"),
    )
    .unwrap();
    let section = text
        .split("[contributes.remote_attach]")
        .nth(1)
        .expect("remote_attach section")
        .split("\n[")
        .next()
        .unwrap();
    assert!(
        section.contains(r#"methods = ["spawn", "tell", "kill"]"#),
        "{section}"
    );
    assert!(
        section.contains(r#"receive = "remote_receive""#),
        "{section}"
    );
    for key in [
        crate::hook::STOP_FAILURE_META_KEY,
        crate::hook::BACKGROUND_WAIT_META_KEY,
        crate::task_watch::BACKGROUND_QUIET_META_KEY,
    ] {
        assert!(section.contains(&format!("\"{key}\"")), "{key} not relayed");
    }
}

#[test]
fn the_call_number_is_read_from_the_unknown_outcome_message() {
    assert_eq!(call_id_in("call 42 outcome unknown"), Some(42));
    assert_eq!(call_id_in("remote call 7: reconnect failed"), Some(7));
    assert_eq!(call_id_in("outcome unknown"), None);
}

/// 결과불명이었던 spawn 의 자식이 늦게 생기면 그때의 부모로 경로를 묶고 recorded 로 답한다(host 가 commit).
#[test]
fn a_late_child_of_an_unknown_spawn_is_bound_to_its_waiting_parent() {
    let (_d, returns) = store();
    let host = client_host(true, || {
        fail(
            remote_attach::OUTCOME_UNKNOWN_CODE,
            "call 9101 outcome unknown",
        )
    });
    crate::handlers::handle_spawn(
        &scanner(),
        &returns,
        &host,
        &spawn_params(json!({})),
        None,
        &tr(),
    )
    .unwrap_err();
    let r = receive(
        &returns,
        &host,
        &json!({
            "kind": "late_spawn", "call_id": 9101, "result": { "child_surface_id": 99 }, "error": null,
            "children": [{ "surface_id": 15, "remote_surface_id": 99, "index": 2 }],
        }),
        &tr(),
    )
    .unwrap();
    assert_eq!(r, json!({ "status": "recorded" }));
    assert!(
        host.params_of("hook.set")
            .iter()
            .any(|p| p["surface_id"] == json!(15))
    );
    assert!(
        host.params_of("attach.agent_commit").is_empty(),
        "the host commits"
    );
}

/// 기다리는 부모가 없으면(재시작·만료) 늦은 자식을 닫고 거절한다.
#[test]
fn a_late_child_nobody_waits_for_is_closed_and_refused() {
    let (_d, returns) = store();
    let host = client_host(true, || {
        Ok(json!({ "call_id": 1, "result": { "killed": true }, "children": [] }))
    });
    let r = receive(
        &returns,
        &host,
        &json!({
            "kind": "late_spawn", "call_id": 9202, "result": null, "error": null,
            "children": [{ "surface_id": 15, "remote_surface_id": 99, "index": 2 }],
        }),
        &tr(),
    )
    .unwrap();
    assert_eq!(r["status"], json!("rejected"));
    let kill = &host.params_of("attach.agent_call")[0];
    assert_eq!(kill["method"], json!("kill"));
    assert_eq!(kill["target_surface_id"], json!(15));
    assert!(host.params_of("hook.set").is_empty());
}

/// 서버의 공개 호출(사용자 CLI 등)은 attach 원격 자식을 바꾸지 못한다. 출처 키를 꾸며 넣어도 그 부모의 자식이 아닌
/// surface 에는 닿지 않는다.
#[test]
fn server_local_public_calls_cannot_drive_attach_children() {
    let (_d, returns) = store();
    let host = server_host("pending");
    let err = refuse_public(
        &host,
        "tell",
        &json!({ "surface": 40, "message": "m" }),
        &tr(),
    )
    .unwrap_err();
    assert!(err.message.contains("40"), "{}", err.message);
    for m in ["kill", "respawn", "child_profile", "broadcast"] {
        assert!(
            refuse_public(
                &host,
                m,
                &json!({ "parent_external": "attach:o3", "child": 0 }),
                &tr()
            )
            .is_err(),
            "{m}"
        );
    }
    assert!(refuse_public(&host, "reboot", &json!({ "surface": 40 }), &tr()).is_err());
    assert!(
        refuse_public(
            &host,
            "tell",
            &json!({ "surface": 7, "message": "m" }),
            &tr()
        )
        .is_ok()
    );

    let forged = origin_params(json!({ "message": "m", "attach_target_surface": 7 }));
    let err = crate::handlers::handle_tell(&returns, &host, &forged, &tr()).unwrap_err();
    assert!(err.message.contains("did not name"), "{}", err.message);
    assert!(host.params_of("terminal.tell").is_empty());
}
