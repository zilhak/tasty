//! Freshness 가드 — vendor json ↔ 커밋된 `src/generated/*.rs` +
//! `tasty-type-appearance/src/generated_component.rs` 텍스트 일치 강제.
//!
//! vendor json 을 갱신하고 생성기 재실행을 잊으면 여기서 fail 한다 — 통합 테스트라
//! 자동 실행 채널이 없으니(컴파일만 자동 검사: `docs/dev-guide/ci-gates.md`) 직접 돌려라
//! (`tests/cli_naming_count_drift.rs` 의 스냅샷 가드 패턴).

use tasty_design_tokens::{DTCG_JSON, dtcg};

/// 커밋된 생성물. 생성기 출력 순서(`Generated::files`)와 같은 순서.
const COMMITTED: &[(&str, &str)] = &[
    ("mod.rs", include_str!("../src/generated/mod.rs")),
    (
        "primitive.rs",
        include_str!("../src/generated/primitive.rs"),
    ),
    ("semantic.rs", include_str!("../src/generated/semantic.rs")),
    (
        "component.rs",
        include_str!("../src/generated/component.rs"),
    ),
];

/// 커밋된 `tasty-type-appearance` 산출물. `Generated::type_appearance_files` 와 같은 순서.
const COMMITTED_TYPE_APPEARANCE: &[(&str, &str)] = &[
    (
        "semantic_color_generated.rs",
        include_str!("../../tasty-type-appearance/src/semantic_color_generated.rs"),
    ),
    (
        "generated_component.rs",
        include_str!("../../tasty-type-appearance/src/generated_component.rs"),
    ),
];

/// 토큰 census — 751 (115/137/499). 디자인 측이 `tokens/tasty.tokens.json` export 를
/// 세 CSS 파일로부터 통째로 재생성해 CSS ↔ JSON parity 를 복구한 결과, 그동안 export
/// 에만 빠져 있던 209종이 한꺼번에 들어와 이전 542(111/131/300) 에서 증가했다
/// (primitive 4 · semantic 6 · component 199 — DAG surface 블록 `dag-*` 102종 전부 포함).
/// 제거·개명은 없다(이전 키 집합은 새 export 의 진부분집합).
/// vendor 갱신으로 개수가 바뀌면 의식적으로 이 스냅샷도 갱신한다.
#[test]
fn token_census_matches_design_export() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    assert_eq!(
        set.tier_count(dtcg::Tier::Primitive),
        115,
        "primitive census drift"
    );
    assert_eq!(
        set.tier_count(dtcg::Tier::Semantic),
        137,
        "semantic census drift"
    );
    assert_eq!(
        set.tier_count(dtcg::Tier::Component),
        499,
        "component census drift"
    );
    assert_eq!(set.len(), 751, "total census drift");
}

/// in-memory 재생성 결과가 커밋된 생성물 텍스트와 완전히 일치해야 한다.
#[test]
fn committed_generated_files_are_fresh() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    let generated = dtcg::generate(&set);
    assert_eq!(
        generated.files.len(),
        COMMITTED.len(),
        "생성 파일 목록이 바뀜 — 테스트의 COMMITTED 목록도 갱신할 것"
    );
    for ((name, fresh), (committed_name, committed)) in generated.files.iter().zip(COMMITTED) {
        assert_eq!(name, committed_name, "생성 파일 순서/이름 불일치");
        assert_fresh(&format!("src/generated/{name}"), fresh, committed);
    }

    assert_eq!(
        generated.type_appearance_files.len(),
        COMMITTED_TYPE_APPEARANCE.len(),
        "type-appearance 산출 파일 목록이 바뀜 — 테스트의 COMMITTED_TYPE_APPEARANCE 목록도 갱신할 것"
    );
    for ((name, fresh), (committed_name, committed)) in generated
        .type_appearance_files
        .iter()
        .zip(COMMITTED_TYPE_APPEARANCE)
    {
        assert_eq!(name, committed_name, "생성 파일 순서/이름 불일치");
        assert_fresh(
            &format!("../tasty-type-appearance/src/{name}"),
            fresh,
            committed,
        );
    }
}

/// 재생성 텍스트와 커밋 텍스트를 비교, 어긋나면 첫 불일치 행만 짚어 panic.
///
/// Windows autocrlf 체크아웃에서는 `include_str!` 가 CRLF 를 읽어오므로
/// 비교 전 양쪽을 `\n` 으로 정규화한다 — freshness 는 내용 드리프트를
/// 잡는 가드이지 line ending 가드가 아니다.
fn assert_fresh(label: &str, fresh: &str, committed: &str) {
    let fresh = fresh.replace("\r\n", "\n");
    let committed = committed.replace("\r\n", "\n");
    if fresh != committed {
        let first_diff = fresh
            .lines()
            .zip(committed.lines())
            .position(|(a, b)| a != b)
            .map(|i| i + 1);
        let detail = match first_diff {
            Some(line) => format!("첫 불일치: {line}행"),
            None => "행 내용은 모두 일치 — 행 수/말미 개행 차이".to_string(),
        };
        panic!(
            "{label} 이 vendor json 과 어긋남 ({detail}, \
             재생성 {}줄 vs 커밋 {}줄) — `cargo run -p tasty-design-tokens --bin generate` \
             실행 후 결과를 커밋할 것",
            fresh.lines().count(),
            committed.lines().count(),
        );
    }
}
