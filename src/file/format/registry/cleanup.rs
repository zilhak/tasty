//! `FileFormatRegistry` — cleanup 도메인.

use std::collections::BTreeMap;
use std::path::Path;

use tracing::warn;

use super::helpers::{
    decl_rule_to_kind, identify_by_extension_priority, install_extension_priority, install_one,
    parse_detector_section, parse_extension_priority_section, path_extension_lowercase,
    rule_kind_eq, rule_kind_to_toml,
};
use super::{DetectorContribution, ExtensionPriorityEntry, FileFormatRegistry};
use crate::file::format::config::{
    DetectorDecl, DetectorRuleDecl, ExtensionPriorityDecl, validate_detector_decl,
};
use crate::file::format::evaluator::{DeepCtx, evaluate_cheap, evaluate_deep};
use crate::file::format::types::{
    DetectDepth, DetectorId, DetectorRule, DetectorRuleKind, FileFormatDetector, FileTarget,
    RuleOrigin,
};

impl FileFormatRegistry {
    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let engine = &self.engine_state;
        let _ = engine;
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(&c.origin, RuleOrigin::Plugin(p) if p == plugin_id));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
            inner.install_order.remove(&id);
        }
        inner.dirty = true;
    }
}
