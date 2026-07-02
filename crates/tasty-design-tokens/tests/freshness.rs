//! Freshness 가드 — vendor json ↔ 커밋된 `src/generated/*.rs` 텍스트 일치 강제.
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

/// 토큰 census — 디자인 changelog 2026-07-02 기준 488 (104/127/257).
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
        127,
        "semantic census drift"
    );
    assert_eq!(
        set.tier_count(dtcg::Tier::Component),
        257,
        "component census drift"
    );
    assert_eq!(set.len(), 488, "total census drift");
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
        if fresh != committed {
            // 전체 diff 덤프 대신 첫 불일치 라인만 짚는다.
            let first_diff = fresh
                .lines()
                .zip(committed.lines())
                .position(|(a, b)| a != b)
                .map(|i| i + 1);
            panic!(
                "src/generated/{name} 가 vendor json 과 어긋남 (첫 불일치: {first_diff:?}행, \
                 재생성 {}줄 vs 커밋 {}줄) — `cargo run -p tasty-design-tokens --bin generate` \
                 실행 후 결과를 커밋할 것",
                fresh.lines().count(),
                committed.lines().count(),
            );
        }
    }
}
