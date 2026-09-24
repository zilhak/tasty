//! 현재 소스가 바이너리에 포함된 소스와 같은지 확인한다.
//! mtime은 내용이 같아도 Git 작업으로 달라질 수 있어 라이브러리 지문과 소스 내용을 비교한다.
//! 각 바이너리 소스는 별도로 비교하므로 다른 바이너리 추가가 공통 지문을 바꾸지 않는다.

use std::path::Path;

// 빌드 스크립트와 같은 지문 계산 소스를 include한다.
use crate::fingerprint_rule::fingerprint;

/// 빌드에 포함한 라이브러리 소스 지문.
pub const LIB_FINGERPRINT: &str = env!("TASTY_DOC_GUARDS_LIB_FINGERPRINT");

/// 대조 결과.
#[derive(Debug, PartialEq, Eq)]
pub enum Freshness {
    /// 지어진 내용과 디스크의 내용이 같다.
    Fresh,
    /// 다르다 — 다시 지어야 한다. 무엇이 다른지 사람이 읽을 수 있게 담는다.
    Stale(String),
    /// 배포본·합성 트리 등에 라이브러리 소스가 없어 비교할 수 없다.
    Undecidable,
}

/// 라이브러리 지문과 바이너리의 소스를 비교한다.
/// own_rel은 저장소 상대 경로, own_text는 include_str!로 빌드에 포함한 원문이다.
pub fn check(root: &Path, own_rel: &str, own_text: &str) -> Freshness {
    let lib_dir = root.join("crates/tasty-doc-guards/src");
    if !lib_dir.is_dir() {
        return Freshness::Undecidable;
    }
    let Some(on_disk) = fingerprint(&lib_dir) else {
        return Freshness::Undecidable;
    };
    if on_disk != LIB_FINGERPRINT {
        return Freshness::Stale(format!(
            "라이브러리 소스가 빌드된 내용과 다르다 (빌드 지문 {LIB_FINGERPRINT}, 현재 지문 {on_disk})"
        ));
    }
    let own_path = root.join(own_rel);
    let Ok(disk_text) = std::fs::read_to_string(&own_path) else {
        // 라이브러리는 있지만 해당 바이너리 소스만 없으면 불일치다.
        return Freshness::Stale(format!("바이너리 소스를 읽을 수 없다: {own_rel}"));
    };
    if disk_text.replace("\r\n", "\n") != own_text.replace("\r\n", "\n") {
        return Freshness::Stale(format!("바이너리 소스가 빌드된 내용과 다르다: {own_rel}"));
    }
    Freshness::Fresh
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 빌드 지문을 현재 소스와 비교해 rerun-if-changed 누락을 찾는다.
    #[test]
    fn the_baked_fingerprint_matches_this_tree() {
        let root = crate::repo_root();
        let lib_dir = root.join("crates/tasty-doc-guards/src");
        assert!(
            lib_dir.is_dir(),
            "판정기 소스가 없다: {}",
            lib_dir.display()
        );
        assert_eq!(
            fingerprint(&lib_dir).expect("지문"),
            LIB_FINGERPRINT,
            "빌드 지문이 현재 소스와 다르다. build.rs의 rerun-if-changed를 확인한다. left는 현재 계산값, right는 빌드에 포함된 값이다."
        );
    }

    #[test]
    fn a_tree_without_the_judge_sources_is_undecidable() {
        let empty = std::env::temp_dir().join(format!("tasty-fresh-{}", std::process::id()));
        std::fs::create_dir_all(&empty).expect("임시 디렉토리");
        assert_eq!(check(&empty, "x.rs", "y"), Freshness::Undecidable);
        // 뒷정리다 — 여기서 실패해도 판정은 이미 끝났다.
        let _ = std::fs::remove_dir_all(&empty);
    }

    #[test]
    fn a_changed_own_source_is_stale_even_when_the_library_matches() {
        let root = crate::repo_root();
        let got = check(
            &root,
            "crates/tasty-doc-guards/src/lib.rs",
            "이 내용은 디스크와 다르다",
        );
        assert!(
            matches!(got, Freshness::Stale(_)),
            "자기 소스 차이를 못 봤다: {got:?}"
        );
    }
}
