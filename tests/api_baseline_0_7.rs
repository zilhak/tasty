//! 0.7.0 시점 METHOD_TABLE snapshot. 0.7.x 동안 *추가만 가능, 제거 금지*.
//! 메서드 제거/이름 변경은 SemVer 위반 — major bump (2.0.0) 가 필요.
//!
//! baseline fixture: `tests/fixtures/method_baseline_0_7.txt`
//! 정책: `docs/dev-guide/release.md` §"0.7.x 패치 release 가드"

use std::collections::HashSet;

use tasty_ipc::method_meta::METHOD_TABLE;

const BASELINE_0_7: &str = include_str!("fixtures/method_baseline_0_7.txt");

/// baseline 이 담은 메서드 수. **하한이 아니라 고정값이다.**
///
/// 이 fixture 는 0.7.0 시점에 박제됐고 0.7.x 동안 바뀌지 않는다 — 바뀌는 것은
/// `METHOD_TABLE` 쪽이고, 이 파일이 바뀌어야 하는 유일한 경우는 major bump 다. 모수가
/// 그렇게 **동결**돼 있으므로 하한으로 물러설 이유가 없다: 하한은 "이만큼은 봤다" 까지만
/// 말하고 그 위의 사각을 남기는데, 여기서는 정확한 수를 알고 그 수가 변하면 안 된다.
///
/// 값의 근거: 2026-09-05 실측 191(주석·빈 줄을 걷어낸 뒤). 이 수가 안 맞으면 fixture 가
/// 편집됐다는 뜻이고, 그것은 SemVer 판정 자체가 바뀌었다는 뜻이라 조용히 넘어갈 일이 아니다.
const BASELINE_METHOD_COUNT: usize = 191;

/// fixture 에서 메서드 이름을 뽑는다.
///
/// **모수 단언이 여기 있는 이유**: 아래 두 테스트 모두 이 목록을 모수로 쓰는데, 목록이
/// 비면 둘 다 검사할 것이 없어져 **조용히 통과한다.** 실측으로 확인했다 — fixture 를 0 줄로
/// 만들면 두 테스트가 전부 초록이었다. 모수를 만드는 자리에서 한 번 단언하면 소비자가
/// 늘어도 그 구멍이 다시 생기지 않는다.
fn baseline_methods() -> Vec<&'static str> {
    let methods: Vec<&'static str> = BASELINE_0_7
        .lines()
        .map(|s| s.trim())
        .filter(|s| !s.is_empty() && !s.starts_with('#'))
        .collect();
    assert_eq!(
        methods.len(),
        BASELINE_METHOD_COUNT,
        "baseline fixture 가 메서드 {} 개를 냈다 — 0.7.0 박제 시점의 {BASELINE_METHOD_COUNT} 개와 다르다. \
         fixture 는 0.7.x 동안 동결이므로 이 수가 변하는 것은 major bump 때뿐이다. \
         (0 이면 파일이 비었거나 파싱이 깨진 것이고, 그 상태로는 아래 검사들이 \
         검사할 것 없이 통과한다)",
        methods.len()
    );
    methods
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
