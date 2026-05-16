//! `FileHandlerRegistry` — 등록된 핸들러들을 관리하고 detector 별로 정렬해 반환.

use std::collections::BTreeMap;
use std::sync::RwLock;

use crate::file_format::DetectorId;

use super::config::{HandlerDecl, PluginHandlerActionDecl};
use super::types::{FileHandler, HandlerId};

pub struct FileHandlerRegistry {
    handlers: RwLock<BTreeMap<HandlerId, FileHandler>>,
}

impl FileHandlerRegistry {
    pub fn new() -> Self {
        Self {
            handlers: RwLock::new(BTreeMap::new()),
        }
    }

    pub fn handler(&self, _id: &HandlerId) -> Option<FileHandler> {
        None
    }

    pub fn list_handlers(&self) -> Vec<HandlerId> {
        Vec::new()
    }

    /// `detector` 에 attach 된 활성 handler 들. priority 오름차순, tie 시 `user > plugin > host`.
    pub fn handlers_for(&self, _detector: &DetectorId) -> Vec<FileHandler> {
        Vec::new()
    }

    /// Picker modal 용 — 모든 enabled handler.
    pub fn all_handlers(&self) -> Vec<FileHandler> {
        Vec::new()
    }

    // ── install / uninstall ────────────────────────────────────────────
    // M3 에서 본격 구현. 시그니처만 stub.

    pub fn install_host_defaults(&self, _toml_text: &str) {}

    pub fn install_user_config(&self, _path: &std::path::Path) {}

    pub fn install_plugin_handlers(
        &self,
        _plugin_id: &str,
        _decls: &[HandlerDecl<PluginHandlerActionDecl>],
    ) {
    }

    pub fn uninstall_plugin(&self, _plugin_id: &str) {}
}

impl Default for FileHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}
