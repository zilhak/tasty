//! 파일 형식 식별 시스템.
//!
//! `FileFormatRegistry` 가 모든 detector 를 보관하고 `identify` 가 매칭되는
//! `DetectorId` 를 반환한다. 매칭 실패는 `None` (= unknown) — 등록된 별도
//! "$unknown" detector 는 없다.

#![allow(dead_code, unused_imports)]

pub mod config;
pub(crate) mod evaluator;
pub(crate) mod lua_eval;
pub mod registry;
pub mod types;

pub use registry::FileFormatRegistry;
pub use types::{
    is_valid_detector_id, DetectDepth, DetectorId, DetectorRule, DetectorRuleKind,
    FileFormatDetector, FileTarget, RuleOrigin,
};
