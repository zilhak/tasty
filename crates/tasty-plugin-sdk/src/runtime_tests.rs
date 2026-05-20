//! `runtime_tests` 단위 테스트.

#![cfg(test)]

use super::*;
use crate::plugin::{IpcMethodCtx, IpcMethodError, SurfaceResult};
use serde_json::json;
use std::sync::{Arc, Mutex};

struct StubPlugin {
    last_ctx: Arc<Mutex<Option<IpcMethodCtx>>>,
    behavior: Behavior,
}

#[derive(Clone)]
enum Behavior {
    Ok(Value),
    Err(IpcMethodError),
}

impl Plugin for StubPlugin {
    fn id(&self) -> &str {
        "test.plugin"
    }
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_ipc_method(&mut self, ctx: IpcMethodCtx) -> Result<Value, IpcMethodError> {
        *self.last_ctx.lock().unwrap() = Some(ctx);
        match self.behavior.clone() {
            Behavior::Ok(v) => Ok(v),
            Behavior::Err(e) => Err(e),
        }
    }
}

struct DefaultPlugin;
impl Plugin for DefaultPlugin {
    fn id(&self) -> &str {
        "test.default"
    }
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
}

/// 테스트 전용 dummy HostHandle — 실제 호출하지 않고 ctx에 끼우기만 한다.
fn dummy_host() -> HostHandle {
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).expect("bind localhost");
    let port = listener.local_addr().unwrap().port();
    let accept = std::thread::spawn(move || {
        // 호출자(stream)와 대응하는 server 측 accept — 결과는 즉시 drop.
        let _accepted = listener.accept();
    });
    let stream = TcpStream::connect(("127.0.0.1", port)).expect("connect");
    accept.join().unwrap();
    let pending: PendingCalls = Arc::new(Mutex::new(HashMap::new()));
    HostHandle::new(Arc::new(Mutex::new(stream)), pending)
}

fn invoke_params(method: &str, params: Value, caller: Option<&str>) -> Value {
    let mut obj = serde_json::Map::new();
    obj.insert("method".into(), Value::String(method.into()));
    obj.insert("params".into(), params);
    if let Some(c) = caller {
        obj.insert("caller_plugin_id".into(), Value::String(c.into()));
    }
    Value::Object(obj)
}

#[test]
fn ipc_invoke_ok_serializes_result() {
    let last = Arc::new(Mutex::new(None));
    let mut plugin = StubPlugin {
        last_ctx: last.clone(),
        behavior: Behavior::Ok(json!({"ok": true, "n": 42})),
    };
    let host = dummy_host();
    let params = invoke_params("codex.spawn", json!({"cwd": "/tmp"}), None);
    let resp = build_response(7, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
    assert_eq!(resp.id, 7);
    assert!(resp.error.is_none());
    assert_eq!(resp.result, Some(json!({"ok": true, "n": 42})));

    let ctx = last.lock().unwrap().clone().unwrap();
    assert_eq!(ctx.method, "codex.spawn");
    assert_eq!(ctx.params, json!({"cwd": "/tmp"}));
    assert_eq!(ctx.caller_plugin_id, None);
}

#[test]
fn ipc_invoke_not_found_carries_error_code() {
    let last = Arc::new(Mutex::new(None));
    let mut plugin = StubPlugin {
        last_ctx: last.clone(),
        behavior: Behavior::Err(IpcMethodError::not_found("codex.bogus")),
    };
    let host = dummy_host();
    let params = invoke_params("codex.bogus", json!({}), None);
    let resp = build_response(11, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
    assert_eq!(resp.id, 11);
    assert!(resp.result.is_none());
    assert_eq!(resp.error_code, Some(-32601));
}

#[test]
fn ipc_invoke_passes_caller_plugin_id() {
    let last = Arc::new(Mutex::new(None));
    let mut plugin = StubPlugin {
        last_ctx: last.clone(),
        behavior: Behavior::Ok(Value::Null),
    };
    let host = dummy_host();
    let params = invoke_params("codex.spawn", json!({}), Some("com.other.plugin"));
    build_response(1, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
    let ctx = last.lock().unwrap().clone().unwrap();
    assert_eq!(ctx.caller_plugin_id.as_deref(), Some("com.other.plugin"));

    let params2 = invoke_params("codex.spawn", json!({}), None);
    build_response(2, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params2, &host));
    let ctx2 = last.lock().unwrap().clone().unwrap();
    assert_eq!(ctx2.caller_plugin_id, None);
}

#[test]
fn ipc_invoke_default_impl_returns_not_implemented() {
    let mut plugin = DefaultPlugin;
    let host = dummy_host();
    let params = invoke_params("codex.spawn", json!({}), None);
    let resp = build_response(3, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
    assert_eq!(resp.error_code, Some(-32601));
    assert!(resp.error.unwrap().contains("not implemented"));
}

#[test]
fn ipc_invoke_invalid_params_returns_minus_32602() {
    let mut plugin = DefaultPlugin;
    let host = dummy_host();
    let params = json!({"params": {}});
    let resp = build_response(4, dispatch(&mut plugin, METHOD_IPC_INVOKE, &params, &host));
    assert_eq!(resp.error_code, Some(-32602));
}

#[test]
fn unknown_method_returns_minus_32601() {
    let mut plugin = DefaultPlugin;
    let host = dummy_host();
    let resp = build_response(5, dispatch(&mut plugin, "nonsense", &Value::Null, &host));
    assert_eq!(resp.error_code, Some(-32601));
}

struct OnStartRecorder {
    called: Arc<Mutex<u32>>,
}
impl Plugin for OnStartRecorder {
    fn id(&self) -> &str {
        "test.on_start"
    }
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn on_start(&mut self, _host: HostHandle, _bus: crate::bus::BusHandle) {
        *self.called.lock().unwrap() += 1;
    }
}

struct ExtensionStubPlugin {
    last_ctx: Arc<
        Mutex<
            Option<(
                tasty_plugin_protocol::ExtensionHookKind,
                tasty_plugin_protocol::ExtensionHookPhase,
                tasty_plugin_protocol::ExtensionHookMode,
                String,
                Value,
            )>,
        >,
    >,
    outcome: crate::plugin::ExtensionHookOutcome,
}
impl Plugin for ExtensionStubPlugin {
    fn id(&self) -> &str {
        "test.extension"
    }
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_extension_hook(
        &mut self,
        ctx: crate::plugin::ExtensionHookCtx,
    ) -> crate::plugin::ExtensionHookOutcome {
        *self.last_ctx.lock().unwrap() =
            Some((ctx.kind, ctx.phase, ctx.mode, ctx.target, ctx.payload));
        self.outcome.clone()
    }
}

#[test]
fn extension_invoke_hook_transform_returns_modified_payload() {
    let last = Arc::new(Mutex::new(None));
    let mut plugin = ExtensionStubPlugin {
        last_ctx: last.clone(),
        outcome: crate::plugin::ExtensionHookOutcome::transformed(json!({"x": 99})),
    };
    let host = dummy_host();
    let params = json!({
        "kind": "ipc",
        "phase": "pre",
        "mode": "transform",
        "target": "com.target/method.foo",
        "payload": {"x": 1},
    });
    let resp = build_response(
        21,
        dispatch(
            &mut plugin,
            tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
            &params,
            &host,
        ),
    );
    assert_eq!(resp.id, 21);
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    assert_eq!(result.get("modified_payload"), Some(&json!({"x": 99})));
    assert!(result.get("pass").is_none() || result.get("pass") == Some(&Value::Null));

    let (kind, phase, mode, target, payload) = last.lock().unwrap().clone().unwrap();
    assert_eq!(kind, tasty_plugin_protocol::ExtensionHookKind::Ipc);
    assert_eq!(phase, tasty_plugin_protocol::ExtensionHookPhase::Pre);
    assert_eq!(mode, tasty_plugin_protocol::ExtensionHookMode::Transform);
    assert_eq!(target, "com.target/method.foo");
    assert_eq!(payload, json!({"x": 1}));
}

#[test]
fn extension_invoke_hook_filter_block_returns_pass_false() {
    let last = Arc::new(Mutex::new(None));
    let mut plugin = ExtensionStubPlugin {
        last_ctx: last.clone(),
        outcome: crate::plugin::ExtensionHookOutcome::block(),
    };
    let host = dummy_host();
    let params = json!({
        "kind": "event",
        "phase": "pre",
        "mode": "filter",
        "target": "com.target/event.bar",
        "payload": {},
    });
    let resp = build_response(
        22,
        dispatch(
            &mut plugin,
            tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
            &params,
            &host,
        ),
    );
    let result = resp.result.expect("result present");
    assert_eq!(result.get("pass"), Some(&Value::Bool(false)));
}

#[test]
fn extension_invoke_hook_default_impl_returns_pass() {
    let mut plugin = DefaultPlugin;
    let host = dummy_host();
    let params = json!({
        "kind": "ipc",
        "phase": "post",
        "mode": "observe",
        "target": "com.target/method.foo",
        "payload": {},
    });
    let resp = build_response(
        23,
        dispatch(
            &mut plugin,
            tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
            &params,
            &host,
        ),
    );
    assert!(resp.error.is_none());
    let result = resp.result.expect("result present");
    // pass() = default Outcome → both fields skipped from serialization.
    assert!(result.as_object().unwrap().is_empty());
}

#[test]
fn extension_invoke_hook_invalid_params_returns_minus_32602() {
    let mut plugin = DefaultPlugin;
    let host = dummy_host();
    let params = json!({"kind": "ipc"}); // missing required fields
    let resp = build_response(
        24,
        dispatch(
            &mut plugin,
            tasty_plugin_protocol::METHOD_EXTENSION_INVOKE_HOOK,
            &params,
            &host,
        ),
    );
    assert_eq!(resp.error_code, Some(-32602));
}

/// worker_loop이 dispatch 전에 on_start를 정확히 1회 호출해야 한다.
/// req_rx를 닫아 worker가 즉시 종료하면 on_start만 실행되고 끝.
#[test]
fn worker_loop_invokes_on_start_once_before_dispatch() {
    let called = Arc::new(Mutex::new(0u32));
    let plugin = OnStartRecorder {
        called: called.clone(),
    };
    let host = dummy_host();
    // dummy writer: 어디로도 안 가는 TcpStream 페어
    let listener = std::net::TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    let accept = std::thread::spawn(move || {
        let _accepted = listener.accept();
    });
    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    accept.join().unwrap();
    let writer = Arc::new(Mutex::new(stream));

    let (tx, rx) = mpsc::channel::<PluginRequest>();
    drop(tx); // queue 즉시 닫기 — worker_loop은 on_start 후 iter()로 빠져나간다.
    let join = std::thread::spawn(move || {
        worker_loop(plugin, rx, writer, host);
    });
    join.join().unwrap();
    assert_eq!(*called.lock().unwrap(), 1);
}

/// popup.open / popup.event / popup.closed 라우팅과 콜백 호출 검증.
struct PopupStubPlugin {
    opened: Arc<Mutex<Vec<(String, u64, Value)>>>,
    events: Arc<Mutex<Vec<(u64, tasty_plugin_protocol::UiEvent)>>>,
    closed: Arc<Mutex<Vec<(u64, tasty_plugin_protocol::PopupCloseReason)>>>,
    next_open_tree: Option<tasty_plugin_protocol::UiNode>,
    request_close: bool,
}

impl Plugin for PopupStubPlugin {
    fn id(&self) -> &str {
        "test.popup"
    }
    fn create_surface(&mut self, _ctx: SurfaceCreateCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn handle_event(&mut self, _ctx: SurfaceEventCtx) -> SurfaceResult {
        SurfaceResult {
            tree: None,
            display_name: None,
        }
    }
    fn open_popup(
        &mut self,
        ctx: crate::plugin::PopupOpenCtx,
    ) -> tasty_plugin_protocol::PopupOpenResult {
        self.opened
            .lock()
            .unwrap()
            .push((ctx.popup_id, ctx.instance_id, ctx.context));
        tasty_plugin_protocol::PopupOpenResult {
            tree: self.next_open_tree.take(),
        }
    }
    fn handle_popup_event(
        &mut self,
        ctx: crate::plugin::PopupEventCtx,
    ) -> tasty_plugin_protocol::PopupEventResult {
        self.events
            .lock()
            .unwrap()
            .push((ctx.instance_id, ctx.event));
        tasty_plugin_protocol::PopupEventResult {
            tree: None,
            close: self.request_close,
        }
    }
    fn on_popup_closed(&mut self, ctx: crate::plugin::PopupClosedCtx) {
        self.closed
            .lock()
            .unwrap()
            .push((ctx.instance_id, ctx.reason));
    }
}

fn make_popup_plugin() -> PopupStubPlugin {
    PopupStubPlugin {
        opened: Arc::new(Mutex::new(Vec::new())),
        events: Arc::new(Mutex::new(Vec::new())),
        closed: Arc::new(Mutex::new(Vec::new())),
        next_open_tree: None,
        request_close: false,
    }
}

#[test]
fn popup_open_calls_plugin_with_ctx_and_returns_tree() {
    let mut plugin = make_popup_plugin();
    plugin.next_open_tree = Some(tasty_plugin_protocol::UiNode::Spacer { size: 4 });
    let opened = plugin.opened.clone();
    let host = dummy_host();
    let params = json!({
        "popup_id": "search",
        "instance_id": 7,
        "context": {"q": "abc"},
    });
    let resp = build_response(1, dispatch(&mut plugin, METHOD_POPUP_OPEN, &params, &host));
    assert!(resp.error.is_none(), "got error: {:?}", resp.error);
    let tree = resp.result.unwrap().get("tree").cloned().unwrap();
    assert!(!tree.is_null());

    let opened = opened.lock().unwrap();
    assert_eq!(opened.len(), 1);
    assert_eq!(opened[0].0, "search");
    assert_eq!(opened[0].1, 7);
    assert_eq!(opened[0].2["q"], "abc");
}

#[test]
fn popup_event_request_close_propagates() {
    let mut plugin = make_popup_plugin();
    plugin.request_close = true;
    let host = dummy_host();
    let params = json!({
        "instance_id": 11,
        "event": {"kind": "click", "node_id": "btn"},
    });
    let resp = build_response(2, dispatch(&mut plugin, METHOD_POPUP_EVENT, &params, &host));
    assert!(resp.error.is_none(), "got error: {:?}", resp.error);
    let r = resp.result.unwrap();
    assert_eq!(r.get("close"), Some(&Value::Bool(true)));
}

#[test]
fn popup_closed_dispatch_invokes_callback() {
    let mut plugin = make_popup_plugin();
    let closed = plugin.closed.clone();
    let host = dummy_host();
    let params = json!({"instance_id": 5, "reason": "outside_click"});
    let resp = build_response(
        3,
        dispatch(&mut plugin, METHOD_POPUP_CLOSED, &params, &host),
    );
    assert!(resp.error.is_none(), "got error: {:?}", resp.error);
    assert_eq!(resp.result, Some(Value::Null));

    let c = closed.lock().unwrap();
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].0, 5);
    assert_eq!(
        c[0].1,
        tasty_plugin_protocol::PopupCloseReason::OutsideClick
    );
}
