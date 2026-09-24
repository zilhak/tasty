//! PathGlob 패턴을 GlobSet으로 함께 컴파일한다. dirty일 때만 재구성하며
//! 파일마다 매칭 인덱스를 한 번 구해 여러 규칙이 공유한다.

use std::collections::{HashMap, HashSet};

use globset::{Glob, GlobSet, GlobSetBuilder};

/// 컴파일된 PathGlob 매처 묶음. 패턴 문자열(`to_slash` 로 이미 정규화된 `/` 형태)
/// → `GlobSet` 내 인덱스. 동일 패턴 문자열은 여러 detector 가 공유해도 한 번만
/// 컴파일한다.
#[derive(Default)]
pub(crate) struct PathGlobCache {
    set: Option<GlobSet>,
    index_of: HashMap<String, usize>,
}

impl PathGlobCache {
    /// 중복 패턴은 한 번만 컴파일한다. 등록 단계에서 문법과 / 정규화를 확인하지만,
    /// 여기서 실패해도 경고를 남기고 해당 패턴만 비매칭으로 처리한다.
    pub(crate) fn rebuild<'a>(patterns: impl Iterator<Item = &'a str>) -> Self {
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
    pub(crate) fn matched_indices(&self, name: &str) -> HashSet<usize> {
        match &self.set {
            Some(set) => set.matches(name).into_iter().collect(),
            None => HashSet::new(),
        }
    }

    /// 특정 패턴 문자열이 (이미 계산된) 매칭 인덱스 집합에 포함되는지 — rule 평가
    /// 시점의 O(1) membership 조회.
    pub(crate) fn pattern_matched(&self, pattern: &str, matched: &HashSet<usize>) -> bool {
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
        assert!(!cache.pattern_matched("*.toml", &matched));
    }
}
