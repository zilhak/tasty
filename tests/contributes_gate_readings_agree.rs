//! `contributes` 게이트 표를 **두 방법으로 읽은 결과가 같은지** 붙박는다.
//!
//! 문서 대조 가드(`crates/tasty-doc-guards/tests/contributes_gate_docs_parity.rs`)는
//! 의존 0 크레이트에 산다 — 그래야 `doc-guards.yml` 이 **경로 필터 없이** 매 push 돌 수
//! 있다(ADR-0138). 그 표의 입력은 전부 `docs/**` 라, 필터가 걸린 잡에 두면 **그 문서만
//! 고치는 push 에서 정확히 안 돈다.**
//!
//! 대가로 그 가드는 `tasty-plugin-manifest` 를 링크하지 못해 표를 텍스트로 읽는다. 판독이
//! 진짜 표와 어긋나면 그 가드가 조용히 다른 것을 검사한다 — 문서 대조는 통과하는데 대조한
//! 대상이 표가 아닌 상태다. 이 테스트가 그 위험을 받는다: 여기는 본체 패키지라
//! `ContributesGate::ALL` 을 **런타임에 열거**할 수 있으므로 판독본과 열거본을 맞댄다.
//!
//! 채널이 갈리는 것이 맞다 — **판독기가 바뀌는 것은 소스 변경이라 이쪽 채널이 본다.**

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
        "런타임 열거가 비었다 — 이 대조는 그 상태에서 아무 뜻이 없다"
    );
    // 순서까지 같아야 한다. 표는 매크로 한 곳에서 나오므로 순서가 갈리면 그것도 판독 오류다.
    assert_eq!(
        text, link,
        "`contributes_gates!` 의 두 판독이 갈렸다. doc-guards 의 텍스트 판독기\
         (`tasty_doc_guards::manifest_text::contributes_gates`)가 낡았다는 뜻이고, 그 \
         크레이트의 문서 대조 가드가 조용히 다른 집합을 검사하고 있다."
    );
}

/// 토큰 표 쪽도 같은 위험을 진다 — 게이트의 문서 표기가 거기서 나온다.
#[test]
fn the_text_reading_resolves_every_token_the_link_does() {
    let src = std::fs::read_to_string(
        tasty_doc_guards::repo_root().join("crates/tasty-plugin-manifest/src/types.rs"),
    )
    .expect("read types.rs");
    let tokens = tasty_doc_guards::manifest_text::permission_tokens(&src);
    // 게이트가 참조하는 variant 는 전부 표에 있어야 한다. 하나라도 빠지면 위 테스트가
    // panic 으로 죽지만, 그때 이유가 "게이트가 갈렸다" 로 잘못 읽히는 것을 막는다.
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
