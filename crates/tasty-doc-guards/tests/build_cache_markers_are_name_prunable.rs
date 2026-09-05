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

use std::path::Path;

/// 순회 가드들의 이름 제외 목록 **교집합**(2026-09-06 실측, 8 곳).
///
/// 넓은 집합을 쓰면 가장 좁게 자르는 자리에서 거짓 안심이 된다 — 그 자리가
/// 안전해야 전부 안전하므로 교집합이 유일하게 옳은 모수다. 어떤 순회 가드가
/// 이보다 좁게 자르기 시작하면 이 단정은 그만큼 약해진다.
const COMMON_PRUNED: &[&str] = &["target", ".git"];

/// 순회가 살아 있는지 보는 연기 검사의 하한. 표식 **수**에는 하한을 걸 수 없다 —
/// 0 개는 고장이 아니라 정상 상태다(빌드 전이거나 `CARGO_TARGET_DIR` 가 레포 밖).
/// 반면 디렉토리를 하나도 못 세면 그것은 순회가 죽었다는 뜻이다.
/// 값의 근거: 2026-09-06 실측 367(표식 아래와 심볼릭 링크는 안 센다).
const MIN_DIRS_WALKED: usize = 200;

/// `CACHEDIR.TAG` 규격의 서명 줄. 여기서는 **판정이 아니라 입력**으로만 쓴다 —
/// 판정은 언제나 [`tasty_doc_guards::is_build_cache_dir`] 가 한다.
const CACHEDIR_LINE: &[u8] = b"Signature: 8a477f597d28d172789f06886806bc55\n";

fn rel_of(path: &Path, root: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
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
            out.push(rel_of(&path, root));
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
        .filter(|rel| !is_covered_by_name_pruning(rel))
        .collect();

    assert!(
        outside.is_empty(),
        "빌드 캐시 표식이 이름 제외({COMMON_PRUNED:?}) 밖에 있다:\n{}\n\n\
         레포를 순회하는 가드 대부분은 이름으로만 가지친다. 이 디렉토리는 그 자리들의 \
         모수에 통째로 들어간다 — 순회 시간이 두 자릿수 배로 늘거나, 그 안의 산출물이 \
         소스로 판정된다. 고치는 방법은 둘이다: 이 디렉토리를 공통 제외 이름 아래로 \
         옮기거나, 그것을 훑는 가드들이 `tasty_doc_guards::is_build_cache_dir` 를 \
         부르게 하거나. 뒤쪽이 근본이고, 이미 그렇게 고친 자리가 몇 곳 있다.",
        outside
            .iter()
            .map(|r| format!("  {r}"))
            .collect::<Vec<_>>()
            .join("\n")
    );
}

/// 위 단정이 **무엇이든 잡을 수 있는지** 를 같은 순회·같은 판정으로 확인한다.
///
/// 한 방향만 재면 무정보다. "위반 0" 은 "안전하다" 와 "애초에 못 본다" 둘 다와
/// 양립한다 — 두 팔을 같은 범위 안에 심어야 그 둘이 갈린다.
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
    for dir in [&outside, &inside] {
        std::fs::create_dir_all(dir).expect("프로브 디렉토리를 만들지 못했다");
        std::fs::write(dir.join("CACHEDIR.TAG"), CACHEDIR_LINE).expect("표식을 쓰지 못했다");
    }

    // 심은 것이 실제로 있는지 **먼저** 단정한다. 변이가 불발했는데 결과만 읽으면
    // 두 팔이 모두 조용해지고, 그 초록은 양방향을 다 틀리게 읽힌다.
    for dir in [&outside, &inside] {
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

    assert_eq!(
        found,
        vec!["target/nested".to_string(), "tools/cache".to_string()],
        "순회가 심은 표식 둘을 다 찾아야 한다 — 못 찾으면 위 단정의 초록은 무정보다"
    );

    let reported: Vec<&String> = found
        .iter()
        .filter(|rel| !is_covered_by_name_pruning(rel))
        .collect();
    assert_eq!(
        reported,
        vec![&"tools/cache".to_string()],
        "이름 제외 밖의 것만 위반으로 세야 한다"
    );

    // 이유: 정리는 판정 뒤라 여기서 실패해도 이 테스트의 결론이 안 바뀐다.
    std::fs::remove_dir_all(&base).ok();
}
