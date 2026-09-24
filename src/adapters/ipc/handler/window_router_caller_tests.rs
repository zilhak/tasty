//! 창 라우터의 메서드별 호출자 정책을 확인한다. 일반 권한 검사만으로는 Local/Agent의
//! 사용자 상태 변경을 막지 못하므로 핸들러에서도 필요한 호출자 제한을 적용해야 한다.
//!
//! 첫 시험은 모든 route_window_handler 정의에서 메서드를 읽어 정책 목록과 비교한다.
//! 본문은 Some(match ...) 한 식이어야 하고 기본 분기는 return None이어야 한다.
//! 패턴은 일반 문자열 리터럴만 허용한다. 읽을 수 없는 분기를 버리지 않고 실패시킨다.
//! 구조 분석은 주석·리터럴을 마스킹한 match_arms로 하며 실제 이름은 원문에서 읽는다.
//! 둘째 시험은 PluginOnly 메서드를 Local/Agent로 호출해 -32016과 상태 불변을 확인한다.
//!
//! ## 한계
//!
//! Rust 문법 전체를 해석하거나 모든 우회를 검출하는 검사는 아니다.
//! - handler.rs의 같은 이름 함수만 읽는다. 다른 이름·파일의 라우터나 EntryWindow에
//!   추가한 연산은 검사하지 못한다.
//! - 등록된 분기 내부의 params 조건은 모두 실행하지 않는다. 호출 시험은 빈 params만 쓴다.
//! - source_text의 문자/라이프타임 구분은 어휘 분석의 근사다. 지원하지 않는 문법은
//!   구조를 잘못 읽을 수 있다. 이 목록이 가능한 모든 한계를 열거한 것도 아니다.

use serde_json::json;
use std::collections::BTreeSet;
use std::ops::Range;
use std::sync::Arc;

use tasty_doc_guards::match_arms::{Source, matching_close};

use crate::ipc::caller::CallerContext;
use crate::ipc::protocol::JsonRpcRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WindowCallers {
    /// plugin 만 부른다. CLI·agent 호출은 `-32016` 으로 끝나고 창을 건드리지 않는다.
    PluginOnly,
}

/// 메서드별 호출자 정책과 근거.
const WINDOW_ROUTER_CALLERS: &[(&str, WindowCallers, &str)] = &[(
    "file_picker.trigger",
    WindowCallers::PluginOnly,
    "고른 경로는 호출한 plugin 에만 push 된다 — CLI·agent 호출은 사용자 포커스만 가져간다(ADR-0031)",
)];

/// 모든 정의에서 명시된 메서드를 읽는다. match 밖의 추가 라우팅은 허용하지 않는다.
fn window_router_arms() -> BTreeSet<String> {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/adapters/ipc/handler.rs");
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("{} 을 읽지 못했다: {e}", path.display()));
    let src = Source::new(&text);
    let bodies = src.fn_bodies("route_window_handler");
    assert!(
        !bodies.is_empty(),
        "`fn route_window_handler` 를 찾지 못했다 — 라우터가 옮겨졌으면 이 시험도 옮겨라"
    );
    let mut arms = BTreeSet::new();
    let mut unreadable = Vec::new();
    for body in bodies {
        let line = src.line_of(body.start);
        let block = match router_match_block(&src, &body) {
            Ok(block) => block,
            Err(e) => {
                unreadable.push(format!("{line}행 정의: {e}"));
                continue;
            }
        };
        let parsed = match src.match_arms(block) {
            Ok(parsed) => parsed,
            Err(e) => {
                unreadable.push(format!("{line}행 정의: {e}"));
                continue;
            }
        };
        for arm in parsed {
            let pattern = src.slice(&arm.pattern);
            if pattern == "_" && arm.guard.is_none() {
                // 기본 분기에서 다른 라우터로 넘기면 정책 목록으로 확인할 수 없다.
                let body = src
                    .code_slice(&arm.body)
                    .split_whitespace()
                    .collect::<String>();
                if body != "returnNone" {
                    unreadable.push(format!(
                        "{}행: `_` 팔의 본문이 `return None` 이 아니다 — `{}`",
                        src.line_of(arm.body.start),
                        src.slice(&arm.body)
                    ));
                }
                continue;
            }
            let mut bad = Vec::new();
            for alt in src.alternatives(&arm.pattern) {
                match src.plain_string(&alt) {
                    Some(name) => {
                        arms.insert(name.to_string());
                    }
                    None => bad.push(src.slice(&alt).to_string()),
                }
            }
            if !bad.is_empty() {
                unreadable.push(format!(
                    "{}행: 팔 `{pattern}` · 따옴표 이름이 아닌 조각 {bad:?}",
                    src.line_of(arm.pattern.start)
                ));
            }
        }
    }
    assert!(
        unreadable.is_empty(),
        "창 라우터에서 이 시험이 읽지 못한 자리가 있다:\n  {}\n★ 명부를 고치지 말고 \
         라우터를 명부가 읽을 수 있는 모양으로 두어라 — 못 읽는 팔은 누가 부르는지 \
         판정할 수 없다",
        unreadable.join("\n  ")
    );
    arms
}

/// 본문이 Some(match ...) 한 식인 경우 match 범위를 반환한다.
fn router_match_block(src: &Source, body: &Range<usize>) -> Result<Range<usize>, String> {
    const HEAD: &str = "Some(match request.method.as_str()";
    let inner = src.trim(body.start + 1..body.end - 1);
    let code = src.code_slice(&inner);
    let squeezed: String = code.split_whitespace().collect();
    let expected = format!("{}{{", HEAD.split_whitespace().collect::<String>());
    if !squeezed.starts_with(&expected) {
        return Err(format!(
            "본문이 `{HEAD} {{ … }})` 로 시작하지 않는다 — `match` 앞의 분기는 팔이 아니라 \
             명부를 안 거친다"
        ));
    }
    let open = inner.start
        + code
            .find('{')
            .ok_or_else(|| "`match` 블록을 못 찾았다".to_string())?;
    let close = matching_close(&src.code, open).ok_or("`match` 블록이 닫히지 않는다")?;
    let tail: String = src.code[close + 1..inner.end].split_whitespace().collect();
    if tail != ")" {
        return Err(format!(
            "`match` 뒤에 `)` 말고 `{tail}` 가 있다 — 본문은 그 식 하나여야 한다"
        ));
    }
    Ok(open..close + 1)
}

#[test]
fn every_window_router_arm_declares_who_may_call_it() {
    let arms = window_router_arms();
    assert!(
        !arms.is_empty(),
        "창 라우터에서 팔을 하나도 못 찾았다 — 추출기가 깨졌다"
    );
    let declared: BTreeSet<String> = WINDOW_ROUTER_CALLERS
        .iter()
        .map(|(m, _, _)| (*m).to_string())
        .collect();
    let undeclared: Vec<&String> = arms.difference(&declared).collect();
    let stale: Vec<&String> = declared.difference(&arms).collect();
    assert!(
        undeclared.is_empty() && stale.is_empty(),
        "창 라우터 팔과 호출자 명부가 다르다.\n  명부에 없는 팔: {undeclared:?}\n  \
         팔이 없는 명부 항목: {stale:?}\n★ 창 라우터 팔은 사용자 상태(popup·포커스)에 닿는다. \
         새 팔이면 누가 불러도 되는지 정해 `WINDOW_ROUTER_CALLERS` 에 근거와 함께 적고, \
         CLI·agent 가 사용자 상태를 바꾸지 않게 핸들러에서 판정하라"
    );
}

fn request(method: &str) -> JsonRpcRequest {
    JsonRpcRequest {
        response_timeout_ms: None,
        idempotency_key: None,
        jsonrpc: "2.0".into(),
        id: Some(json!(1)),
        method: method.into(),
        params: json!({}),
        session_token: None,
    }
}

#[test]
fn a_plugin_only_window_arm_refuses_cli_and_agent_without_touching_the_window() {
    let agent = CallerContext::Agent {
        agent_id: "child:1".into(),
        permissions: Arc::new(Default::default()),
    };
    let mut checked = 0usize;
    for (method, policy, _) in WINDOW_ROUTER_CALLERS {
        if *policy != WindowCallers::PluginOnly {
            continue;
        }
        for caller in [CallerContext::Local, agent.clone()] {
            let (mut state, mut engine) = crate::state::tests::test_state();
            let active_before = state.active_workspace;
            let resp = super::route_window_handler(
                &mut state,
                &mut engine,
                &caller,
                &request(method),
                json!(1),
            )
            .unwrap_or_else(|| panic!("`{method}` 이 창 라우터에서 라우팅되지 않았다"));
            let err = resp
                .error
                .unwrap_or_else(|| panic!("`{method}` 을 {caller:?} 가 불렀는데 성공했다"));
            assert_eq!(err.code, -32016, "`{method}` · {caller:?}");
            assert!(
                state.take_pending_intents().is_empty(),
                "`{method}` 을 {caller:?} 가 불렀는데 창 큐에 intent 가 남았다"
            );
            assert!(
                !state.popups.has_focused(),
                "`{method}` 을 {caller:?} 가 불렀는데 포커스를 가진 popup 이 있다"
            );
            assert_eq!(
                state.active_workspace, active_before,
                "`{method}` · {caller:?}"
            );
            checked += 1;
        }
    }
    assert!(checked > 0, "PluginOnly 팔을 하나도 재지 않았다");
}
