//! IPC 라우터는 **스캔이 볼 수 있는 이름**으로 갈린다.
//!
//! 이 레포의 여러 가드가 라우터를 텍스트로 읽어 메서드 이름을 뽑는다 — 헤드리스 커버리지,
//! † 게이트 대조, 라우터 표 정합이 다 그렇다. 그 판정들은 이름이 **문자열 리터럴**이라는
//! 전제 위에 있다. 이름이 매크로에서 나오거나 상수와 맞대지면 그 갈래는 어느 목록에도 안
//! 들어오고, 답하지도 사유가 적혀 있지도 않은 메서드가 **조용히** 생긴다.
//!
//! # 이 부류는 수를 세는 검사로 못 잡는다 (실측)
//!
//! 가드들은 저마다 "몇 개나 뽑혔나" 의 하한을 갖고 있고, 그 하한이 이 사각을 본다고
//! 적혀 있었다. 2026-09-05 에 변이로 쟀고 거짓이다:
//!
//! - 리터럴 하나를 매크로 뒤로 숨기면 항목이 **하나** 줄 뿐이라 하한에 안 걸린다.
//! - 매크로가 만든 이름으로 갈래를 **더하면** 항목 수가 아예 안 변한다.
//!
//! 뒤쪽이 핵심이다. 하한은 줄어드는 방향만 볼 수 있어서 "안 보이는 이름이 느는 것" 은
//! **원리적으로** 못 본다 — 하한을 실측값까지 조여도 못 잡는다.
//!
//! # 무엇을 재는가
//!
//! 라우터가 `request.method` 로 갈래를 칠 때 맞대는 값이 문자열 리터럴인가. 판정 자리는
//! 셋이다 — `== <값>` · `.starts_with(<값>)` · `match ….as_str()` 의 팔. 값을 위임 함수
//! 인자로 **넘기기만** 하는 자리는 여기서 이름을 가르지 않으므로 대상이 아니다.
//!
//! # 사거리
//!
//! **`request.method` 로 갈래를 치는 라우터만 본다.** 메서드 이름을 지역 변수(`method:
//! &str`)로 받아 가르는 자리(`handler/ime.rs` 같은 위임 라우터)는 이 표현식이 없어 대상이
//! 아니다 — 빠진 것이 아니라 범위 밖이고, 그 자리를 재려면 "무엇이 메서드 이름인가" 를
//! 함수 경계 너머로 판정할 수단이 먼저 있어야 한다. 그리고 명부는 **두 방향으로** 못박는다:
//! 명부에 있는 파일은 리터럴만 쓰고, 명부 밖의 파일은 판정 자리를 갖지 않는다. 뒤쪽이
//! 없으면 새 라우터가 명부에 안 들어온 채 아무도 안 보는 자리가 된다.

use std::collections::BTreeSet;

use super::{METHOD_EXPR, fn_body, opaque_method_sites, repo_root, rust_sources};

/// `request.method` 로 갈래를 치는 라우터 전부. 2026-09-05 실측.
const ROUTERS: &[&str] = &[
    "src/adapters/ipc/handler.rs",
    "src/app/dispatch/list_global.rs",
    "src/app/ipc/app_methods.rs",
    "src/app/ipc/debug_methods.rs",
    "src/app/ipc/window_required.rs",
    "src/boot/headless_dispatch.rs",
];

/// 이 규칙을 **재는** 쪽. 가드는 판정 자리의 모양을 합성 입력으로 담으므로 라우터가
/// 아니면서 같은 형태를 갖는다 — 명부 대조에서 뺀다.
const GUARD_DIR: &str = "src/source_guards/";

fn has_decision_site(src: &str) -> bool {
    let mut at = 0usize;
    while let Some(i) = src[at..].find(METHOD_EXPR) {
        at += i + METHOD_EXPR.len();
        let rest = src[at..].trim_start();
        if rest.starts_with("==") || rest.starts_with(".starts_with(") {
            return true;
        }
        if let Some(r) = rest.strip_prefix(".as_str()")
            && r.trim_start().starts_with('{')
        {
            return true;
        }
    }
    false
}

#[test]
fn every_router_decides_by_a_name_the_scan_can_see() {
    for rel in ROUTERS {
        let path = repo_root().join(rel);
        let src = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
            .replace("\r\n", "\n");
        assert!(
            has_decision_site(&src),
            "{rel} 에 `{METHOD_EXPR}` 로 갈래를 치는 자리가 하나도 없다. 라우터가 아니게 \
             됐으면 명부에서 빼고, 이름을 읽는 표현식이 바뀌었으면 `METHOD_EXPR` 을 \
             고쳐라 — 안 고치면 이 검사는 아무 자리도 안 보면서 초록이다"
        );
        let opaque = opaque_method_sites(&src);
        assert!(
            opaque.is_empty(),
            "{rel} 이 **문자열 리터럴이 아닌 값**으로 메서드 이름을 가른다. 라우터를 \
             텍스트로 읽는 가드들이 그 이름을 못 보고, 답하지도 사유가 적혀 있지도 않은 \
             메서드가 조용히 생긴다. 리터럴로 적어라: {opaque:?}"
        );
    }
}

#[test]
fn no_router_escapes_the_roster() {
    let listed: BTreeSet<&str> = ROUTERS.iter().copied().collect();
    let mut found: Vec<String> = Vec::new();
    for (path, src) in rust_sources() {
        let rel = path.to_string_lossy().replace('\\', "/");
        if rel.starts_with(GUARD_DIR) {
            continue;
        }
        if has_decision_site(&src) && !listed.contains(rel.as_str()) {
            found.push(rel);
        }
    }
    assert!(
        found.is_empty(),
        "`{METHOD_EXPR}` 로 갈래를 치는데 명부에 없는 파일이 있다. 새 라우터는 아무도 \
         안 보는 자리가 된다 — `ROUTERS` 에 넣어라: {found:?}"
    );
}

/// 매크로가 만든 이름을 문다 — 실측으로 뚫렸던 두 형태 그대로.
#[test]
fn a_name_a_macro_makes_is_caught() {
    let hidden = "\
fn pump_ipc(app: &mut App) {
    if cmd.request.method == hidden_name!() { go(); }
}
";
    let body = fn_body(hidden, "fn pump_ipc").unwrap();
    // 이 본문에는 `"ns.method"` 꼴 리터럴이 하나도 없다 — 리터럴만 걷는 스캔에는 이
    // 갈래가 통째로 안 보인다는 것이 전제다.
    assert!(
        !body.contains('"'),
        "고정 입력에 리터럴이 들어갔다 — 전제가 깨졌다"
    );
    assert!(
        !opaque_method_sites(&body).is_empty(),
        "매크로가 만든 이름을 안 봤다"
    );

    let arm = "\
fn pump_ipc(app: &mut App) {
    let r = match cmd.request.method.as_str() {
        \"ns.one\" => a(),
        HIDDEN_NAME => b(),
        other => c(other),
    };
}
";
    let body = fn_body(arm, "fn pump_ipc").unwrap();
    let sites = opaque_method_sites(&body);
    assert_eq!(
        sites.len(),
        1,
        "상수 팔 하나만 걸려야 한다(리터럴 팔과 전부받기 바인딩은 정상이다): {sites:?}"
    );
}

/// 값을 **넘기기만** 하는 자리는 판정 자리가 아니다 — 거짓 양성을 막는 대조군.
#[test]
fn passing_the_name_along_is_not_a_decision() {
    let src = "\
fn pump_ipc(app: &mut App) {
    delegate(&cmd.request.method, &cmd.request.params, id);
    let s = cmd.request.method.as_str();
}
";
    let body = fn_body(src, "fn pump_ipc").unwrap();
    assert!(
        opaque_method_sites(&body).is_empty(),
        "넘기는 자리를 판정으로 셌다"
    );
}

/// 팔의 패턴이 **괄호를 가질 때도** 잡히는가 — 실측으로 뚫렸던 모양 그대로.
///
/// 매크로 호출 패턴은 `mac!()` 처럼 괄호를 담는다. 팔의 끝을 닫는 괄호로도 인정하면
/// 패턴의 시작 자리가 그 괄호 **뒤로** 밀려 패턴이 빈 문자열이 되고, 빈 패턴은 건너뛰어
/// 진다. 게다가 앞 팔이 블록이고 쉼표가 없으며 그 사이에 `#[cfg(...)]` 이 끼는 것이
/// 실제 dispatch 의 흔한 모양이라, 이 셋이 겹친 자리에서 정확히 통과했다.
#[test]
fn a_macro_arm_with_parentheses_is_caught() {
    let src = "\
fn route(request: &Request) -> Option<Response> {
    Some(match request.method.as_str() {
        \"ns.one\" => {
            one(request)
        }
        #[cfg(feature = \"gui\")]
        probe!() => {
            two(request)
        }
        \"ns.three\" => three(request),
        _ => return None,
    })
}
";
    let found = opaque_method_sites(src);
    assert_eq!(
        found.len(),
        1,
        "괄호를 가진 매크로 팔 하나만 걸려야 한다(리터럴 팔과 `_` 는 정상이다): {found:?}"
    );
}
