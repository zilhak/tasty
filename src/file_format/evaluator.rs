//! detector rule 평가자. M3 에서 cheap path 본격 구현.

use super::types::{DetectorRuleKind, FileTarget};

/// 단일 rule 이 target 에 매칭되는지 평가. cheap path 만 (Phase A).
pub fn evaluate_cheap(_rule: &DetectorRuleKind, _target: &FileTarget) -> bool {
    // M3 에서 채움.
    false
}
