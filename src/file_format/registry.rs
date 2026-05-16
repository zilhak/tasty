//! `FileFormatRegistry` — 등록된 detector 들을 관리하고 file 을 identify 한다.
//!
//! 부팅 / plugin enable / 사용자 설정 reload 시 install 함수들로 채워진다.
//! identify 는 read-lock 한 번이라 hot path 부담 작다.

use std::collections::BTreeMap;
use std::sync::RwLock;

use super::types::{DetectDepth, DetectorId, FileFormatDetector, FileTarget};

/// detector 들의 BTreeMap (deterministic iteration). install 시 마지막 출처가
/// 메타데이터 patch, rule union 으로 합친다.
pub struct FileFormatRegistry {
    detectors: RwLock<BTreeMap<DetectorId, FileFormatDetector>>,
}

impl FileFormatRegistry {
    pub fn new() -> Self {
        Self {
            detectors: RwLock::new(BTreeMap::new()),
        }
    }

    /// detector 조회 (clone 반환 — read-lock 짧게 잡고 해제).
    pub fn detector(&self, _id: &DetectorId) -> Option<FileFormatDetector> {
        // M3 에서 채움
        None
    }

    pub fn list_detectors(&self) -> Vec<DetectorId> {
        // M3 에서 채움
        Vec::new()
    }

    /// `target` 에 매칭되는 detector id 를 결정. 매칭 실패 시 `None` (= unknown).
    pub fn identify(&self, _target: &FileTarget, _depth: DetectDepth) -> Option<DetectorId> {
        // M3 에서 채움
        None
    }

    // ── install / uninstall ────────────────────────────────────────────
    // M3 에서 본격 구현. 시그니처만 stub.

    pub fn install_host_defaults(&self, _toml_text: &str) {}

    pub fn install_user_config(&self, _path: &std::path::Path) {}

    pub fn install_plugin_detectors(
        &self,
        _plugin_id: &str,
        _decls: &[super::config::DetectorDecl],
    ) {
    }

    pub fn uninstall_plugin(&self, _plugin_id: &str) {}
}

impl Default for FileFormatRegistry {
    fn default() -> Self {
        Self::new()
    }
}
