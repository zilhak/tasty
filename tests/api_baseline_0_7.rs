//! 0.7.0 시점 METHOD_TABLE snapshot. 0.7.x 동안 *추가만 가능, 제거 금지*.
//! 메서드 제거/이름 변경은 SemVer 위반 — major bump (2.0.0) 가 필요.
//!
//! baseline fixture: `tests/fixtures/method_baseline_0_7.txt`
//! 정책: `docs/dev-guide/release.md` §"0.7.x 패치 release 가드"

use std::collections::HashSet;

use tasty_ipc::method_meta::METHOD_TABLE;

const BASELINE_0_7: &str = include_str!("fixtures/method_baseline_0_7.txt");

fn baseline_methods() -> Vec<&'static str> {
    BASELINE_0_7
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .collect()
}

#[test]
fn all_baseline_methods_still_registered() {
    let current: HashSet<&str> = METHOD_TABLE.iter().map(|(name, _)| *name).collect();
    let missing: Vec<&str> = baseline_methods()
        .into_iter()
        .filter(|name| !current.contains(name))
        .collect();
    assert!(
        missing.is_empty(),
        "v0.7.0 baseline 의 다음 메서드가 METHOD_TABLE 에서 사라짐 — \
         minor/patch 에서 메서드 제거는 SemVer 위반. major bump (2.0.0) 가 필요. \
         major bump 이라면 tests/fixtures/method_baseline_0_7.txt 를 갱신할 것: {missing:?}"
    );
}

#[test]
fn baseline_file_is_sorted_and_unique() {
    let methods = baseline_methods();
    let mut sorted = methods.clone();
    sorted.sort_unstable();
    assert_eq!(
        methods, sorted,
        "tests/fixtures/method_baseline_0_7.txt 는 sort 된 상태여야 한다"
    );
    let dedup: HashSet<&str> = methods.iter().copied().collect();
    assert_eq!(
        methods.len(),
        dedup.len(),
        "tests/fixtures/method_baseline_0_7.txt 에 중복 메서드 존재"
    );
}
