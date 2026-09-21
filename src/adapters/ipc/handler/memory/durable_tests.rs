//! 쓰기 응답의 `durable: false` — `memory.db` 를 못 열어 in-memory 대체로 뜬 호스트가
//! 쓰기를 정상과 똑같은 `ok` 로만 확인하던 결함의 회귀 시험(ADR-0485).

use serde_json::{Value, json};

use tasty_ipc::caller::CallerContext;
use tasty_ipc::method_meta::{METHOD_TABLE, MethodEffect};

use super::super::cli_entry_tests::test_core_builder;

fn fallback() -> tasty_memory::InitFallback {
    tasty_memory::InitFallback {
        cause: "corrupt",
        error: "memory.db corrupted: /x/memory.db".into(),
    }
}

/// 실제 스토어(in-memory SQLite)를 넣은 `Core`. 시험용 fake 는 `import_regular` 를
/// 구현하지 않는다.
fn core_with(fallback: Option<tasty_memory::InitFallback>) -> crate::core::Core {
    let store = tasty_memory::MemoryStore::open_in_memory().expect("store");
    test_core_builder()
        .with_memory(std::sync::Arc::new(std::sync::Mutex::new(store)))
        .with_memory_init_fallback(fallback)
        .build()
        .expect("core")
}

fn call(core: &mut crate::core::Core, method: &str, params: Value) -> Value {
    let (mut state, mut engine) = crate::state::tests::test_state();
    let req = tasty_ipc::protocol::JsonRpcRequest {
        response_timeout_ms: None,
        idempotency_key: None,
        session_token: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
    };
    let resp = super::super::handle_with_caller(
        core,
        &mut state,
        &mut engine,
        &req,
        &CallerContext::Local,
    );
    resp.result
        .unwrap_or_else(|| panic!("{method} 이 실패했다: {:?}", resp.error))
}

fn writes() -> Vec<(&'static str, Value)> {
    vec![
        (
            "memory.put",
            json!({ "scope": "global", "key": "k", "value": "v" }),
        ),
        ("memory.delete", json!({ "scope": "global", "key": "k" })),
        ("memory.import", json!({ "entries": [], "replace": false })),
        ("memory.gc", json!({})),
    ]
}

/// 대체 저장소에서는 쓰기가 성공해도 `durable: false` 가 붙고, `ok` 등 기존 칸은 그대로다.
/// 응답 모양이 다른 넷(`ok`+`version` · `ok` · `applied`/`skipped` · `regular`/`secret`)을 본다.
#[test]
fn a_write_to_a_fallback_store_says_it_is_not_durable() {
    let mut core = core_with(Some(fallback()));
    for (method, params) in writes() {
        let result = call(&mut core, method, params);
        assert_eq!(
            result["durable"], false,
            "{method}: 대체 저장소의 쓰기가 durable 로 보였다: {result}"
        );
    }
    let put = call(
        &mut core,
        "memory.put",
        json!({ "scope": "global", "key": "k2", "value": "v" }),
    );
    assert_eq!(put["ok"], true, "기존 칸 `ok` 가 사라졌다: {put}");
    assert!(
        put.get("version").is_some(),
        "기존 칸 `version` 이 사라졌다: {put}"
    );
}

/// 정상 저장소에서는 칸을 싣지 않는다 — 응답이 이 칸이 생기기 전과 같다.
#[test]
fn a_write_to_a_durable_store_answers_as_before() {
    let mut core = core_with(None);
    for (method, params) in writes() {
        let result = call(&mut core, method, params);
        assert!(
            result.get("durable").is_none(),
            "{method}: 정상 저장소의 응답에 칸이 생겼다: {result}"
        );
    }
}

/// `memory.*` 의 쓰기 계열 메서드는 **전부** [`super::written`] 으로 답한다.
///
/// 쓰기 계열은 메서드 표의 효과 분류(`MethodEffect::Read` 가 아닌 것)로 정한다 — 목록을
/// 여기 따로 두면 표와 갈린다. `memory.export` 하나만 뺀다: 표가 `Mutate` 로 적지만
/// 핸들러는 저장소를 읽기만 한다(`export_regular`). 이름 → 핸들러 함수는 라우터 본문의
/// arm 에서, 함수 본문은 이 디렉토리의 소스에서 읽는다.
#[test]
fn every_memory_write_reports_a_fallback_store_as_not_durable() {
    const NOT_A_STORE_WRITE: &[&str] = &["memory.export"];
    let router = include_str!("../../handler.rs");
    let sources = [
        include_str!("../memory.rs"),
        include_str!("advanced.rs"),
        include_str!("bb.rs"),
        include_str!("cache.rs"),
        include_str!("goal.rs"),
        include_str!("plan.rs"),
        include_str!("secret.rs"),
    ];
    let body_of = |func: &str| -> Option<&'static str> {
        let head = format!("\npub fn {func}(");
        sources.iter().find_map(|src| {
            let at = src.find(&head)?;
            let rest = &src[at + 1..];
            let end = rest[1..].find("\npub fn ").map_or(rest.len(), |i| i + 1);
            Some(&rest[..end])
        })
    };

    let mut checked = Vec::new();
    let mut missing = Vec::new();
    for (method, meta) in METHOD_TABLE {
        if !method.starts_with("memory.")
            || meta.effect == MethodEffect::Read
            || NOT_A_STORE_WRITE.contains(method)
        {
            continue;
        }
        let arm = format!("\"{method}\" =>");
        let at = router
            .find(&arm)
            .unwrap_or_else(|| panic!("라우터에 `{method}` arm 이 없다"));
        let after = &router[at..];
        let call_at = after
            .find("memory::handle_")
            .unwrap_or_else(|| panic!("`{method}` arm 에서 핸들러를 못 찾았다"));
        let func: String = after[call_at + "memory::".len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let body = body_of(&func).unwrap_or_else(|| panic!("`{func}` 본문을 못 찾았다"));
        if !body.contains("written(") {
            missing.push(format!("{method} → {func}"));
        }
        checked.push(*method);
    }
    assert!(
        checked.len() >= 20,
        "쓰기 계열을 {}개밖에 못 찾았다 — 파서가 낡았다: {checked:?}",
        checked.len()
    );
    for exempt in NOT_A_STORE_WRITE {
        assert!(
            METHOD_TABLE.iter().any(|(m, _)| m == exempt),
            "면제 목록의 `{exempt}` 이 표에 없다 — 면제가 낡았다"
        );
    }
    assert!(
        missing.is_empty(),
        "쓰기 성공을 `written` 없이 답하는 핸들러가 있다 — 대체 저장소에서 durable 로 보인다:\n{}",
        missing.join("\n")
    );
}
