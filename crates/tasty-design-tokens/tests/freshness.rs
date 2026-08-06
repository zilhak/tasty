//! Freshness 가드 — vendor json ↔ 커밋된 `src/generated/*.rs` +
//! `tasty-type-appearance/src/generated_component.rs` 텍스트 일치 강제.
//!
//! vendor json 을 갱신하고 생성기 재실행을 잊으면 여기서 CI fail 한다
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

/// 토큰 census — 492 (104/129/259). semantic 129 = 기존 131 에서
/// `font-size-prose-h2`·`line-height-prose` 2종 제거 — egui_commonmark 이 헤딩 보간·본문
/// leading 을 소유해 소비처 소멸 → 은퇴 확정.
/// vendor 갱신으로 개수가 바뀌면 의식적으로 이 스냅샷도 갱신한다.
#[test]
fn token_census_matches_design_export() {
    let set = dtcg::parse(DTCG_JSON).expect("vendor json must parse");
    assert_eq!(
        set.tier_count(dtcg::Tier::Primitive),
        104,
        "primitive census drift"
    );
    assert_eq!(
        set.tier_count(dtcg::Tier::Semantic),
        129,
        "semantic census drift"
    );
    assert_eq!(
        set.tier_count(dtcg::Tier::Component),
        259,
        "component census drift"
    );
    assert_eq!(set.len(), 492, "total census drift");
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
