//! CountedOn의 임시 작업 커밋 사용 수가 늘지 않는지 확인한다.
//! 체리픽·이력 재작성 뒤 사라질 수 있는 작업 브랜치 SHA를 측정 근거로 남기는 일을 막기 위한 검사다.
//! Tree로 기록된 SHA의 객체 존재나 조상 관계는 검사하지 않는다. 얕은 체크아웃에서는 확인할 수 없기 때문이다.
//! 전체 이력을 받는 워크플로가 둘 이상이 되면 직접 도달성 검사 도입을 재검토한다.

use std::path::Path;
use tasty_doc_guards::floored_walk::{
    CountedOn, Descend, Floor, Walked, populations, walk_with_floor,
};

/// 저장소 Rust 파일 수집의 하한. 일부 누락이 임시 좌표 감소처럼 보이지 않도록 둔다.
const SOURCE_FLOOR: Floor = Floor {
    min: 1232,
    measured: 1343,
    measured_on: "2026-09-08",
    counted_on: CountedOn::Tree(
        "12bc0f4b2 + 당시 작업 커밋 — 이력 재작성으로 현재 main에서 도달할 수 없는 과거 측정이다.",
    ),
    why_this_gap: "당시 Rust 파일 수를 기준으로, 크레이트 하나가 삭제될 때의 여유111을 뒀다. 측정 당시 가장 큰 크레이트 tasty-gallery의 파일 수111에 해당한다. 과거 감소 폭을 따로 측정한 배수는 아니다.",
};

/// 임시 좌표 사용 수의 기준. 재측정으로 줄었다면 기준도 함께 낮춘다.
const LANE_TIP_SITES: usize = 1;

/// 검색 문자열 자체가 집계되지 않도록 enum 이름을 조각으로 조립한다.
fn marker(variant: &str) -> String {
    format!("Counted{}::{}", "On", variant)
}

fn sources(root: &Path) -> Vec<Walked> {
    walk_with_floor(
        root,
        root,
        &SOURCE_FLOOR,
        Descend::SkipBuildCachesAndDotDirs,
        &|w: &Walked| w.rel.ends_with(".rs"),
    )
    .unwrap_or_else(|why| panic!("{why}"))
}

/// 정의·match가 있는 floored_walk와 문서 주석 줄은 소비자로 세지 않는다.
fn sites(root: &Path, variant: &str) -> Vec<String> {
    let needle = marker(variant);
    let mut out = Vec::new();
    for w in sources(root) {
        if w.rel.ends_with("src/floored_walk.rs") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&w.path) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("///") || line.trim_start().starts_with("//!") {
                continue;
            }
            for _ in 0..line.matches(&needle).count() {
                out.push(format!("{}:{}", w.rel, i + 1));
            }
        }
    }
    out
}

#[test]
fn temporary_coordinates_do_not_spread() {
    let root = &tasty_doc_guards::repo_root();
    let found = sites(root, "LaneTip");
    assert_eq!(
        found.len(),
        LANE_TIP_SITES,
        "임시 좌표가 {}곳으로 기준 {LANE_TIP_SITES}와 다르다.\n{}\n증가했다면 유지되는 base SHA와 필요한 변경 설명으로 측정 근거를 남긴다. 감소했다면 실제 재측정인지 확인하고 기준을 함께 낮춘다. 임시 좌표를 허용하려고 기준을 높이지 않는다.",
        found.len(),
        found.join("\n")
    );
}

#[test]
fn the_census_reads_real_sites_not_prose() {
    let root = &tasty_doc_guards::repo_root();
    // 실제 소비자가 있는 세 종류를 수집해 빈 결과가 아닌지 확인한다.
    for variant in ["Tree", "SyntheticTree", "NEVER_COUNTED"] {
        assert!(
            !sites(root, variant).is_empty(),
            "{variant} 사용을 찾지 못했다. 종류 이름 변경과 수집 오류를 확인한다."
        );
    }
    let mine: Vec<_> = sites(root, "LaneTip")
        .into_iter()
        .filter(|s| s.contains("floor_coordinates_are_permanent.rs"))
        .collect();
    assert!(
        mine.is_empty(),
        "검사 파일 자신의 설명이 집계에 포함됐다: {mine:?}"
    );
}

#[test]
fn the_population_that_this_file_moves_is_current() {
    // 측정값을 소비하는 검사도 통합 타깃 수에 포함되므로 새 파일 추가 시 기준을 갱신해야 한다.
    let root = &tasty_doc_guards::repo_root();
    let mut n = 0;
    for w in sources(root) {
        let parts: Vec<&str> = w.rel.split('/').collect();
        if parts.len() == 4 && parts[0] == "crates" && parts[2] == "tests" {
            n += 1;
        }
    }
    assert_eq!(
        n,
        populations::CRATE_TEST_TARGETS.measured,
        "`crates/*/tests/*.rs` 가 {} 개인데 선언은 {} 이다. 이 파일이 그 모수를 움직였다면 \
         같은 커밋에서 갱신해라 — 값과 잰 트리를 함께.",
        n,
        populations::CRATE_TEST_TARGETS.measured
    );
}
