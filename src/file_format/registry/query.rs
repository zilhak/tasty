//! `FileFormatRegistry` — query 도메인.

use std::collections::BTreeMap;
use std::path::Path;

use tracing::warn;

use super::helpers::{
    decl_rule_to_kind, identify_by_extension_priority, install_extension_priority, install_one,
    parse_detector_section, parse_extension_priority_section, path_extension_lowercase,
    rule_kind_eq, rule_kind_to_toml,
};
use super::{DetectorContribution, ExtensionPriorityEntry, FileFormatRegistry};
use crate::file_format::config::{
    DetectorDecl, DetectorRuleDecl, ExtensionPriorityDecl, validate_detector_decl,
};
use crate::file_format::evaluator::{DeepCtx, evaluate_cheap, evaluate_deep};
use crate::file_format::types::{
    DetectDepth, DetectorId, DetectorRule, DetectorRuleKind, FileFormatDetector, FileTarget,
    RuleOrigin,
};

impl FileFormatRegistry {
    /// detector 조회 — clone 반환.
    pub fn detector(&self, id: &DetectorId) -> Option<FileFormatDetector> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        inner.finalized.get(id).cloned()
    }

    pub fn list_detectors(&self) -> Vec<DetectorId> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        inner.finalized.keys().cloned().collect()
    }

    /// `target` 에 매칭되는 detector id 결정. 매칭 실패 시 `None` (= unknown).
    ///
    /// - `DetectDepth::Cheap`: file IO 없음. 확장자/glob/is-directory 만.
    /// - `DetectDepth::Deep`: 같은 cheap rule 들 + magic/MIME 까지 평가. 한 호출 동안
    ///   `DeepCtx` 로 head/MIME 캐시 → detector 가 여러 magic rule 을 가져도 head 는 1회만 read.
    ///
    /// Phase E 의 확장자 fast path: 파일이고 확장자가 있으면 광고 confirmed detector 중
    /// `extension_priority` 표 + `install_order` 순서로 결정적 1순위 선택. 표 적용 결과가
    /// 비면 기존 BTreeMap 순회 (PathGlob / IsDirectory / Magic / MIME 등) 로 fallback.
    pub fn identify(&self, target: &FileTarget, depth: DetectDepth) -> Option<DetectorId> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        let is_dir = target.is_directory();

        // 확장자 fast path — 파일에만 적용. 디렉토리는 IsDirectory pre-filter 로 처리.
        if !is_dir {
            if let Some(ext) = path_extension_lowercase(target.as_path()) {
                if let Some(id) = identify_by_extension_priority(&inner, &ext) {
                    return Some(id);
                }
            }
        }

        let mut deep_ctx = match depth {
            DetectDepth::Deep => Some(DeepCtx::new()),
            DetectDepth::Cheap => None,
        };

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
            // 매칭: OR — 하나라도 match 면 detector 매칭.
            for rule in &det.rules {
                let matched = match deep_ctx.as_mut() {
                    Some(ctx) => evaluate_deep(&rule.kind, target, ctx),
                    None => evaluate_cheap(&rule.kind, target),
                };
                if matched {
                    return Some(id.clone());
                }
            }
        }
        None
    }

    // ── install / uninstall ────────────────────────────────────────────
}
