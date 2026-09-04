use super::*;

/// 크레이트 하나를 스캔 결과에서 통째로 지운다. **개수 하한은 여전히 통과**하는
/// 크기라, 이 변이가 죽는 것은 오직 집합 대조 때문이다.
#[test]
fn a_crate_dropped_from_the_scan_is_reported_missing() {
    let files = rust_sources();
    let counts = unit_counts(&files);
    let victim = counts
        .iter()
        .filter(|(unit, _)| unit.starts_with("crates/"))
        .max_by_key(|(_, n)| **n)
        .map(|(unit, _)| unit.clone())
        .expect("크레이트 단위가 하나도 없다");
    let mutated: Vec<(PathBuf, String)> = files
        .into_iter()
        .filter(|(rel, _)| unit_of(rel).as_deref() != Some(victim.as_str()))
        .collect();
    assert!(
        mutated.len() >= MIN_SCANNED_FILES,
        "변이가 개수 하한까지 건드리면 무엇이 이 변이를 죽였는지 갈리지 않는다 — 남은 {}",
        mutated.len()
    );
    let (missing, extra) = unit_diff(&scanned_units(&mutated), &expected_units());
    assert_eq!(missing, vec![victim], "빠진 단위를 지목하지 못했다");
    assert!(extra.is_empty(), "여분이 생기면 안 된다: {extra:?}");
}

/// 반대 방향 — 스캔에만 있고 매니페스트 쪽에 없는 단위도 잡아야 한다.
#[test]
fn a_unit_absent_from_the_manifest_side_is_reported_extra() {
    let ghost = "crates/definitely-not-a-crate".to_owned();
    let mut scanned = scanned_units(&rust_sources());
    scanned.insert(ghost.clone());
    let (missing, extra) = unit_diff(&scanned, &expected_units());
    assert!(missing.is_empty(), "빠진 단위가 없어야 한다: {missing:?}");
    assert_eq!(extra, vec![ghost], "여분 단위를 지목하지 못했다");
}

/// 정당한 형태는 그대로 통과해야 한다 — 판정기가 무조건 빨간 것이 아님을 못박는다.
#[test]
fn the_unmutated_scan_passes() {
    let (missing, extra) = unit_diff(&scanned_units(&rust_sources()), &expected_units());
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{missing:?} / {extra:?}"
    );
}
