//! ADR이 없는 상태를 명시한 인덱스와 실제 파일 집합을 대조한다.
//! 개수 하한 대신 디렉터리를 독립적으로 읽어 수집 누락을 검사한다.

use std::collections::BTreeSet;
use std::path::Path;

use crate::adr_index::{ADR_DIR, Adr, group_slugs};

pub const EMPTY_MARKER: &str = "<!-- adr-empty: intentional -->";

/// 빈 집합은 명시 표식이 있을 때만 허용한다. 표식과 파일이 공존해도 오류다.
pub fn violations(root: &Path, adrs: &[Adr], index: &str) -> Result<Vec<String>, String> {
    let mut files = BTreeSet::new();
    for entry in std::fs::read_dir(root.join(ADR_DIR)).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.ends_with(".md") && name.as_bytes().first().is_some_and(u8::is_ascii_digit) {
            if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                return Err(format!("ADR 항목이 일반 파일이 아니다: {name}"));
            }
            files.insert(name);
        }
    }
    let collected: BTreeSet<_> = adrs.iter().map(|a| a.file.clone()).collect();
    let mut errors = Vec::new();
    if files != collected || adrs.len() != collected.len() {
        errors.push(format!(
            "ADR 디렉터리와 수집 결과가 다르다: files={files:?}, collected={collected:?}"
        ));
    }
    let markers = index
        .lines()
        .filter(|line| line.trim() == EMPTY_MARKER)
        .count();
    if files.is_empty() {
        if markers != 1 {
            errors.push("빈 ADR 집합은 인덱스에 명시 표식 하나가 필요하다".into());
        }
        if !group_slugs(index).is_empty() {
            errors.push("빈 ADR 집합에 생성 그룹이 남아 있다".into());
        }
    } else if markers != 0 {
        errors.push("ADR이 존재하므로 빈 집합 표식을 제거해야 한다".into());
    }
    Ok(errors)
}
