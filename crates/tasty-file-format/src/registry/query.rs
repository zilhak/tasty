//! `FileFormatRegistry` — query 도메인.

use super::FileFormatRegistry;
use super::helpers::{identify_by_extension_priority, path_extension_lowercase};
use crate::evaluator::{DeepCtx, evaluate_cheap, evaluate_deep};
use crate::types::{
    DetectDepth, DetectorId, DetectorRuleKind, FileFormatDetector, FileTarget, RuleOrigin,
};

impl FileFormatRegistry {
    /// detector 조회 — clone 반환.
    pub fn detector(&self, id: &DetectorId) -> Option<FileFormatDetector> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.get(id).cloned()
    }

    pub fn list_detectors(&self) -> Vec<DetectorId> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.keys().cloned().collect()
    }

    /// user가 규칙·메타데이터·disabled 중 하나라도 선언했는지 확인한다.
    /// 병합된 규칙은 중복 제거로 출처를 잃을 수 있어 원래 contribution을 확인한다.
    pub fn has_user_contribution(&self, id: &DetectorId) -> bool {
        let inner = self.lock_read();
        inner
            .contributions
            .get(id)
            .is_some_and(|cs| cs.iter().any(|c| matches!(c.origin, RuleOrigin::User)))
    }

    /// rule 을 하나 이상 선언한 출처들 — 병합 순서(Host → Plugin → User), 중복 없이.
    ///
    /// finalize 된 rule 의 origin 과 다르다: 그쪽은 dedupe 로 같은 rule 의 뒤 출처를 지운다.
    /// 여기는 contribution 을 보므로 같은 rule 을 함께 적은 출처도 모두 나온다.
    pub fn rule_origins(&self, id: &DetectorId) -> Vec<RuleOrigin> {
        let inner = self.lock_read();
        let mut origins: Vec<RuleOrigin> = Vec::new();
        if let Some(cs) = inner.contributions.get(id) {
            for c in super::merge_order(cs) {
                if !c.rules.is_empty() && !origins.contains(&c.origin) {
                    origins.push(c.origin.clone());
                }
            }
        }
        origins
    }

    /// 매칭 detector를 반환하며 없으면 None이다. Cheap은 파일 내용 없이 판정하고,
    /// Deep은 magic·MIME·Lua·구조 검증도 수행하며 DeepCtx로 head/MIME을 재사용한다.
    /// 파일 확장자의 후보는 우선순위 표와 설치 순서로 고른다. 후보가 없으면 나머지 규칙을 순회한다.
    pub fn identify(&self, target: &FileTarget, depth: DetectDepth) -> Option<DetectorId> {
        // URL은 DispatchTarget::Url에서 처리한다. 마지막 경로의 확장자를 로컬 파일로 오인하지 않는다.
        if target.is_url_shaped() {
            return None;
        }
        self.ensure_finalized();
        let inner = self.lock_read();
        let is_dir = target.is_directory();

        // 확장자 fast path — 파일에만 적용. 디렉토리는 IsDirectory pre-filter 로 처리.
        if !is_dir
            && let Some(ext) = path_extension_lowercase(target.as_path())
            && let Some(id) = identify_by_extension_priority(&inner, &ext)
        {
            return Some(id);
        }

        let mut deep_ctx = match depth {
            DetectDepth::Deep => Some(DeepCtx::new()),
            DetectDepth::Cheap => None,
        };

        // 파일마다 매칭 인덱스를 한 번 구하고 아래 규칙들이 공유한다.
        let matched_globs = target
            .as_path()
            .file_name()
            .and_then(|s| s.to_str())
            .map(|name| inner.path_globs.matched_indices(name))
            .unwrap_or_default();

        for (id, det) in inner.finalized.iter() {
            if det.disabled {
                continue;
            }
            // pre-filter: 디렉토리면 IsDirectory rule 가진 detector 만 평가.
            // 파일이면 IsDirectory rule 가진 detector 는 제외 (즉 file vs dir 단순 분리).
            let has_is_dir = det
                .rules
                .iter()
                .any(|r| matches!(r.kind, DetectorRuleKind::IsDirectory));
            if is_dir != has_is_dir {
                continue;
            }
            for rule in &det.rules {
                let matched = match &rule.kind {
                    DetectorRuleKind::PathGlob { pattern } => {
                        inner.path_globs.pattern_matched(pattern, &matched_globs)
                    }
                    _ => match deep_ctx.as_mut() {
                        Some(ctx) => evaluate_deep(&rule.kind, target, ctx),
                        None => evaluate_cheap(&rule.kind, target),
                    },
                };
                if matched {
                    return Some(id.clone());
                }
            }
        }
        None
    }
}
