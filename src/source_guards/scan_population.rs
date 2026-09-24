//! 공용 소스 스캔의 파일 목록을 Git이 알려주는 목록과 비교한다.
//! 파일 수 하한만으로는 일부 크레이트·파일의 누락을 찾지 못해 경로 집합도 대조한다.
//! 추적 파일과 무시되지 않은 미추적 파일을 함께 읽고 디스크에 없는 파일은 제외한다.
//! Git 실행이 실패하면 비교할 수 없으므로 검사를 중단한다(ADR-0048).

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::process::Command;

use super::{MIN_SCANNED_FILES, SCAN_ROOTS, repo_root, rust_sources};

fn git_listed_sources() -> BTreeSet<PathBuf> {
    let root = repo_root();
    let output = Command::new("git")
        .arg("-C")
        .arg(&root)
        .args(["ls-files", "-co", "--exclude-standard", "--"])
        .args(SCAN_ROOTS)
        .output()
        .unwrap_or_else(|e| {
            panic!("git ls-files 실행 실패: {e}. 소스 수집과 비교할 Git 목록이 필요하다.")
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
        // Git의 / 구분 경로를 성분별 PathBuf로 바꿔 플랫폼별 경로와 비교한다.
        .map(|line| line.split('/').collect::<PathBuf>())
        .filter(|rel| root.join(rel).is_file())
        .collect();

    assert!(
        listed.len() >= MIN_SCANNED_FILES,
        "Git의 Rust 파일 목록이 {}개로 하한 {MIN_SCANNED_FILES} 미만이다. 비교 전에 Git 목록과 범위를 확인한다.",
        listed.len()
    );
    listed
}

fn drift(scanned: &BTreeSet<PathBuf>, listed: &BTreeSet<PathBuf>) -> (Vec<PathBuf>, Vec<PathBuf>) {
    (
        listed.difference(scanned).cloned().collect(),
        scanned.difference(listed).cloned().collect(),
    )
}

fn scanned_set() -> BTreeSet<PathBuf> {
    rust_sources().into_iter().map(|(path, _)| path).collect()
}

/// 누락과 추가를 양방향으로 비교한다. 파일 하나씩의 삭제·추가가 상쇄돼도 찾을 수 있다.
#[test]
fn the_scan_population_matches_what_git_lists() {
    let scanned = scanned_set();
    let listed = git_listed_sources();
    let (missing, extra) = drift(&scanned, &listed);

    assert!(
        missing.is_empty() && extra.is_empty(),
        "소스 수집 {}개와 Git 목록 {}개가 다르다.\n  수집 누락 {}개: {missing:?}\n  수집에만 있는 파일 {}개: {extra:?}\n누락은 순회 제외를, 추가는 빌드 캐시·무시 파일이 포함됐는지 확인한다.",
        scanned.len(),
        listed.len(),
        missing.len(),
        extra.len(),
    );
}

#[cfg(test)]
mod exemption_mutations {
    use super::*;

    #[test]
    fn the_unmutated_population_has_no_drift() {
        let scanned = scanned_set();
        let listed = git_listed_sources();
        let (missing, extra) = drift(&scanned, &listed);
        assert!(missing.is_empty() && extra.is_empty());
        assert!(
            scanned.len() > MIN_SCANNED_FILES && listed.len() > MIN_SCANNED_FILES,
            "빈 목록끼리는 차이를 검증할 수 없다. 소스 수집 {}개 · Git {}개",
            scanned.len(),
            listed.len()
        );
    }

    /// 크레이트가 작아져 누락 입력이 약해지지 않도록 현재 가장 큰 크레이트를 고른다.
    fn largest_unit(listed: &BTreeSet<PathBuf>) -> PathBuf {
        let mut counts: std::collections::BTreeMap<PathBuf, usize> =
            std::collections::BTreeMap::new();
        for path in listed {
            let unit: PathBuf = match path.components().next().map(|c| c.as_os_str()) {
                Some(first) if first == "crates" => path.components().take(2).collect(),
                _ => path.components().take(1).collect(),
            };
            *counts.entry(unit).or_default() += 1;
        }
        // src 전체 누락은 하한도 검출하므로 하한을 통과하는 일부 누락을 검증할 크레이트를 고른다.
        counts
            .into_iter()
            .filter(|(unit, _)| unit.starts_with("crates"))
            .max_by_key(|(_, n)| *n)
            .map(|(unit, _)| unit)
            .expect("크레이트 단위가 하나도 없다 — 대조군이 비었다")
    }

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
            "가장 큰 크레이트 {}개를 빼면 {}개가 남아 하한 {MIN_SCANNED_FILES} 미만이다. 이 입력으로는 하한이 놓치는 일부 누락을 검증할 수 없다.",
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
