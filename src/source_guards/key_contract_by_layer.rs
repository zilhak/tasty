//! 이름 표의 멱등 키 선언(`method_meta::KeyContract`)이 **실제 배선과 맞는가.**
//!
//! 선언은 대부분 `MethodEffect` 에서 유도되고(`Mutate` → 판 1 의 `Kept`), 손으로 적는 것은
//! 층이 다른 이름 두 무리뿐이다 — App 층이 끝내는 `Mutate`(판 2 의 `Kept`)와 GUI debug step 이
//! 끝내는 `Mutate`(`Outside`). 그 두 무리가 틀리면 client 가 거짓을 믿는다: App 층 이름을
//! 판 1 로 적으면 판 1 서버(App 층 보존소가 없다)에 키를 실어 재시도가 두 번째 실행이 되고,
//! debug step 이름을 `Kept` 로 적으면 보존소를 안 지나는 호출에 키를 싣는다.
//!
//! 그래서 이 가드는 **dispatch 본문이 부르는 이름**을 읽어 선언과 양방향으로 맞댄다.
//!
//! - GUI app_methods step(`ipc_step_app_methods` 본문)이 부르는 `Mutate` 의 집합
//!   = 표에서 판 [`KEY_KEPT_BY_APP_LAYER`] 로 선언된 이름의 집합.
//! - GUI debug step(`debug_methods.rs` · `window_required.rs`)이 부르는 `Mutate` 의 집합
//!   = 표에서 `Outside` 로 선언된 호스트 이름의 집합.
//! - 헤드리스 App 층 가로채기가 부르는 `Mutate` 는 전부 둘 중 하나로 선언돼 있다 — 그
//!   가로채기도 첫 줄에서 보존소를 지나지만(`run_app_layer`), 판 1 서버에는 그 층의
//!   보존소가 없었으므로 판 1 의 `Kept` 는 거짓이다.
//!
//! 이름 추출은 옆 가드(`headless_app_layer_coverage`)의 것을 그대로 쓴다 — 같은 본문에서
//! 같은 물음(그 층이 무엇을 부르는가)을 두 방식으로 답하면 갈린다. 끝이 `.` 인 리터럴은
//! prefix 판정이라 표의 이름으로 펼친다(`plugin.` → `plugin.install` 등).
//!
//! **텍스트로 읽는 한계**는 옆 가드와 같다 — 리터럴이 아닌 이름은 안 보인다. 그 사각을 이
//! 가드가 따로 좁히지 않는 이유는 옆 가드가 이미 "명부 밖에 이름이 사는가" 를 잰다는 것이다.

use std::collections::BTreeSet;

use tasty_ipc::method_meta::{
    DEBUG_METHODS, KEY_KEPT_BY_APP_LAYER, KeyContract, METHOD_TABLE, MethodEffect, MethodMeta,
};

use super::headless_app_layer_coverage::{headless_dispatch_code, method_literals};
use super::{fn_body, repo_root, strip_comments};

const GUI_APP_STEP: &str = "src/app/ipc/app_methods.rs";
const GUI_APP_FN: &str = "fn ipc_step_app_methods";
/// GUI debug step 의 두 파일. 파일 전체가 그 step 이다(모듈 doc 이 그렇게 적는다).
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

/// 시험 모듈을 뺀 본문 — 시험 안의 리터럴은 dispatch 가 아니다.
fn shipped(src: &str) -> &str {
    src.split("\n#[cfg(test)]").next().unwrap_or(src)
}

fn table() -> impl Iterator<Item = &'static (&'static str, MethodMeta)> {
    METHOD_TABLE.iter().chain(DEBUG_METHODS)
}

/// 리터럴 집합을 표의 `Mutate` 이름 집합으로 — prefix 리터럴은 펼친다.
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

/// GUI app_methods step 이 끝내는 `Mutate` 는 전부 판 2 이고, 판 2 는 그것뿐이다.
#[test]
fn the_app_layer_mutations_are_exactly_the_names_kept_from_version_two() {
    let answered = gui_app_step_mutations();
    // 좌변이 비면 아래 대조가 "둘 다 빈 집합" 으로 초록이 된다.
    assert!(
        answered.len() >= 6,
        "app_methods step 에서 Mutate 를 {} 개만 읽었다 — 추출이 망가졌다: {answered:?}",
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

/// GUI debug step 이 끝내는 `Mutate` 는 전부 `Outside` 이고, 호스트 이름의 `Outside` 는
/// 그것뿐이다. release 빌드에서는 debug 표가 비어 양쪽이 다 빈 집합이다.
#[test]
fn the_debug_step_mutations_are_exactly_the_host_names_outside_the_contract() {
    let answered = gui_debug_step_mutations();
    if cfg!(debug_assertions) {
        assert!(
            !answered.is_empty(),
            "debug step 에서 Mutate 를 하나도 못 읽었다 — 추출이 망가졌다"
        );
    }
    let declared = declared(|c| c == KeyContract::Outside);
    assert_eq!(
        answered, declared,
        "GUI debug step 이 끝내는 Mutate(왼쪽)와 `Outside` 선언(오른쪽)이 다르다. debug step 은 \
         app_methods step 뒤라 보존소를 안 지난다 — 거기 Mutate 를 더했으면 표에서 \
         `.outside_key_contract()` 를 붙인다"
    );
}

/// 헤드리스 App 층 가로채기가 끝내는 `Mutate` 는 판 1 로 선언돼 있지 않다.
#[test]
fn a_headless_app_layer_mutation_is_never_declared_kept_from_version_one() {
    let answered = mutating(&method_literals(&headless_dispatch_code()));
    assert!(
        answered.contains("plugin.request_permission"),
        "헤드리스 가로채기에서 plugin.request_permission 을 못 읽었다 — 추출이 망가졌다: {answered:?}"
    );
    for name in answered {
        let contract = table()
            .find(|(n, _)| *n == name)
            .map(|(_, m)| m.key_contract)
            .expect("mutating() 은 표의 이름만 낸다");
        assert!(
            kept_by_app_layer(contract) || contract == KeyContract::Outside,
            "{name}: 헤드리스 App 층이 끝내는데 {contract:?} 로 선언됐다"
        );
    }
}
