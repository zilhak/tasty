//! 권한 표의 텍스트 판독 결과를 링크한 METHOD_TABLE의 실제 열거와 대조한다.
//! doc-guards는 tasty_ipc에 의존하지 않으므로 이 루트 타깃에서 두 목록과 plugin_callable 값을 비교한다.

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
        "실제 메서드 목록이 비어 텍스트 판독 결과와 비교할 수 없다"
    );
    let only_runtime: Vec<&String> = runtime.difference(&text).collect();
    let only_text: Vec<&String> = text.difference(&runtime).collect();
    assert!(
        only_runtime.is_empty() && only_text.is_empty(),
        "METHOD_TABLE의 텍스트 판독 결과와 실제 열거가 다르다. tasty_doc_guards::method_table의 판독 범위를 확인한다.\n  링크에만: {only_runtime:?}\n  텍스트에만: {only_text:?}"
    );
}

#[test]
fn the_text_reading_agrees_on_which_methods_plugins_may_call() {
    let text = parsed();
    let mut mismatched = Vec::new();
    for (name, meta) in METHOD_TABLE.iter() {
        let Some(required) = text.get(*name) else {
            // 키 누락은 별도 the_two_readings_name_exactly_the_same_methods 시험에서 검사한다.
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
