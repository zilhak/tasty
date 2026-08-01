//! `DetectorRuleKind::PathGlob` 패턴 전체를 하나의 `globset::GlobSet` 으로 묶어
//! 컴파일하고 캐시하는 헬퍼.
//!
//! 개별 `Glob::is_match()` 를 파일마다 N(rule 개수)회 호출하는 대신, 등록된
//! 패턴 전부를 `GlobSetBuilder` 로 하나의 자동에 합쳐 컴파일해두고 파일 하나당
//! `GlobSet::matches()` 1회로 매칭되는 패턴 인덱스 집합을 구한다. 컴파일은
//! registry 가 dirty 할 때(`ensure_finalized`)만 다시 하고, `identify` 호출
//! 경로는 이미 빌드된 결과를 재사용한다 — 매 파일마다 패턴을 재컴파일하지 않는다.

use std::collections::{HashMap, HashSet};

use globset::{Glob, GlobSet, GlobSetBuilder};

/// 컴파일된 PathGlob 매처 묶음. 패턴 문자열(`to_slash` 로 이미 정규화된 `/` 형태)
/// → `GlobSet` 내 인덱스. 동일 패턴 문자열은 여러 detector 가 공유해도 한 번만
/// 컴파일한다.
#[derive(Default)]
pub(in crate::file::format) struct PathGlobCache {
    set: Option<GlobSet>,
    index_of: HashMap<String, usize>,
}

impl PathGlobCache {
    /// 등록된 모든 PathGlob 패턴(중복 허용, 내부에서 dedupe)으로 재구성한다.
    ///
    /// 패턴은 `DetectorRuleKind::PathGlob` 생성 시점(`registry/helpers.rs::decl_rule_to_kind`)에
    /// 이미 `to_slash` 로 정규화돼 있고, 컴파일 가능 여부도 등록 시점
    /// (`config::validate_detector_decl`)에 이미 검증됐다 — 따라서 여기서의 컴파일
    /// 실패는 정상 경로에서 발생하지 않아야 한다. 그래도 방어적으로, 실패한
    /// 패턴은 warn 만 남기고 항상 비매칭으로 취급한다(전체 rebuild 를 막지 않음).
    pub(in crate::file::format) fn rebuild<'a>(patterns: impl Iterator<Item = &'a str>) -> Self {
        let mut builder = GlobSetBuilder::new();
        let mut index_of = HashMap::new();
        for pattern in patterns {
            if index_of.contains_key(pattern) {
                continue;
            }
            match Glob::new(pattern) {
                Ok(glob) => {
                    let idx = index_of.len();
                    builder.add(glob);
                    index_of.insert(pattern.to_string(), idx);
                }
                Err(e) => {
                    tracing::warn!(
                        "file_format: path-glob pattern '{pattern}' failed to compile at \
                         finalize (should have been rejected at registration time): {e}"
                    );
                }
            }
        }
        let set = match builder.build() {
            Ok(s) => Some(s),
            Err(e) => {
                tracing::warn!("file_format: GlobSetBuilder::build failed: {e}");
                None
            }
        };
        Self { set, index_of }
    }

    /// `name`(파일명 — 슬래시 없는 단일 컴포넌트)에 매칭되는 패턴들의 인덱스 집합을
    /// 1회 계산한다. 호출자는 파일 하나당 이걸 한 번만 호출해 재사용해야 한다.
    pub(in crate::file::format) fn matched_indices(&self, name: &str) -> HashSet<usize> {
        match &self.set {
            Some(set) => set.matches(name).into_iter().collect(),
            None => HashSet::new(),
        }
    }

    /// 특정 패턴 문자열이 (이미 계산된) 매칭 인덱스 집합에 포함되는지 — rule 평가
    /// 시점의 O(1) membership 조회.
    pub(in crate::file::format) fn pattern_matched(
        &self,
        pattern: &str,
        matched: &HashSet<usize>,
    ) -> bool {
        self.index_of
            .get(pattern)
            .is_some_and(|idx| matched.contains(idx))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_patterns_never_match() {
        let cache = PathGlobCache::rebuild(std::iter::empty());
        assert!(cache.matched_indices("anything.rs").is_empty());
        assert!(!cache.pattern_matched("*.rs", &HashSet::new()));
    }

    #[test]
    fn duplicate_patterns_share_one_index() {
        let cache = PathGlobCache::rebuild(["*.rs", "*.rs", "*.toml"].into_iter());
        let matched = cache.matched_indices("main.rs");
        assert!(cache.pattern_matched("*.rs", &matched));
        assert!(!cache.pattern_matched("*.toml", &matched));
    }

    #[test]
    fn matched_indices_reflects_multiple_patterns() {
        let cache =
            PathGlobCache::rebuild(["Dockerfile", "*.config.json", "file?.txt"].into_iter());

        let d = cache.matched_indices("Dockerfile");
        assert!(cache.pattern_matched("Dockerfile", &d));
        assert!(!cache.pattern_matched("*.config.json", &d));

        let c = cache.matched_indices("bar.config.json");
        assert!(cache.pattern_matched("*.config.json", &c));
        assert!(!cache.pattern_matched("Dockerfile", &c));

        let q = cache.matched_indices("file1.txt");
        assert!(cache.pattern_matched("file?.txt", &q));
    }

    #[test]
    fn unknown_pattern_string_never_matches() {
        let cache = PathGlobCache::rebuild(["*.rs"].into_iter());
        let matched = cache.matched_indices("main.rs");
        // registry 에 등록된 적 없는 패턴 문자열 — index_of 에 없으므로 false.
        assert!(!cache.pattern_matched("*.toml", &matched));
    }
}
