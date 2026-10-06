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
            &CallerContext::Local,
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
