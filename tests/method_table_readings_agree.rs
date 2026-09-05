//! `METHOD_TABLE` 을 **두 방법으로 읽은 결과가 같은지** 붙박는다.
//!
//! 왜 두 방법이 있나: 권한 문서를 대조하는 가드 둘은 `crates/tasty-doc-guards` 에 산다.
//! 그 크레이트는 의존이 0 이라 콜드 빌드가 1 초 미만이고, 그래서 `doc-guards.yml` 이
//! **경로 필터 없이** 매 push 돌 수 있다(ADR-0138). 대가로 그 가드들은 `tasty_ipc` 를
//! 링크하지 못해 `crates/tasty-ipc/src/method_meta.rs` 를 **텍스트로 읽는다.**
//!
//! 그 판독이 진짜 표와 어긋나면 두 가드가 조용히 다른 것을 검사하게 된다 — 문서 대조는
//! 통과하는데 대조한 대상이 표가 아닌 상태다. 이 테스트가 그 위험을 받는다: 여기는 본체
//! 패키지라 `METHOD_TABLE` 을 **런타임에 열거**할 수 있으므로, 판독본과 열거본을 직접
//! 맞댄다.
//!
//! 이 자리에 둔 이유가 곧 doc-guards 에 못 두는 이유다 — 링크가 필요하다. 그래서 채널도
//! 다르다: 이 테스트는 `check-headless`(경로 필터 뒤)에서 돌고, 저쪽 가드 둘은 필터
//! 없는 잡에서 돈다. **판독기가 바뀌는 것은 소스 변경이라 이쪽 채널이 본다.**

use std::collections::BTreeSet;

use tasty_ipc::method_meta::METHOD_TABLE;

fn parsed() -> std::collections::BTreeMap<String, Option<Vec<String>>> {
    let path = tasty_doc_guards::repo_root().join("crates/tasty-ipc/src/method_meta.rs");
    let src =
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    tasty_doc_guards::method_table(&src)
}

#[test]
fn the_two_readings_name_exactly_the_same_methods() {
    let runtime: BTreeSet<String> = METHOD_TABLE.iter().map(|(m, _)| (*m).to_string()).collect();
    let text: BTreeSet<String> = parsed().into_keys().collect();
    assert!(
        !runtime.is_empty(),
        "런타임 열거가 비었다 — 이 대조는 그 상태에서 아무 뜻이 없다"
    );
    let only_runtime: Vec<&String> = runtime.difference(&text).collect();
    let only_text: Vec<&String> = text.difference(&runtime).collect();
    assert!(
        only_runtime.is_empty() && only_text.is_empty(),
        "`METHOD_TABLE` 의 두 판독이 갈렸다. doc-guards 의 텍스트 판독기\
         (`tasty_doc_guards::method_table`)가 낡았다는 뜻이고, 그 크레이트의 권한 문서 \
         가드 둘이 조용히 다른 집합을 검사하고 있다.\n  \
         링크에만 있음: {only_runtime:?}\n  텍스트에만 있음: {only_text:?}"
    );
}

#[test]
fn the_text_reading_agrees_on_which_methods_plugins_may_call() {
    let text = parsed();
    let mut mismatched = Vec::new();
    for (name, meta) in METHOD_TABLE.iter() {
        let Some(required) = text.get(*name) else {
            // `the_two_readings_name_exactly_the_same_methods` 가 담당한다 — 그 테스트를
            // 지우면 이 `continue` 가 조용한 구멍이 된다.
            continue;
        };
        let text_callable = required.is_some();
        if text_callable != meta.plugin_callable {
            mismatched.push(format!(
                "{name}: 링크 plugin_callable={} / 텍스트={text_callable}",
                meta.plugin_callable
            ));
        }
    }
    assert!(
        mismatched.is_empty(),
        "두 판독이 `plugin_callable` 에서 갈렸다 — 텍스트 판독기가 생성자를 잘못 해석한다:\
         \n  {}",
        mismatched.join("\n  ")
    );
}
