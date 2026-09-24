//! 멱등 키의 지원 버전 선언을 GUI·헤드리스 라우터의 메서드 목록과 대조한다.
//! 잘못된 버전을 선언하면 클라이언트가 키를 보존하지 않는 서버에 재시도를 보낼 수 있다.
//! GUI app의 Mutate는 버전 2, GUI debug와 namespace 전달 경로는 버전 3을 요구한다.
//! 헤드리스 app 가로채기도 버전 1로 선언돼서는 안 된다.
//!
//! debug 처리 전에 보존소를 호출하는지, namespace 전달이 키 보존 함수의 인자 안에 있는지 텍스트로 확인한다.
//! relay 클로저는 전달받은 명령을 쓰고 바깥 명령 이름은 쓰지 않아야 한다. 그렇지 않으면
//! 키를 제거한 명령과 결과 기록용 응답 경로를 우회할 수 있다.
//!
//! handled 클로저에서는 인자 이름과 IpcStep::Handled의 존재만 확인한다. 실제 비교식이나 분기 결과를
//! 해석하지 않으며, 실행 결과는 idempotency의 별도 시험에서 검증한다(ADR-0005).
//! 메서드 이름은 리터럴로 수집하고 prefix를 표의 이름으로 펼친다. 비리터럴 이름은 여기서 보이지 않는다.

use std::collections::BTreeSet;

use tasty_ipc::method_meta::{
    DEBUG_METHODS, KEY_KEPT_BY_APP_LAYER, KEY_KEPT_ON_EVERY_HOST_PATH, KeyContract, METHOD_TABLE,
    MethodEffect, MethodMeta,
};

use super::headless_app_layer_coverage::{headless_dispatch_code, method_literals};
use super::{fn_body, mask_non_code, repo_root, strip_comments, word_positions};

const GUI_DISPATCH: &str = "src/app/ipc.rs";
const GUI_ROUTING: &str = "src/app/ipc/routing.rs";
const HEADLESS_DISPATCH: &str = "src/boot/headless_dispatch.rs";
const GUI_APP_STEP: &str = "src/app/ipc/app_methods.rs";
const GUI_APP_FN: &str = "fn ipc_step_app_methods";
/// GUI debug 메서드 이름을 수집할 파일들.
const GUI_DEBUG_STEPS: &[&str] = &[
    "src/app/ipc/debug_methods.rs",
    "src/app/ipc/window_required.rs",
];

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("{rel} 을 읽지 못했다: {e}"))
        .replace("\r\n", "\n")
}

/// 첫 줄 시작의 #[cfg(test)] 앞까지만 읽는다. 모든 cfg 형태를 분석하는 것은 아니다.
fn shipped(src: &str) -> &str {
    src.split("\n#[cfg(test)]").next().unwrap_or(src)
}

fn table() -> impl Iterator<Item = &'static (&'static str, MethodMeta)> {
    METHOD_TABLE.iter().chain(DEBUG_METHODS)
}

fn mutating(literals: &BTreeSet<String>) -> BTreeSet<&'static str> {
    table()
        .filter(|(_, m)| m.effect == MethodEffect::Mutate)
        .map(|(name, _)| *name)
        .filter(|name| {
            literals.iter().any(|lit| {
                if lit.ends_with('.') {
                    name.starts_with(lit.as_str())
                } else {
                    lit == name
                }
            })
        })
        .collect()
}

fn declared(pred: impl Fn(KeyContract) -> bool) -> BTreeSet<&'static str> {
    table()
        .filter(|(_, m)| pred(m.key_contract))
        .map(|(name, _)| *name)
        .collect()
}

fn kept_by_app_layer(c: KeyContract) -> bool {
    c == KeyContract::Kept {
        since: KEY_KEPT_BY_APP_LAYER,
    }
}

fn kept_on_every_host_path(c: KeyContract) -> bool {
    c == KeyContract::Kept {
        since: KEY_KEPT_ON_EVERY_HOST_PATH,
    }
}

fn gui_app_step_mutations() -> BTreeSet<&'static str> {
    let src = read(GUI_APP_STEP);
    let body = fn_body(&src, GUI_APP_FN)
        .unwrap_or_else(|| panic!("{GUI_APP_STEP} 에서 `{GUI_APP_FN}` 본문을 못 잘랐다"));
    mutating(&method_literals(&strip_comments(&body)))
}

fn gui_debug_step_mutations() -> BTreeSet<&'static str> {
    let mut lits = BTreeSet::new();
    for rel in GUI_DEBUG_STEPS {
        let src = read(rel);
        lits.extend(method_literals(&strip_comments(shipped(&src))));
    }
    mutating(&lits)
}

#[test]
fn the_app_layer_mutations_are_exactly_the_names_kept_from_version_two() {
    let answered = gui_app_step_mutations();
    assert!(
        answered.len() >= 6,
        "app_methods에서 Mutate를 {}개만 읽었다. 추출 범위를 확인한다: {answered:?}",
        answered.len()
    );
    let declared = declared(kept_by_app_layer);
    assert_eq!(
        answered, declared,
        "App 층이 끝내는 Mutate(왼쪽)와 판 {KEY_KEPT_BY_APP_LAYER} 선언(오른쪽)이 다르다. \
         App 층에 Mutate 를 더했으면 표에서 `.kept_by_app_layer()` 를 붙이고, engine \
         라우터로 옮겼으면 뗀다"
    );
}

/// release 빌드에서는 debug 표가 비어 양쪽 집합이 비는 것을 허용한다.
#[test]
fn the_debug_step_mutations_are_exactly_the_debug_names_kept_from_version_three() {
    let answered = gui_debug_step_mutations();
    if cfg!(debug_assertions) {
        assert!(
            !answered.is_empty(),
            "debug step의 Mutate를 찾지 못했다. 추출 범위를 확인한다."
        );
    }
    let declared: BTreeSet<&'static str> = DEBUG_METHODS
        .iter()
        .filter(|(_, m)| kept_on_every_host_path(m.key_contract))
        .map(|(name, _)| *name)
        .collect();
    assert_eq!(
        answered, declared,
        "GUI debug step 이 끝내는 Mutate(왼쪽)와 debug 표의 판 {KEY_KEPT_ON_EVERY_HOST_PATH} \
         선언(오른쪽)이 다르다. debug step 에 Mutate 를 더했으면 표에서 \
         `.kept_on_every_host_path()` 를 붙인다"
    );
}

#[test]
fn a_headless_app_layer_mutation_is_never_declared_kept_from_version_one() {
    let answered = mutating(&method_literals(&headless_dispatch_code()));
    assert!(
        answered.contains("plugin.request_permission"),
        "헤드리스 app에서 plugin.request_permission을 찾지 못했다. 추출 범위를 확인한다: {answered:?}"
    );
    for name in answered {
        let contract = table()
            .find(|(n, _)| *n == name)
            .map(|(_, m)| m.key_contract)
            .expect("mutating() 은 표의 이름만 낸다");
        assert!(
            kept_by_app_layer(contract) || kept_on_every_host_path(contract),
            "{name}: 헤드리스 App 층이 끝내는데 {contract:?} 로 선언됐다"
        );
    }
}

fn call_args<'a>(src: &'a str, call: &str) -> Option<&'a str> {
    let at = src.find(call)? + call.len();
    let mut depth = 1usize;
    for (i, c) in src[at..].char_indices() {
        match c {
            '(' => depth += 1,
            ')' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&src[at..at + i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// 직접 debug step을 부르는 우회를 금지하고 묶음 함수 안의 보존소 호출 위치를 비교한다.
#[test]
fn the_gui_debug_steps_run_behind_the_store() {
    let src = strip_comments(shipped(&read(GUI_DISPATCH)));
    let dispatch = fn_body(&src, "fn ipc_dispatch_command")
        .unwrap_or_else(|| panic!("{GUI_DISPATCH} 에서 `ipc_dispatch_command` 를 못 잘랐다"));
    assert!(
        dispatch.contains("ipc_step_debug_layers("),
        "dispatch 가 debug 묶음을 안 부른다"
    );
    for step in ["ipc_step_debug(", "ipc_step_window_required("] {
        assert!(
            !dispatch.contains(step),
            "dispatch가 `{step}`을 debug 묶음 밖에서 부른다. 해당 호출이 키 보존소를 우회하지 않도록 묶음 함수로 전달한다."
        );
    }
    let layers = fn_body(&src, "fn ipc_step_debug_layers")
        .unwrap_or_else(|| panic!("{GUI_DISPATCH} 에서 `ipc_step_debug_layers` 를 못 잘랐다"));
    let store = layers
        .find("run_app_layer(")
        .expect("debug 묶음이 보존소(`run_app_layer`)를 안 부른다");
    for step in ["ipc_step_debug(", "ipc_step_window_required("] {
        let at = layers
            .find(step)
            .unwrap_or_else(|| panic!("debug 묶음이 `{step}` 를 안 부른다"));
        assert!(
            store < at,
            "debug 묶음이 보존소보다 `{step}` 를 먼저 부른다"
        );
    }
}

/// namespace 전달이 보존소 밖에서 실행되면 멱등 키를 보존하지 못할 수 있어 인자 안에 있는지 확인한다.
#[test]
fn both_namespace_forwards_go_through_the_store() {
    for rel in [GUI_ROUTING, HEADLESS_DISPATCH] {
        let src = strip_comments(shipped(&read(rel)));
        assert_eq!(
            src.matches("forward_namespace_call(").count(),
            1,
            "{rel}: forward 자리가 하나가 아니다"
        );
        let args = call_args(&src, "forward_keeping_the_key(")
            .unwrap_or_else(|| panic!("{rel}: `forward_keeping_the_key` 를 안 부른다"));
        assert!(
            args.contains("forward_namespace_call("),
            "{rel}: `forward_namespace_call` 이 보존소 함수 밖에 있다"
        );
    }
}

fn top_level_args(args: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut depth = 0usize;
    let mut from = 0usize;
    for (i, c) in args.char_indices() {
        match c {
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            ',' if depth == 0 => {
                out.push(&args[from..i]);
                from = i + 1;
            }
            _ => {}
        }
    }
    if !args[from..].trim().is_empty() {
        out.push(&args[from..]);
    }
    out
}

fn closure_parts(arg: &str) -> Option<(&str, &str)> {
    let s = arg.trim();
    let s = s.strip_prefix("move").map_or(s, str::trim_start);
    let s = s.strip_prefix('|')?;
    let end = s.find('|')?;
    let param = s[..end].split(':').next()?.trim();
    Some((param, &s[end + 1..]))
}

struct StoreCall {
    rel: &'static str,
    scope_fn: Option<&'static str>,
    call: &'static str,
}

const STORE_CALLS: &[StoreCall] = &[
    StoreCall {
        rel: GUI_ROUTING,
        scope_fn: None,
        call: "forward_keeping_the_key(",
    },
    StoreCall {
        rel: HEADLESS_DISPATCH,
        scope_fn: None,
        call: "forward_keeping_the_key(",
    },
    StoreCall {
        rel: GUI_DISPATCH,
        scope_fn: Some("fn ipc_step_debug_layers"),
        call: "run_app_layer(",
    },
];

fn store_call_args(site: &StoreCall) -> Vec<String> {
    let masked = mask_non_code(shipped(&read(site.rel)));
    let scope = match site.scope_fn {
        Some(sig) => fn_body(&masked, sig)
            .unwrap_or_else(|| panic!("{}: `{sig}` 본문을 못 잘랐다", site.rel)),
        None => masked,
    };
    let args = call_args(&scope, site.call)
        .unwrap_or_else(|| panic!("{}: `{}` 를 안 부른다", site.rel, site.call));
    top_level_args(args)
        .into_iter()
        .map(str::to_string)
        .collect()
}

/// 키를 제거하고 응답을 기록하는 명령 대신 바깥 명령을 쓰지 않는지 이름으로 검사한다. shadowing과 값의 흐름까지 해석하지 않는다.
#[test]
fn the_relay_closure_uses_only_its_own_argument() {
    for site in STORE_CALLS {
        let args = store_call_args(site);
        assert!(
            args.len() >= 3,
            "{}의 `{}` 호출 인자를 {}개만 읽었다. 추출을 확인한다: {args:?}",
            site.rel,
            site.call,
            args.len()
        );
        let outer = args[1].trim();
        assert!(
            !outer.is_empty() && outer.chars().all(|c| c.is_alphanumeric() || c == '_'),
            "{}: 보존소에 넘기는 명령이 바인딩 하나가 아니다(`{outer}`) — 이 가드의 전제가 깨졌다",
            site.rel
        );
        let last = args.last().expect("길이를 위에서 쟀다");
        let (param, body) = closure_parts(last).unwrap_or_else(|| {
            panic!(
                "{}: 마지막 인자가 `|인자| 본문` closure 가 아니다: `{}`",
                site.rel,
                last.trim()
            )
        });
        assert!(
            !param.starts_with('_') && !word_positions(body, param).is_empty(),
            "{}의 relay 클로저가 인자 `{param}`을 사용하지 않는다. 키를 제거한 명령과 응답 기록 경로를 사용해야 한다.",
            site.rel
        );
        assert!(
            word_positions(body, outer).is_empty(),
            "{}의 relay 클로저가 바깥 명령 `{outer}`를 쓴다. 원래 응답 경로로 결과를 보내지 않도록 전달받은 `{param}`을 사용한다.",
            site.rel
        );
    }
}

/// 처리 결과를 무조건 handled로 판단하지 않도록 인자와 Handled 이름의 존재를 확인한다. 두 값의 실제 비교까지 증명하지는 않는다.
#[test]
fn the_debug_layers_judge_handled_by_the_step() {
    let site = STORE_CALLS
        .iter()
        .find(|s| s.rel == GUI_DISPATCH)
        .expect("위 표에 debug 묶음이 있다");
    let args = store_call_args(site);
    // run_app_layer의 넷째 인자가 handled 판정이다.
    assert_eq!(
        args.len(),
        5,
        "{}: `run_app_layer` 인자가 다섯이 아니다 — 시그니처가 바뀌었으면 이 가드를 같이 고친다: {args:?}",
        site.rel
    );
    let (param, body) = closure_parts(&args[3]).unwrap_or_else(|| {
        panic!(
            "{}: 판정 인자가 closure 가 아니다: `{}`",
            site.rel,
            args[3].trim()
        )
    });
    assert!(
        !param.starts_with('_') && !word_positions(body, param).is_empty(),
        "{}의 handled 판정 클로저가 결과 인자 `{param}`을 사용하지 않는다",
        site.rel
    );
    // 경로 내부 공백은 무시하되 HandledDirty와 같은 다른 식별자는 제외한다.
    let compact: String = body.chars().filter(|c| !c.is_whitespace()).collect();
    let handled = !word_positions(&compact, "IpcStep::Handled").is_empty();
    assert!(
        handled,
        "{}: 판정 closure 가 `IpcStep::Handled` 를 안 본다: `{}`",
        site.rel,
        body.trim()
    );
}
