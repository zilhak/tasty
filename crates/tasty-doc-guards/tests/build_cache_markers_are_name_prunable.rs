//! 빌드 캐시 표식이 **이름 가지치기 밖에** 있으면, 표식 판정을 부르지 않는 순회
//! 가드가 그 디렉토리를 통째로 모수에 넣는다.
//!
//! 레포를 순회하는 가드는 대부분 디렉토리 **이름**으로만 가지친다. 이름은 성질이
//! 아니라서 `CARGO_TARGET_DIR` 로 다른 이름을 준 빌드 디렉토리는 목록에 안 걸린다.
//! 성질로 판정하는 [`tasty_doc_guards::is_build_cache_dir`] 는 이미 있지만 그것을
//! 부르는 자리는 **그 사고를 실제로 겪은 몇 곳뿐**이다(주석에 남은 실측: 1.30s →
//! 86.30s · 0.05s → 89.17s · 형식 판정 8251 건 오탐). 나머지는 이력이 없을 뿐
//! 성질이 다르지 않다.
//!
//! **그래서 이 가드는 그 자리들을 고치는 대신 그것들이 오늘 안전한 조건을 단정한다:**
//! 레포 안의 모든 빌드 캐시 표식은 이름만 보는 가드도 자르는 이름 아래에 있다.
//! 조건이 깨지는 날 여기가 빨개지고, 실패 메시지가 무엇이 위험해졌는지 말한다.
//!
//! 조건부 안전을 단정으로 두는 것이 전수 개조보다 정직하다 — 개조는 "고쳤다" 로
//! 끝나지만 이 단정은 "나머지는 조건부로 안전하다" 를 계속 말한다.

// 이유: 이 타깃은 전부 테스트다. 테스트의 `let _` 무시는 정책이 사유를 요구하지
// 않으므로 `clippy::let_underscore_must_use` 명부(프로덕션 전용)에 섞이면 안 된다
// — docs/dev-guide/error-handling.md.
#![allow(clippy::let_underscore_must_use)]

use std::path::Path;
use tasty_doc_guards::floored_walk::{Descend, Floor, Walked, normalized_rel, walk_with_floor};

/// 순회 가드들의 이름 제외 목록 **교집합**(2026-09-06 실측, 8 곳).
///
/// 넓은 집합을 쓰면 가장 좁게 자르는 자리에서 거짓 안심이 된다 — 그 자리가
/// 안전해야 전부 안전하므로 교집합이 유일하게 옳은 모수다. 어떤 순회 가드가
/// 이보다 좁게 자르기 시작하면 이 단정은 그만큼 약해진다.
///
/// **이 값은 "이름 축에만 의존하는 가드 집합" 의 함수다.** 그래서 반대 방향으로도
/// 낡는다 — 어떤 가드가 성질 판정([`tasty_doc_guards::is_build_cache_dir`])을 부르기
/// 시작하면 그 가드에 대해서는 이름 축 요구가 **불필요해진다.** 실제로 그렇게 됐다:
/// 이 상수를 잡을 때 성질 판정을 부르는 자리가 셋이었고 2026-09-06 에 일곱이 됐다.
/// 그 값을 움직인 것이 이 가드를 쓴 사람 자신이었다. 그래서 아래 [`KNOWN_OUTSIDE`] 는
/// 이름이 아니라 **왜 안전한가**를 담는다.
const COMMON_PRUNED: &[&str] = &["target", ".git"];

/// 이름 제외 밖에 있는 것이 **알려진** 빌드 캐시 디렉토리와 그것이 안전한 근거.
/// `(경로 성분, 왜 밖에 있는가 · 무엇이 그것을 안전하게 하는가)`.
///
/// **이 명부에는 역방향 검사를 걸지 않는다.** 항목의 실재는 기계마다 다르다 — 문서화된
/// 절차를 돌린 기계에만 생기고, CI 는 그 절차를 돌지 않는다. "등재됐는데 없다" 로
/// 빨개지면 그 빨강이 커밋이 아니라 기계 상태에 귀속되어 아무것도 지키지 못한다.
/// 대신 항목마다 **안전한 이유**를 요구한다 — 이유 없이 이름만 늘리면 그때부터 이
/// 명부가 도망길이 된다.
const KNOWN_OUTSIDE: &[(&str, &str)] = &[(
    "target-e2e-headless",
    "문서화된 e2e 탈출구의 `CARGO_TARGET_DIR`(docs/dev-guide/e2e-tests.md). \
     `target/` 아래로 옮길 수 없다 — `tasty-host-plugin` 의 `ensure_dev_bundle` 이 \
     실행 파일 디렉토리의 **조부모**를 워크스페이스로 역산해서 깊이가 정확히 2 여야 \
     하고, 어기면 `sync_builtin_dev` 가 전부 false 를 반환해 번들 plugin 이 하나도 \
     스테이징되지 않는다(조용히 낡은 plugin 으로 돈다). \
     안전한 이유: cargo 가 만든 디렉토리라 `CACHEDIR.TAG` 를 갖는다(2026-09-06 실측, \
     서명 일치). 성질 판정을 부르는 순회 가드는 이름과 무관하게 이것을 가지친다.",
)];

/// 순회가 살아 있는지 보는 연기 검사의 하한. 표식 **수**에는 하한을 걸 수 없다 —
/// 0 개는 고장이 아니라 정상 상태다(빌드 전이거나 `CARGO_TARGET_DIR` 가 레포 밖).
/// 반면 디렉토리를 하나도 못 세면 그것은 순회가 죽었다는 뜻이다.
/// 값의 근거: 2026-09-06 실측 367(표식 아래와 심볼릭 링크는 안 센다).
const MIN_DIRS_WALKED: usize = 200;

/// 통합 타깃 순회의 하한.
const TARGET_FLOOR: Floor = Floor {
    min: 60,
    measured: 105,
    measured_on: "2026-09-06",
    why_this_gap: "이 모수는 통합 테스트 타깃의 수다. 가드가 하나 늘 때마다 하나씩 느는 \
                   식이라 한 번에 크게 안 움직이지만, 한 크레이트가 통째로 갈리면 그 \
                   크레이트의 `tests/` 가 함께 옮겨 간다 — 그래서 여유를 넓게 둔다. 이 수는 \
                   판정에 안 쓰이고 실패문의 정보로만 쓰이므로, 하한의 목적은 그 정보가 \
                   순회 고장에서 나온 0 이 아님을 보장하는 것 하나다. 값의 계보: \
                   `ls tests/*.rs crates/*/tests/*.rs` 로 센 105(2026-09-06). 이 순회가 같은 \
                   것을 보는지는 수가 아니라 `the_target_walk_sees_both_roots_and_stays_flat` \
                   이 확인한다 — 수를 박으면 낡고, 뿌리와 깊이는 안 낡는다.",
};

/// `CACHEDIR.TAG` 규격의 서명 줄. 여기서는 **판정이 아니라 입력**으로만 쓴다 —
/// 판정은 언제나 [`tasty_doc_guards::is_build_cache_dir`] 가 한다.
const CACHEDIR_LINE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n";

/// 순회하면서 성질 판정을 **안 부르는** 통합 테스트 타깃 수.
///
/// 판정에 쓰지 않는다 — 실패문에 **정보로만** 싣는다. 이 수가 [`KNOWN_OUTSIDE`] 의
/// 안전 근거가 얼마나 두꺼운지를 말해 준다(0 이면 이름 축 요구 자체가 사라진다).
/// 판정에 쓰면 그 순간 "수를 고쳐서 통과시킨다" 는 가장 싼 수선이 생긴다.
///
/// `read_dir` 이 이 레포의 유일한 순회 수단이라는 것은 `file_walks_declare_their_mechanism`
/// 이 따로 고정한다 — 그래서 이 세기는 흉내가 아니라 그 사실 위에 선다.
fn scanners_without_property_check(root: &Path) -> Option<usize> {
    let Ok(targets) = walk_with_floor(
        root,
        root,
        &TARGET_FLOOR,
        Descend::SkipBuildCaches,
        &is_integration_target,
    ) else {
        // 이 수는 실패문의 정보일 뿐이라 순회가 죽어도 판정을 막지 않는다. 다만 0 을
        // 정보로 싣지는 않는다 — 0 은 "이름 축 요구가 사라졌다" 는 뜻으로 읽히는데
        // 순회가 죽어서 나온 0 은 그 뜻이 아니다. `None` 이 그 둘을 코드로 가른다.
        return None;
    };
    Some(
        targets
            .iter()
            .filter(|t| {
                std::fs::read_to_string(&t.path).is_ok_and(|src| {
                    src.contains("read_dir") && !src.contains("is_build_cache_dir")
                })
            })
            .count(),
    )
}

/// 통합 테스트 타깃인가 — 루트 패키지와 각 크레이트의 `tests/` 바로 아래 `.rs`.
/// 경로로 가른다. `rel` 은 공용 순회가 정규화해서 주므로 플랫폼마다 안 갈린다.
fn is_integration_target(found: &Walked) -> bool {
    if !found.rel.ends_with(".rs") {
        return false;
    }
    let parts: Vec<&str> = found.rel.split('/').collect();
    match parts.as_slice() {
        ["tests", _] => true,
        ["crates", _, "tests", _] => true,
        _ => false,
    }
}

/// 경로 성분 중 하나라도 [`KNOWN_OUTSIDE`] 에 등재된 이름인가.
fn is_known_outside(rel: &str) -> bool {
    rel.split('/')
        .any(|c| KNOWN_OUTSIDE.iter().any(|(name, _)| *name == c))
}

/// 경로 성분 중 하나라도 공통 제외 이름이면, 이름만 보는 가드도 여기 도달하지 못한다.
fn is_covered_by_name_pruning(rel: &str) -> bool {
    rel.split('/').any(|c| COMMON_PRUNED.contains(&c))
}

/// 빌드 캐시 표식을 가진 디렉토리를 모은다. **이름으로 가지치기하지 않는다** — 이
/// 순회의 물음이 바로 "이름 밖에 표식이 있는가" 이기 때문이다.
///
/// 두 가지만 제한한다. 심볼릭 링크는 따라가지 않고(레포 밖 실제 경로로 순회가 샌다),
/// 표식을 찾으면 그 아래로 들어가지 않는다(캐시 내부는 이 물음의 답에 안 들어가고,
/// 들어가면 이 가드 자신이 그 사고의 규모를 재현한다).
fn find_markers(dir: &Path, root: &Path, out: &mut Vec<String>, walked: &mut usize) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_symlink() || !kind.is_dir() {
            continue;
        }
        let path = entry.path();
        *walked += 1;
        if tasty_doc_guards::is_build_cache_dir(&path) {
            out.push(normalized_rel(&path, root));
            continue;
        }
        find_markers(&path, root, out, walked);
    }
}

#[test]
fn every_build_cache_marker_sits_under_a_name_that_every_scanner_prunes() {
    let root = tasty_doc_guards::repo_root();
    let mut markers = Vec::new();
    let mut walked = 0usize;
    find_markers(&root, &root, &mut markers, &mut walked);
    markers.sort();

    assert!(
        walked >= MIN_DIRS_WALKED,
        "디렉토리를 {walked} 개만 순회했다(하한 {MIN_DIRS_WALKED}) — 순회가 죽었다면 \
         아래 판정은 빈 집합을 훑고 조용히 통과한다."
    );

    let outside: Vec<&String> = markers
        .iter()
        .filter(|rel| !is_covered_by_name_pruning(rel) && !is_known_outside(rel))
        .collect();

    let blind = match scanners_without_property_check(&root) {
        Some(n) => format!("{n} 개"),
        None => "셀 수 없었다 — 그 순회가 하한에 걸렸다".to_string(),
    };
    assert!(
        outside.is_empty(),
        "빌드 캐시 표식이 이름 제외({COMMON_PRUNED:?}) 밖에 있고 `KNOWN_OUTSIDE` 에도 \
         없다:\n{}\n\n\
         ★ 이 빨강은 커밋이 아니라 **이 기계의 파일시스템 상태**에 귀속된다. 그 디렉토리가 \
         실재할 때만 빨갛고, CI 는 그것을 만드는 절차를 돌지 않으므로 거기서는 이 판정이 \
         영원히 초록이다 — 초록이 아니라 **그 축에서 미측정**이다.\n\
         지금 순회하면서 성질 판정을 안 부르는 통합 테스트 타깃이 {blind}다. 그 자리에서 \
         이 디렉토리가 모수에 통째로 들어가면 순회 시간이 두 자릿수 배로 늘거나 그 안의 \
         산출물이 소스로 판정된다.\n\n\
         무엇을 할지 — 순서대로 확인하라:\n\
         1. `target/` 아래로 옮길 수 있는가. 옮길 수 없는 이유가 있으면 그것이 등재 사유다.\n\
         2. 그것을 훑는 가드가 실제로 있는가:\n     \
            grep -L is_build_cache_dir $(grep -rl 'fs::read_dir' tests/*.rs crates/*/tests/*.rs)\n     \
            각 파일의 순회 시작점을 본다 — 레포 루트부터 도는 것만 이 디렉토리를 만난다.\n\
         3. 옮길 수 없고 `CACHEDIR.TAG` 를 갖는다면 `KNOWN_OUTSIDE` 에 **사유와 함께** \
            등재하라. 사유에는 왜 못 옮기는지와 무엇이 그것을 안전하게 하는지를 적는다.\n\n\
         ★ `PRUNE_DIRS` 에 이름을 더해서 끄지 마라 — 여덟 곳의 교집합을 여덟 번 고치는 \
         일이라 사본이 여덟 벌 생긴다. 근본은 그 가드들이 \
         `tasty_doc_guards::is_build_cache_dir` 를 부르게 하는 쪽이다(성질로 판정하면 \
         이름이 무엇이든 걸린다).",
        outside
            .iter()
            .map(|r| format!("  {r}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

#[test]
fn the_scan_separates_a_marker_outside_the_pruned_names_from_one_inside() {
    let base = std::env::temp_dir().join(format!(
        "tasty-cachetag-probe-{}-{}",
        std::process::id(),
        line!()
    ));
    // 이유: 앞선 실행이 남긴 잔여를 치우는 것이라 없을 때가 정상이다 — 실패가
    // 정보를 주지 않는다. 심은 것이 실제로 있는지는 아래에서 따로 단정한다.
    let _ = std::fs::remove_dir_all(&base);

    let outside = base.join("tools").join("cache");
    let inside = base.join("target").join("nested");
    // 셋째 팔: 이름 제외 밖이지만 `KNOWN_OUTSIDE` 에 등재된 것. 이것까지 걸리면
    // 명부가 판정에 안 붙어 있다는 뜻이고, 안 걸리면 명부가 실제로 작동한다.
    let excused = base.join(KNOWN_OUTSIDE[0].0).join("debug");
    for dir in [&outside, &inside, &excused] {
        std::fs::create_dir_all(dir).expect("프로브 디렉토리를 만들지 못했다");
        std::fs::write(dir.join("CACHEDIR.TAG"), CACHEDIR_LINE).expect("표식을 쓰지 못했다");
    }

    // 심은 것이 실제로 있는지 **먼저** 단정한다. 변이가 불발했는데 결과만 읽으면
    // 두 팔이 모두 조용해지고, 그 초록은 양방향을 다 틀리게 읽힌다.
    for dir in [&outside, &inside, &excused] {
        assert!(
            tasty_doc_guards::is_build_cache_dir(dir),
            "프로브가 적용되지 않았다: {}",
            dir.display()
        );
    }

    let mut found = Vec::new();
    let mut walked = 0usize;
    find_markers(&base, &base, &mut found, &mut walked);
    found.sort();

    let mut expected = vec![
        format!("{}/debug", KNOWN_OUTSIDE[0].0),
        "target/nested".to_string(),
        "tools/cache".to_string(),
    ];
    expected.sort();
    assert_eq!(
        found, expected,
        "순회가 심은 표식 셋을 다 찾아야 한다 — 못 찾으면 위 단정의 초록은 무정보다"
    );

    let reported: Vec<&String> = found
        .iter()
        .filter(|rel| !is_covered_by_name_pruning(rel) && !is_known_outside(rel))
        .collect();
    assert_eq!(
        reported,
        vec![&"tools/cache".to_string()],
        "이름 제외 밖이면서 등재되지도 않은 것만 위반으로 세야 한다 — 이름 아래의 것과 \
         등재된 것은 둘 다 빠져야 하고, 그 둘이 빠지는 이유는 서로 다르다"
    );

    // 이유: 정리는 판정 뒤라 여기서 실패해도 이 테스트의 결론이 안 바뀐다.
    std::fs::remove_dir_all(&base).ok();
}

/// [`KNOWN_OUTSIDE`] 의 **사유**가 실재하는지 본다.
///
/// 사유는 판정에 안 쓰인다 — 지워도 위 두 테스트의 값이 하나도 안 움직인다. 그래서
/// 이 검사가 없으면 명부가 "이름만 적으면 통과" 로 조용히 바뀐다. 값을 움직이는 변이는
/// 위에서 잡히고, **값을 안 움직이는 변이를 잡는 것이 이 검사의 전부다.**
#[test]
fn every_excused_directory_carries_a_reason_not_just_a_name() {
    for (name, why) in KNOWN_OUTSIDE {
        let words = why.split_whitespace().count();
        assert!(
            words >= 20,
            "`KNOWN_OUTSIDE` 의 `{name}` 사유가 {words} 낱말뿐이다. 이 명부는 이름이 \
             아니라 근거를 담는다 — 두 물음에 답해야 한다: (1) 왜 이름 제외 아래로 \
             옮길 수 없는가 (2) 그런데도 왜 안전한가.\n\
             ★ 이 문턱을 내려서 통과시키지 마라. 사유가 짧아도 되는 항목이 아니라 \
             사유를 못 대는 항목이라면, 그것은 등재 대상이 아니라 옮길 대상이다."
        );
    }
}

/// 통합 타깃 순회가 **두 뿌리를 다 보는지** 확인한다.
///
/// 수로 확인하지 않는 이유가 있다. 수는 가드가 하나 늘 때마다 낡고, 낡은 수를 고치는
/// 손이 그 자리에서 판정을 함께 무디게 한다. 반면 "루트 패키지와 크레이트 양쪽을
/// 보는가" 는 구조라 안 낡는다. 한쪽 뿌리만 잡혀도 수는 그럴듯하게 나오므로, 이
/// 확인이 없으면 절반 죽은 순회가 조용히 통과한다.
///
/// **"한 겹에 머무는가" 는 여기서 못 묻는다.** 그 물음의 답을 정하는 것은 순회가 아니라
/// [`is_integration_target`] 이고, 순회 결과를 다시 재면 그 술어가 통과시킨 것만 보게 되어
/// 단정이 언제나 참이 된다 — 실제로 그렇게 썼다가 변이로 걸렸다(술어의 깊이 조건을
/// 풀어도 그 단정은 안 죽었다). 그래서 그 물음은 아래 술어 대조로 옮겼다.
#[test]
fn the_target_walk_sees_both_roots() {
    let root = tasty_doc_guards::repo_root();
    let targets = walk_with_floor(
        &root,
        &root,
        &TARGET_FLOOR,
        Descend::SkipBuildCaches,
        &is_integration_target,
    )
    .unwrap_or_else(|why| panic!("{why}"));

    assert!(
        targets.iter().any(|t| t.rel.starts_with("tests/")),
        "루트 패키지의 `tests/` 를 하나도 못 봤다 — 순회가 절반 죽었다"
    );
    assert!(
        targets.iter().any(|t| t.rel.starts_with("crates/")),
        "크레이트의 `tests/` 를 하나도 못 봤다 — 순회가 절반 죽었다"
    );
}

/// 통합 타깃 술어가 **한 겹만** 집는지. cargo 는 `tests/` 바로 아래 `.rs` 만 타깃으로
/// 만들고 그 아래 디렉토리는 공유 헬퍼다 — 물음이 다르므로 세면 안 된다.
///
/// 술어를 직접 부른다. 순회 결과로 물으면 술어가 통과시킨 것만 보게 되어 답이 언제나
/// 참이고, 레포에 그런 자리가 있는지에도 답이 흔들린다(지금 `tests/` 아래에는 하위
/// 디렉토리가 일곱 있고 그 안에 `.rs` 가 여섯 있다 — 2026-09-06 실측).
#[test]
fn the_target_predicate_takes_only_the_flat_layer() {
    let probe = |rel: &str| Walked {
        path: std::path::PathBuf::from(rel),
        rel: rel.to_string(),
    };
    assert!(is_integration_target(&probe("tests/layering.rs")));
    assert!(is_integration_target(&probe(
        "crates/tasty-doc-guards/tests/x.rs"
    )));
    assert!(
        !is_integration_target(&probe("tests/common/mod.rs")),
        "`tests/` 하위 디렉토리는 공유 헬퍼지 타깃이 아니다"
    );
    assert!(
        !is_integration_target(&probe("crates/a/tests/sub/x.rs")),
        "크레이트 쪽도 마찬가지다"
    );
    assert!(!is_integration_target(&probe("src/main.rs")));
    assert!(
        !is_integration_target(&probe("tests/fixtures/a.md")),
        "`.rs` 가 아닌 것을 타깃으로 센다"
    );
}
