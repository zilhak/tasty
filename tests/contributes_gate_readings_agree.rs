//! doc-guards의 텍스트 판독 결과를 ContributesGate::ALL의 실제 열거와 대조한다.
//! 문서 검사 크레이트는 플러그인 매니페스트에 의존하지 않으므로 여기서 두 목록의 일치를 확인한다.

use tasty_plugin_manifest::ContributesGate;

fn parsed() -> Vec<(String, String)> {
    let root = tasty_doc_guards::repo_root();
    let read = |rel: &str| {
        let p = root.join(rel);
        std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
    };
    tasty_doc_guards::manifest_text::contributes_gates(
        &read("crates/tasty-plugin-manifest/src/gates.rs"),
        &read("crates/tasty-plugin-manifest/src/types.rs"),
    )
}

fn linked() -> Vec<(String, String)> {
    ContributesGate::ALL
        .iter()
        .map(|g| (g.contributes_key().to_string(), g.token().doc_form()))
        .collect()
}

#[test]
fn the_two_readings_name_exactly_the_same_gates() {
    let (text, link) = (parsed(), linked());
    assert!(
        !link.is_empty(),
        "실제 게이트 목록이 비어 있어 판독 결과와 비교할 수 없다"
    );
    assert_eq!(
        text, link,
        "contributes 게이트의 텍스트 판독 결과와 실제 열거가 다르다. tasty_doc_guards::manifest_text::contributes_gates의 판독 범위를 확인한다."
    );
}

#[test]
fn the_text_reading_resolves_every_token_the_link_does() {
    let src = std::fs::read_to_string(
        tasty_doc_guards::repo_root().join("crates/tasty-plugin-manifest/src/types.rs"),
    )
    .expect("read types.rs");
    let tokens = tasty_doc_guards::manifest_text::permission_tokens(&src);
    for gate in ContributesGate::ALL {
        let doc_form = gate.token().doc_form();
        assert!(
            tokens.values().any(|f| match f {
                tasty_doc_guards::manifest_text::TokenForm::Literal(t) => *t == doc_form,
                tasty_doc_guards::manifest_text::TokenForm::Prefixed(p) => doc_form.starts_with(p),
            }),
            "게이트 {} 의 문서 표기 `{doc_form}` 을 텍스트 판독의 토큰 표에서 못 찾았다",
            gate.contributes_key()
        );
    }
}
