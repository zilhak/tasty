//! 대체 메모리 저장소의 쓰기 응답이 durable:false를 알리는지 확인한다(ADR-0010).

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

/// 실제 스토어(in-memory SQLite)를 넣은 `AppServices`. 시험용 fake 는 `import_regular` 를
/// 구현하지 않는다.
fn core_with(fallback: Option<tasty_memory::InitFallback>) -> crate::app::services::AppServices {
    let store = tasty_memory::MemoryStore::open_in_memory().expect("store");
    test_core_builder()
        .with_memory(std::sync::Arc::new(std::sync::Mutex::new(store)))
        .with_memory_init_fallback(fallback)
        .build()
        .expect("core")
}

fn call(core: &mut crate::app::services::AppServices, method: &str, params: Value) -> Value {
    let resp = call_raw(core, method, params);
    resp.result
        .unwrap_or_else(|| panic!("{method} 이 실패했다: {:?}", resp.error))
}

fn call_raw(
    core: &mut crate::app::services::AppServices,
    method: &str,
    mut params: Value,
) -> tasty_ipc::protocol::JsonRpcResponse {
    let (mut state, mut engine_session) = crate::state::tests::test_state();
    let mut engine = engine_session.borrow_mut();
    // surface metadata는 열린 surface에만 쓴다. 표의 surface_id 0은 fixture의 열린 surface로 바꾼다.
    if params.get("surface_id") == Some(&json!(0)) {
        let live = engine
            .workspaces()
            .into_iter()
            .flat_map(|ws| ws.all_surface_ids())
            .next()
            .expect("fixture 에 열린 surface 가 있다");
        params["surface_id"] = json!(live);
    }
    let req = tasty_ipc::protocol::JsonRpcRequest {
        response_timeout_ms: None,
        idempotency_key: None,
        session_token: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params,
    };
    super::super::handle_with_caller(core, &mut state, &mut engine, &req, &CallerContext::Local)
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

/// `memory.*` 밖에서 같은 `memory.db` 에 쓰는 메서드 — 이름공간마다 하나 이상, 응답 모양이
/// 다른 것(직렬화한 레코드 · `ok` · `deleted` · 토큰)을 고른다.
fn writes_outside_memory() -> Vec<(&'static str, Value)> {
    vec![
        (
            "surface.meta.set",
            // surface_id 0은 call_raw가 fixture의 열린 surface로 바꾼다.
            json!({ "surface_id": 0, "key": "role", "value": "x" }),
        ),
        (
            "surface.meta.unset",
            json!({ "surface_id": 0, "key": "role" }),
        ),
        (
            "approval.summary.set",
            json!({ "workspace_id": 1, "content": "s" }),
        ),
        (
            "agent.semaphore_create",
            json!({ "workspace_id": 1, "name": "s", "permits": 1 }),
        ),
        (
            "agent.semaphore_delete",
            json!({ "workspace_id": 1, "name": "s" }),
        ),
        (
            "agent.lease_acquire",
            json!({ "workspace_id": 1, "resource": "r", "holder": "h" }),
        ),
        (
            "telemetry.cap.set",
            json!({ "agent": "a", "metric": "m", "threshold": 1.0, "window": "total", "action": "notify" }),
        ),
        ("session.issue", json!({ "agent_id": "a" })),
    ]
}

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
    for (method, params) in writes_outside_memory() {
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

#[test]
fn a_write_to_a_durable_store_answers_as_before() {
    let mut core = core_with(None);
    for (method, params) in writes().into_iter().chain(writes_outside_memory()) {
        let result = call(&mut core, method, params);
        assert!(
            result.get("durable").is_none(),
            "{method}: 정상 저장소의 응답에 칸이 생겼다: {result}"
        );
    }
}

/// `func` 의 본문을 `sources` 에서 찾는다. 본문은 중괄호 균형으로 자르므로 뒤따르는 도우미 함수는
/// 들어가지 않는다. 주석·문자열 속 중괄호는 가린 사본에서 센다. 같은 이름이 둘 이상이면 어느 본문을
/// 볼지 모르므로 실패한다.
fn body_of(sources: &[&'static str], func: &str) -> Option<&'static str> {
    let mut found = sources.iter().flat_map(|src| {
        let parsed = tasty_doc_guards::match_arms::Source::new(src);
        parsed
            .fn_bodies(func)
            .into_iter()
            .map(|r| &src[r])
            .collect::<Vec<_>>()
    });
    let body = found.next()?;
    assert!(
        found.next().is_none(),
        "`{func}` 이 두 곳에 있다 — 어느 본문을 볼지 모른다"
    );
    Some(body)
}

/// 본문이 대체 저장소 표시(`written` · `mark_durability`)를 직접 부르는가.
fn marks_durability(body: &str) -> bool {
    body.contains("written(") || body.contains("mark_durability(")
}

/// 공개 핸들러 뒤의 private 도우미가 표시를 불러도 핸들러 본문으로 세지 않는다. 다음 `pub fn` 까지를
/// 본문으로 보면 파일 끝까지가 본문이 되어 도우미의 표시를 핸들러 것으로 읽는다.
#[test]
fn a_helper_after_the_handler_does_not_count_as_its_marking() {
    const FIXTURE: &str = "\
pub fn handle_put(core: &mut Core, p: &Value) -> Result<Value, String> {
    let inner = { core.store.put(p)? };
    Ok(json!({ \"ok\": inner, \"note\": \"{ not a brace }\" }))
}

fn helper(core: &mut Core, out: &mut Value) {
    mark_durability(core, out);
}
";
    let body = body_of(&[FIXTURE], "handle_put").expect("본문");
    assert!(
        body.ends_with("}") && !body.contains("fn helper"),
        "본문이 핸들러 밖까지 늘었다:\n{body}"
    );
    assert!(
        !marks_durability(body),
        "도우미의 표시를 핸들러 것으로 읽었다:\n{body}"
    );
    let helper = body_of(&[FIXTURE], "helper").expect("도우미 본문");
    assert!(marks_durability(helper), "전제: 도우미는 표시한다");
}

// 메서드 효과 표와 라우터에서 저장소 쓰기 핸들러를 찾아 durable 표시 여부를 검사한다.
// 쓰기로 분류됐어도 실제로 저장하지 않는 메서드는 이유와 함께 제외한다.
// 함수 이름이 여러 파일에 있으면 검사 대상을 확정할 수 없으므로 실패한다.
//
// 라우터 팔은 공용 match 판정기(tasty_doc_guards::match_arms)로 읽지 않고 `"<메서드>" =>` 텍스트로
// 찾는다. 표의 메서드마다 팔이 있어야 한다는 명제라 못 찾으면 실패하고, 판정기로 옮겨도 조용히
// 빠지는 팔이 줄지 않는다. 남는 사각은 주석 속 같은 텍스트를 먼저 집는 것이다. 재는 변이로
// `"memory.put" =>` 팔을 주석으로 바꾸면 handle_put이 유일한 호출자를 잃어 dead-code 컴파일
// 오류로 막혔다(2026-10-05). 이 검사가 "팔 ⊆ 표" 같은 부분집합 명제를 갖게 되거나 주석 속
// 인용 때문에 잘못 통과한 일이 생기면 판정기로 옮긴다.
#[test]
fn every_memory_write_reports_a_fallback_store_as_not_durable() {
    /// 이 저장소(`memory.db`)에 쓰는 이름공간. 여기 없는 이름공간은 다른 저장소
    /// (`state.db` · 설정 파일 · 프로세스 메모리)에 쓴다.
    const NAMESPACES: &[&str] = &[
        "memory.",
        "agent.",
        "approval.",
        "surface.meta.",
        "telemetry.",
        "session.",
    ];
    const NOT_A_STORE_WRITE: &[(&str, &str)] = &[
        (
            "memory.export",
            "표가 Mutate 로 적지만 핸들러는 저장소를 읽기만 한다(export_regular)",
        ),
        (
            "agent.task_run",
            "runner 스레드를 켜고 끌 뿐 응답 전에 저장소에 쓰지 않는다",
        ),
        (
            "agent.task_reduce",
            "저장된 결과를 모아 계산해 돌려줄 뿐 쓰지 않는다",
        ),
    ];
    let router = include_str!("../../handler.rs");
    let sources = [
        include_str!("../memory.rs"),
        include_str!("advanced.rs"),
        include_str!("bb.rs"),
        include_str!("cache.rs"),
        include_str!("goal.rs"),
        include_str!("plan.rs"),
        include_str!("secret.rs"),
        include_str!("../agent/barrier.rs"),
        include_str!("../agent/lease.rs"),
        include_str!("../agent/ratelimit.rs"),
        include_str!("../agent/semaphore.rs"),
        include_str!("../agent/task.rs"),
        include_str!("../agent/task_graph_submit.rs"),
        include_str!("../approval/read.rs"),
        include_str!("../approval/request.rs"),
        include_str!("../approval/respond.rs"),
        include_str!("../approval/summary.rs"),
        include_str!("../meta.rs"),
        include_str!("../telemetry/cap.rs"),
        include_str!("../telemetry/record.rs"),
        include_str!("../session.rs"),
    ];

    let mut checked = Vec::new();
    let mut missing = Vec::new();
    for (method, meta) in METHOD_TABLE {
        if !NAMESPACES.iter().any(|ns| method.starts_with(ns))
            || meta.effect == MethodEffect::Read
            || NOT_A_STORE_WRITE.iter().any(|(m, _)| m == method)
        {
            continue;
        }
        let arm = format!("\"{method}\" =>");
        let at = router
            .find(&arm)
            .unwrap_or_else(|| panic!("라우터에 `{method}` arm 이 없다"));
        let after = &router[at..];
        let call_at = after
            .find("::handle_")
            .unwrap_or_else(|| panic!("`{method}` arm 에서 핸들러를 못 찾았다"));
        let func: String = after[call_at + "::".len()..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
            .collect();
        let body = body_of(&sources, &func).unwrap_or_else(|| panic!("`{func}` 본문을 못 찾았다"));
        if !marks_durability(body) {
            missing.push(format!("{method} → {func}"));
        }
        checked.push(*method);
    }
    for ns in NAMESPACES {
        assert!(
            checked.iter().any(|m| m.starts_with(ns)),
            "`{ns}` 의 쓰기 계열을 하나도 못 찾았다 — 파서나 이름공간 목록이 낡았다: {checked:?}"
        );
    }
    assert!(
        checked.len() >= 50,
        "쓰기 계열을 {}개밖에 못 찾았다 — 파서가 낡았다: {checked:?}",
        checked.len()
    );
    for (exempt, _) in NOT_A_STORE_WRITE {
        assert!(
            METHOD_TABLE.iter().any(|(m, _)| m == exempt),
            "면제 목록의 `{exempt}` 이 표에 없다 — 면제가 낡았다"
        );
    }
    assert!(
        missing.is_empty(),
        "쓰기 성공을 durable 표시 없이 답하는 핸들러가 있다 — 대체 저장소에서 durable 로 보인다:\n{}",
        missing.join("\n")
    );
}

fn graph(durability: Option<&str>) -> Value {
    let mut g = json!({"contract_version": 2, "tasks": [
        {"id": "only", "command": {"kind": "custom", "ipc_method": "system.ping", "params": {}}}]});
    if let Some(d) = durability {
        g["durability"] = json!(d);
    }
    json!({"workspace_id": 1, "graph": g})
}

#[test]
fn a_fallback_store_refuses_a_graph_that_needs_a_restart_to_survive() {
    let mut core = core_with(Some(fallback()));
    for method in ["agent.task_graph_validate", "agent.task_graph_submit"] {
        let e = call_raw(&mut core, method, graph(None))
            .error
            .unwrap_or_else(|| panic!("{method} 이 비영속 저장소에서 기본 그래프를 받았다"));
        assert_eq!(e.code, -32602, "{method}");
        let data = e.data.expect("data");
        assert_eq!(data["location"], json!("/durability"), "{method}");
        assert_eq!(data["store_durable"], json!(false), "{method}");
    }
    let tasks = call(&mut core, "agent.task_list", json!({"workspace_id": 1}));
    assert_eq!(
        tasks["tasks"],
        json!([]),
        "거절한 그래프의 task 가 남았다: {tasks}"
    );

    let ok = call(
        &mut core,
        "agent.task_graph_submit",
        graph(Some("best_effort")),
    );
    assert_eq!(ok["activated"], json!(true));
    assert_eq!(ok["durability"], json!("best_effort"));
    assert_eq!(ok["durable"], json!(false));
}

#[test]
fn a_durable_store_runs_graphs_without_an_opt_in() {
    let mut core = core_with(None);
    let ok = call(&mut core, "agent.task_graph_submit", graph(None));
    assert_eq!(ok["durability"], json!("required"));
    assert!(ok.get("durable").is_none(), "{ok}");
}
