//! 스캔 **모수**를 집합 동등으로 고정한다 — "몇 개 봤나" 가 아니라 "무엇을 봤나".
//!
//! [ADR-0133](../../docs/adr/0133-guard-scan-population-is-pinned-not-enumerated.md)
//! 의 Decision 3 이 요구하는 형태다. 그 ADR 의 표현대로 어느 형태를 어느 용도로 썼는지
//! 적어 둔다:
//!
//! - `MIN_SCANNED_FILES`(하한 900) — **연기 검사**다. 경로가 틀리면 예외가 아니라 조용한
//!   0 이 되고 0 인 모수는 언제나 초록이라, `rust_sources` 안에서 즉시 죽는 쪽이 낫다.
//!   모수 고정에는 쓰지 않는다.
//! - 이 파일 — **모수 고정**이다. 실측 1100 개 남짓 대 하한 900 이라 **200 개 넘게 조용히
//!   빠져도** 하한은 안 움직인다 — 가장 큰 스캔 단위를 통째로 빼도 안 걸리는 폭이다.
//!   그리고 빠진 파일에 위반이 0 건이면 offender 목록도 안 움직여 신호가 아예 없다.
//!   집합 동등만이 빠진 이름을 이름으로 말한다. 그 폭은 `a_whole_unit_can_vanish_under_the_floor`
//!   가 실측으로 못박는다 — 서술이 아니라 단정이다.
//!
//! ## 왜 스냅샷 상수가 아니라 git 인가
//!
//! ADR 은 "스냅샷과 `BTreeSet` 비교" 라고 적었지만, 여기 모수는 1120 개다. 상수로 박으면
//! `.rs` 를 더할 때마다 사람이 갱신해야 하고 — **그 갱신을 잊는 것이 바로 이 ADR 이 막으려는
//! 실패**다. 대신 `git ls-files` 를 모수로 쓴다. git 의 목록은 스캐너의 순회 로직과 완전히
//! 다른 시스템이 만든 것이라 **같은 버그를 공유하지 않고**, 파일이 늘거나 줄면 저절로 따라온다.
//! 갱신 절차가 필요 없는 것이 이 선택이 사는 것이다.
//!
//! 추적본(`-c`)과 무시되지 않은 미추적본(`-o --exclude-standard`)을 함께 쓴다 — 새로 만든
//! `.rs` 는 `git add` 전에도 양쪽 집합에 동시에 들어와야 이 가드가 작업 중에 헛되이 빨개지지
//! 않는다. 인덱스에만 있고 디스크에 없는 것(삭제 중)은 실재로 걸러낸다.
//!
//! ## git 이 없으면 크게 죽는다
//!
//! 조용히 넘기지 않는다. "git 이 없어서 판정 안 함" 은 이 가드가 막으려는 **0 회 실행**
//! 그 자체이고, 0 회 실행은 0 건 발견과 구별되지 않는다.

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use super::{MIN_SCANNED_FILES, SCAN_ROOTS, repo_root, rust_sources};

/// git 이 아는 `.rs` 목록 — 스캔 루트 아래, 디스크에 실재하는 것만.
fn git_listed_sources() -> BTreeSet<PathBuf> {
    let root = repo_root();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-co", "--exclude-standard", "--"])
        .args(SCAN_ROOTS)
        .output()
        .unwrap_or_else(|e| {
            panic!(
                "`git ls-files` 를 실행할 수 없다 — {e}. 이 가드는 git 의 목록을 스캔 모수의 \
                 대조군으로 쓴다. 여기서 조용히 넘어가면 '0 회 실행' 이 '0 건 발견' 으로 \
                 보이므로 죽는다"
            )
        });
    assert!(
        output.status.success(),
        "`git ls-files` 가 실패했다(rc {:?}): {}",
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).trim()
    );

    let listed: BTreeSet<PathBuf> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter(|line| line.ends_with(".rs"))
        // git 은 플랫폼과 무관하게 `/` 로 낸다. `PathBuf::from` 에 통째로 주면 Windows 에서
        // 구분자가 달라 `rust_sources` 의 `Path::join` 결과와 안 맞는다.
        .map(|line| line.split('/').collect::<PathBuf>())
        .filter(|rel| root.join(rel).is_file())
        .collect();

    assert!(
        listed.len() >= MIN_SCANNED_FILES,
        "git 이 낸 `.rs` 가 {} 개뿐이다(하한 {MIN_SCANNED_FILES}). 대조군이 비면 집합 동등은 \
         언제나 초록이므로, 대조군 자신에도 연기 검사를 둔다",
        listed.len()
    );
    listed
}

/// 스캔 집합과 대조군의 차이를 `(빠진 것, 여분)` 으로 낸다.
///
/// 순수 함수다 — 변이 테스트가 트리를 고치지 않고 이 판정기를 직접 찌를 수 있어야 한다.
fn drift(scanned: &BTreeSet<PathBuf>, listed: &BTreeSet<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    (
        listed.difference(scanned).cloned().collect(),
        scanned.difference(listed).cloned().collect(),
    )
}

fn scanned_set() -> BTreeSet<PathBuf> {
    rust_sources().into_iter().map(|(path, _)| path).collect()
}

/// 스캔 모수가 git 이 아는 목록과 **집합으로** 같은지 못박는다.
///
/// 양방향을 다 본다. 빠진 것은 가드의 사각이고, 여분은 무시 대상(빌드 산출물 등)이 모수에
/// 샌 것이라 반대쪽 신호다. 한 방향만 보면 같은 수로 상쇄되는 드리프트를 놓친다.
#[test]
fn the_scan_population_matches_what_git_lists() {
    let scanned = scanned_set();
    let listed = git_listed_sources();
    let (missing, extra) = drift(&scanned, &listed);

    assert!(
        missing.is_empty() && extra.is_empty(),
        "스캔 모수가 git 목록과 갈라졌다 — 스캔 {} 개 · git {} 개.\n\
         빠진 것({} 개): 이 파일들은 어떤 가드도 안 본다. 스캔 루프에 제외가 생겼는지 확인해라.\n  {missing:?}\n\
         여분({} 개): 무시 대상이 모수에 샜다. `target` 류 디렉토리 건너뛰기가 깨졌는지 확인해라.\n  {extra:?}",
        scanned.len(),
        listed.len(),
        missing.len(),
        extra.len(),
    );
}

/// 이 승급을 겨냥한 변이 — 집합 동등이 실제로 무는지, 그리고 하한·건수 고정으로는 못 잡는
/// 형태를 잡는지 확인한다. 판정기가 순수 함수라 트리를 안 고치고 찌른다.
#[cfg(test)]
mod exemption_mutations {
    use super::*;

    /// 무변이 대조. 이게 빨가면 아래 변이들의 빨강은 변이 때문이 아니다.
    #[test]
    fn the_unmutated_population_has_no_drift() {
        let scanned = scanned_set();
        let listed = git_listed_sources();
        let (missing, extra) = drift(&scanned, &listed);
        assert!(missing.is_empty() && extra.is_empty());
        // 0 을 보고하는 자리라 같은 산출물의 비영 대조를 같은 단정에 둔다.
        assert!(
            scanned.len() > MIN_SCANNED_FILES && listed.len() > MIN_SCANNED_FILES,
            "두 집합이 비어 있으면 차분 0 은 초록이 아니라 계측 실패다: \
             스캔 {} · git {}",
            scanned.len(),
            listed.len()
        );
    }

    /// 가장 큰 스캔 단위를 골라 온다 — 변이 대상을 이름으로 박으면 그 크레이트가 줄었을 때
    /// 변이가 조용히 약해진다(`tasty-ssh` 는 `.rs` 가 하나뿐이라 실제로 그랬다).
    fn largest_unit(listed: &BTreeSet<PathBuf>) -> PathBuf {
        let mut counts: std::collections::BTreeMap<PathBuf, usize> =
            std::collections::BTreeMap::new();
        for path in listed {
            // 스캔 단위는 `src` 하나와 `crates/<이름>` 각각이다.
            let unit: PathBuf = match path.components().next().map(|c| c.as_os_str()) {
                Some(first) if first == "crates" => path.components().take(2).collect(),
                _ => path.components().take(1).collect(),
            };
            *counts.entry(unit).or_default() += 1;
        }
        // `src` 는 늘 가장 크고 통째로 빠지면 하한이 잡는다 — 하한이 못 잡는 폭을 재는 것이
        // 목적이므로 크레이트 중에서 고른다.
        counts
            .into_iter()
            .filter(|(unit, _)| unit.starts_with("crates"))
            .max_by_key(|(_, n)| *n)
            .map(|(unit, _)| unit)
            .expect("크레이트 단위가 하나도 없다 — 대조군이 비었다")
    }

    /// **하한이 못 잡는 폭을 실측으로 박는다.** 가장 큰 크레이트를 통째로 빼도 남은 개수가
    /// 여전히 `MIN_SCANNED_FILES` 이상이라, 하한만 있는 가드는 이 변이에 초록이다.
    /// 집합 동등은 빠진 것을 이름으로 말한다 — 그 차이가 이 승급의 전부다.
    #[test]
    fn a_whole_unit_can_vanish_under_the_floor() {
        let listed = git_listed_sources();
        let victim = largest_unit(&listed);
        let dropped: BTreeSet<PathBuf> = listed
            .iter()
            .filter(|path| !path.starts_with(&victim))
            .cloned()
            .collect();

        let vanished = listed.len() - dropped.len();
        assert!(
            vanished > 1,
            "변이가 파일 하나만 지웠다 — `{}` 은 이 판정을 가르지 못한다",
            victim.display()
        );
        assert!(
            dropped.len() >= MIN_SCANNED_FILES,
            "전제가 깨졌다: 가장 큰 크레이트({} 개)를 빼면 {} 개가 남아 하한 {MIN_SCANNED_FILES}              아래로 내려간다. 그렇다면 하한이 이 변이를 잡는다는 뜻이고, 이 테스트가 재려던              '하한의 사각' 이 그만큼 좁아진 것이다 — doc 의 서술을 다시 재고 고쳐라",
            vanished,
            dropped.len()
        );

        let (missing, extra) = drift(&dropped, &listed);
        assert_eq!(
            missing.len(),
            vanished,
            "빠진 것을 전부 말하지 않는다: {missing:?}"
        );
        assert!(
            missing.iter().all(|path| path.starts_with(&victim)),
            "빠진 단위를 이름으로 말하지 않는다: {missing:?}"
        );
        assert!(extra.is_empty(), "여분이 생길 이유가 없다: {extra:?}");
    }

    /// 모수에 없는 것이 스캔에 끼면 반대쪽으로 말하는가.
    #[test]
    fn a_path_outside_the_git_list_is_reported_extra() {
        let listed = git_listed_sources();
        let intruder: PathBuf = ["target", "debug", "build", "generated.rs"]
            .iter()
            .collect();
        let mut polluted = listed.clone();
        polluted.insert(intruder.clone());

        let (missing, extra) = drift(&polluted, &listed);
        assert!(
            missing.is_empty(),
            "빠진 것이 생길 이유가 없다: {missing:?}"
        );
        assert_eq!(extra, vec![intruder]);
    }

    /// **건수 고정으로는 못 잡는 형태.** 하나를 빼고 하나를 넣으면 개수가 그대로다.
    /// 집합 동등만이 양쪽을 다 말한다 — 이 테스트가 곧 승급의 근거다.
    #[test]
    fn a_same_sized_swap_is_still_caught() {
        let listed = git_listed_sources();
        let removed = listed.iter().next().expect("대조군이 비었다").clone();
        let added: PathBuf = ["src", "this-file-does-not-exist.rs"].iter().collect();

        let mut swapped = listed.clone();
        swapped.remove(&removed);
        swapped.insert(added.clone());
        assert_eq!(
            swapped.len(),
            listed.len(),
            "변이가 개수를 바꿨다 — 이 테스트의 전제가 깨졌다"
        );

        let (missing, extra) = drift(&swapped, &listed);
        assert_eq!(missing, vec![removed]);
        assert_eq!(extra, vec![added]);
    }
}
