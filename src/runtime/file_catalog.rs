//! Shared registry queries for editors. No mutators or file execution are exposed.
#![cfg(feature = "gui")]
use std::sync::Arc;
use tasty_file_format::{
    DetectorId, DetectorInfo, FileFormatDetector, FileFormatRegistry, RuleOrigin,
};
use tasty_file_handler::{FileHandler, FileHandlerRegistry, HandlerId};
#[derive(Clone)]
pub(crate) struct FormatCatalog(Arc<FileFormatRegistry>);
impl FormatCatalog {
    pub(crate) fn new(registry: Arc<FileFormatRegistry>) -> Self {
        Self(registry)
    }
    pub(crate) fn detector(&self, id: &DetectorId) -> Option<FileFormatDetector> {
        self.0.detector(id)
    }
    pub(crate) fn list_detectors(&self) -> Vec<DetectorId> {
        self.0.list_detectors()
    }
    pub(crate) fn rule_origins(&self, id: &DetectorId) -> Vec<RuleOrigin> {
        self.0.rule_origins(id)
    }
    pub(crate) fn has_user_contribution(&self, id: &DetectorId) -> bool {
        self.0.has_user_contribution(id)
    }
    pub(crate) fn extension_priority_order(&self, extension: &str) -> Option<Vec<DetectorId>> {
        self.0.extension_priority_order(extension)
    }
    pub(crate) fn extension_priority_keys(&self) -> Vec<String> {
        self.0.extension_priority_keys()
    }
}
impl DetectorInfo for FormatCatalog {
    fn advertised_extensions(&self, id: &DetectorId) -> Vec<String> {
        self.0.advertised_extensions(id)
    }
    fn detectors_for_extension(&self, extension: &str) -> Vec<DetectorId> {
        self.0.detectors_for_extension(extension)
    }
    fn all_advertised_extensions(&self) -> Vec<String> {
        self.0.all_advertised_extensions()
    }
    fn is_enabled(&self, id: &DetectorId) -> bool {
        self.0.is_enabled(id)
    }
}
#[derive(Clone)]
pub(crate) struct HandlerCatalog(Arc<FileHandlerRegistry>);
impl HandlerCatalog {
    pub(crate) fn new(registry: Arc<FileHandlerRegistry>) -> Self {
        Self(registry)
    }
    pub(crate) fn handler(&self, id: &HandlerId) -> Option<FileHandler> {
        self.0.handler(id)
    }
    pub(crate) fn list_handlers(&self) -> Vec<HandlerId> {
        self.0.list_handlers()
    }
}
pub(crate) fn hook_handlers() -> Vec<crate::hook_handler::HookHandler> {
    crate::hook_handler::global().all_handlers_including_disabled()
}
pub(crate) fn hook_handler_defaults()
-> std::collections::BTreeMap<crate::hook_handler::HookHandlerId, crate::hook_handler::HookHandler>
{
    crate::hook_handler::global().patched_defaults()
}
