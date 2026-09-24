use super::*;

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
        "크레이트를 뺀 뒤 {}개만 남아 파일 수 하한도 실패한다. 집합 비교만의 검출력을 확인할 입력이 필요하다.",
        mutated.len()
    );
    let (missing, extra) = unit_diff(&scanned_units(&mutated), &expected_units());
    assert_eq!(missing, vec![victim], "빠진 단위를 지목하지 못했다");
    assert!(extra.is_empty(), "여분이 생기면 안 된다: {extra:?}");
}

#[test]
fn a_unit_absent_from_the_manifest_side_is_reported_extra() {
    let ghost = "crates/definitely-not-a-crate".to_owned();
    let mut scanned = scanned_units(&rust_sources());
    scanned.insert(ghost.clone());
    let (missing, extra) = unit_diff(&scanned, &expected_units());
    assert!(missing.is_empty(), "빠진 단위가 없어야 한다: {missing:?}");
    assert_eq!(extra, vec![ghost], "여분 단위를 지목하지 못했다");
}

#[test]
fn the_unmutated_scan_passes() {
    let (missing, extra) = unit_diff(&scanned_units(&rust_sources()), &expected_units());
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{missing:?} / {extra:?}"
    );
}
