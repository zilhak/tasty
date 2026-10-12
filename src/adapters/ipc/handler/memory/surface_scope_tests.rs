//! surface scope에 새 항목을 만드는 쓰기는 열린 surface에만 닿는다.
//! 핸들러를 라우터 없이 직접 불러 핸들러의 확인을 본다.

use serde_json::{Value, json};
use tasty_ipc::caller::CallerContext;
use tasty_ipc::protocol::{JsonRpcRequest, JsonRpcResponse};
use tasty_memory::Scope;

use crate::app::services::AppServices;

struct Fixture {
    core: AppServices,
    state: crate::state::RequestContext,
    session: crate::runtime::engine_session::EngineSession,
}

impl Fixture {
    fn new() -> Self {
        let store = tasty_memory::MemoryStore::open_in_memory().expect("store");
        let core = super::super::cli_entry_tests::test_core_builder()
            .with_memory(std::sync::Arc::new(std::sync::Mutex::new(store)))
            .build()
            .expect("core");
        let (state, session) = crate::state::tests::test_state();
        Self {
            core,
            state,
            session,
        }
    }

    /// fixture의 열린 surface 하나와 어느 engine에도 없는 surface ID.
    fn open_and_gone(&mut self) -> (u32, u32) {
        let engine = self.session.borrow_mut();
        let live: Vec<u32> = engine
            .workspaces()
            .into_iter()
            .flat_map(|ws| ws.all_surface_ids())
            .collect();
        let open = *live.first().expect("fixture 에 열린 surface 가 있다");
        (open, live.iter().max().copied().unwrap_or(open) + 1)
    }

    fn call(&mut self, method: &str, params: Value) -> JsonRpcResponse {
        let mut engine = self.session.borrow_mut();
        let req = JsonRpcRequest {
            caller_agent_id: None,
            response_timeout_ms: None,
            idempotency_key: None,
            session_token: None,
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        };
        super::super::handle_with_caller(
            &mut self.core,
            &mut self.state,
            &mut engine,
            &req,
            &CallerContext::local(),
        )
    }
}

#[test]
fn a_put_to_a_closed_surface_scope_is_refused_and_stores_nothing() {
    let mut fx = Fixture::new();
    let (open, gone) = fx.open_and_gone();
    for method in ["memory.put", "memory.secret.put"] {
        let refused = fx.call(
            method,
            json!({ "scope": format!("surface:{gone}"), "key": "k", "value": "v" }),
        );
        let error = refused
            .error
            .unwrap_or_else(|| panic!("{method}: 없는 surface scope 쓰기가 성공했다"));
        assert_eq!(error.code, -32602, "{method}: {}", error.message);
        assert_eq!(
            error.message,
            format!("Surface {gone} not found"),
            "{method}"
        );
    }
    let regular = fx.core.with_memory(|m| m.get(&Scope::Surface(gone), "k"));
    assert!(
        matches!(regular, Ok(None)),
        "거절한 쓰기가 남았다: {regular:?}"
    );
    let secret = fx.core.with_memory(|m| m.scopes());
    assert!(
        secret
            .as_ref()
            .is_ok_and(|s| !s.contains(&format!("surface:{gone}"))),
        "거절한 쓰기의 scope 가 남았다: {secret:?}"
    );

    for method in ["memory.put", "memory.secret.put"] {
        let written = fx.call(
            method,
            json!({ "scope": format!("surface:{open}"), "key": "k", "value": "v" }),
        );
        assert!(
            written.error.is_none(),
            "{method}: 열린 surface scope 쓰기: {:?}",
            written.error
        );
    }
}

/// 닫힌 surface에 남은 옛 항목은 읽고 지울 수 있어야 정리할 수 있다.
#[test]
fn an_entry_left_on_a_closed_surface_scope_can_still_be_read_and_deleted() {
    let mut fx = Fixture::new();
    let (_, gone) = fx.open_and_gone();
    fx.core
        .with_memory(|m| {
            m.put(
                tasty_memory::HOST_OWNER,
                &Scope::Surface(gone),
                "old",
                &tasty_memory::MemoryValue::Text("x".into()),
                &tasty_memory::PutOpts::default(),
            )
        })
        .expect("옛 항목 준비");
    let scope = format!("surface:{gone}");
    let listed = fx.call("memory.list", json!({ "scope": scope }));
    assert_eq!(
        listed.result.as_ref().map(|r| r["count"].clone()),
        Some(json!(1)),
        "옛 항목을 읽지 못했다: {listed:?}"
    );
    let deleted = fx.call("memory.delete", json!({ "scope": scope, "key": "old" }));
    assert!(
        deleted.error.is_none(),
        "옛 항목을 지우지 못했다: {deleted:?}"
    );
    let left = fx.core.with_memory(|m| m.get(&Scope::Surface(gone), "old"));
    assert!(matches!(left, Ok(None)), "지운 항목이 남았다: {left:?}");
}

fn import_entry(scope: String, key: &str) -> Value {
    json!({ "scope": scope, "key": key, "kind": "text", "value": "v" })
}

fn stored(fx: &Fixture, sid: u32, key: &str) -> bool {
    !matches!(
        fx.core.with_memory(|m| m.get(&Scope::Surface(sid), key)),
        Ok(None)
    )
}

/// import도 put과 같은 열린 surface 조건을 따르며, 거절하면 앞 엔트리도 반영하지 않는다.
#[test]
fn an_import_into_a_closed_surface_scope_is_refused_and_stores_nothing() {
    let mut fx = Fixture::new();
    let (open, gone) = fx.open_and_gone();
    let refused = fx.call(
        "memory.import",
        json!({ "entries": [
            import_entry(format!("surface:{open}"), "a"),
            import_entry(format!("surface:{gone}"), "k"),
        ]}),
    );
    let error = refused
        .error
        .expect("닫힌 surface scope import 가 성공했다");
    assert_eq!(error.code, -32602, "{}", error.message);
    assert_eq!(error.message, format!("Surface {gone} not found"));
    assert!(!stored(&fx, gone, "k"), "거절한 엔트리가 남았다");
    assert!(!stored(&fx, open, "a"), "앞 엔트리가 부분 반영됐다");
    let scopes = fx.core.with_memory(|m| m.scopes());
    assert!(
        scopes
            .as_ref()
            .is_ok_and(|s| !s.contains(&format!("surface:{gone}"))),
        "거절한 import 의 scope 가 남았다: {scopes:?}"
    );

    let written = fx.call(
        "memory.import",
        json!({ "entries": [import_entry(format!("surface:{open}"), "a")] }),
    );
    assert!(
        written.error.is_none(),
        "열린 surface scope import: {:?}",
        written.error
    );
    assert!(stored(&fx, open, "a"), "열린 surface scope import 가 없다");
}

/// headless PTY ID 공간의 surface scope도 put과 같이 거절하고 앞 엔트리를 반영하지 않는다.
#[test]
fn an_import_into_the_pty_id_space_is_refused_and_stores_nothing() {
    let mut fx = Fixture::new();
    let (open, _) = fx.open_and_gone();
    let pty = crate::runtime::terminal_store::PTY_ID_BASE;
    let refused = fx.call(
        "memory.import",
        json!({ "entries": [
            import_entry(format!("surface:{open}"), "a"),
            import_entry(format!("surface:{pty}"), "k"),
        ]}),
    );
    let error = refused.error.expect("PTY ID 공간 scope import 가 성공했다");
    assert_eq!(error.code, -32602, "{}", error.message);
    assert!(
        error.message.contains("headless PTY id space"),
        "{}",
        error.message
    );
    assert!(!stored(&fx, pty, "k"), "거절한 엔트리가 남았다");
    assert!(!stored(&fx, open, "a"), "앞 엔트리가 부분 반영됐다");
}

/// 지정한 surface 하나만 가진 engine. 다른 engine과 surface ID가 겹치지 않게 ID를 받는다.
fn engine_with_surface(surface: u32) -> crate::runtime::engine_session::EngineSession {
    use tasty_core::DomainEvent as E;
    crate::state::tests::test_state_from_model(crate::state::tests::test_model(vec![
        E::CategoryCreated {
            id: 0,
            name: "normal".into(),
            index: 0,
        },
        E::WorkspaceCreated {
            id: surface,
            name: "Other".into(),
            category: 0,
            index: 0,
            pane: surface,
        },
        E::TabCreated {
            id: surface,
            pane: surface,
            index: 0,
            name: "Terminal".into(),
            surface: tasty_core::SurfaceSpec {
                id: surface,
                kind: "terminal".into(),
                data: None,
            },
        },
    ]))
    .1
}

/// import는 포커스 engine으로 라우팅되므로 다른 창(engine)의 열린 surface는 라우터가 붙인
/// 모든 engine의 열린 surface로 판정한다. 붙이지 않으면 전달받은 engine만 보고 거절한다.
#[test]
fn an_import_into_an_open_surface_of_another_engine_succeeds_with_the_routers_surfaces() {
    let mut fx = Fixture::new();
    let (open, elsewhere) = fx.open_and_gone();
    let mut other = engine_with_surface(elsewhere);
    let params = json!({ "entries": [
        import_entry(format!("surface:{open}"), "a"),
        import_entry(format!("surface:{elsewhere}"), "b"),
    ]});

    let alone = fx.call("memory.import", params.clone());
    assert_eq!(
        alone.error.as_ref().map(|e| e.message.clone()),
        Some(format!("Surface {elsewhere} not found")),
        "전달받은 engine만 보는 경로: {alone:?}"
    );

    let req = JsonRpcRequest {
        caller_agent_id: None,
        response_timeout_ms: None,
        idempotency_key: None,
        session_token: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: "memory.import".into(),
        params,
    };
    let caller = CallerContext::local();
    let written = {
        let mut engine = fx.session.borrow_mut();
        let open_surfaces = {
            let other = other.borrow_mut();
            crate::core::request_target::open_surfaces_of([engine.as_ref(), other.as_ref()])
        };
        let checked =
            super::super::check_request(&mut fx.core, &mut fx.state, &mut engine, &req, &caller)
                .expect("admitted");
        let routed = checked.with_open_surfaces(open_surfaces);
        super::super::handle_checked_request(&mut fx.core, &mut fx.state, &mut engine, &routed)
            .wait(&fx.core)
    };
    assert!(
        written.error.is_none(),
        "다른 engine 의 열린 surface 를 거절했다: {:?}",
        written.error
    );
    assert!(stored(&fx, open, "a"));
    assert!(stored(&fx, elsewhere, "b"));
}
