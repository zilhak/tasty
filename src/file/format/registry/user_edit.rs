//! `FileFormatRegistry` — user_edit 도메인.

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
    /// Settings UI 가 host/plugin detector 를 user-origin override 로 disable/enable.
    /// 명시적 user 의도를 표현하므로 항상 `disabled_override = Some(value)` 로 push 한다.
    /// "default 로 되돌리기" 는 `clear_user_detector_override` 또는 `remove_user_detector`.
    pub fn set_user_detector_disabled(&self, id: &DetectorId, disabled: bool) {
        let engine = &self.engine_state;
        let _ = engine;
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else {
            warn!(
                detector = id.as_str(),
                "file_format: set_user_detector_disabled — unknown detector"
            );
            return;
        };
        // 같은 detector 에 이미 user contribution 이 있으면 (rules/메타 포함) 그 disabled_override 만
        // 갱신해 다른 필드를 보존. 없으면 disabled-only contribution 신규 push.
        if let Some(existing) = entry
            .iter_mut()
            .find(|c| matches!(c.origin, RuleOrigin::User))
        {
            existing.disabled_override = Some(disabled);
        } else {
            entry.push(DetectorContribution {
                origin: RuleOrigin::User,
                display_name_i18n_key: None,
                icon: None,
                disabled_override: Some(disabled),
                rules: Vec::new(),
            });
        }
        inner.dirty = true;
    }

    /// User-origin contribution 의 `disabled_override` 만 None 으로 비운다. 다른 user 필드
    /// (rule/메타) 는 보존. user 가 명시적 disable 의도를 철회할 때 사용.
    pub fn clear_user_detector_override(&self, id: &DetectorId) {
        let engine = &self.engine_state;
        let _ = engine;
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        let mut empty_user = false;
        if let Some(existing) = entry
            .iter_mut()
            .find(|c| matches!(c.origin, RuleOrigin::User))
        {
            existing.disabled_override = None;
            empty_user = existing.rules.is_empty()
                && existing.display_name_i18n_key.is_none()
                && existing.icon.is_none();
        }
        if empty_user {
            entry.retain(|c| !matches!(c.origin, RuleOrigin::User));
            if entry.is_empty() {
                inner.contributions.remove(id);
                inner.install_order.remove(id);
            }
        }
        inner.dirty = true;
    }

    /// Settings UI 가 user-origin contribution 전체를 제거. host/plugin 은 보존.
    /// 해당 detector 의 다른 출처가 없었다면 (= user-only) detector 전체가 사라진다.
    pub fn remove_user_detector(&self, id: &DetectorId) {
        let engine = &self.engine_state;
        let _ = engine;
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        entry.retain(|c| !matches!(c.origin, RuleOrigin::User));
        if entry.is_empty() {
            inner.contributions.remove(id);
            inner.install_order.remove(id);
        }
        inner.dirty = true;
    }

    /// Settings UI 가 user-origin detector 를 추가/갱신. 기존 host/plugin 이 있으면 patch
    /// (rule union + 메타 last-writer-wins). schema validation 실패 시 변경 없이 에러 반환.
    pub fn upsert_user_detector(
        &self,
        decl: DetectorDecl,
    ) -> Result<(), crate::file::format::config::DetectorDeclError> {
        let engine = &self.engine_state;
        let _ = engine;
        // user 영역도 `$` 예약 id 는 host 만 정의 가능 → from_plugin = true 와 동일 규칙.
        // 단 user 가 host 의 예약 detector ($directory) 를 patch 하는 시나리오는 있을 수 있어,
        // 이미 등록된 id 면 허용.
        let already_exists = self
            .inner
            .read()
            .map(|g| g.contributions.contains_key(&DetectorId(decl.id.clone())))
            .unwrap_or(false);
        let from_restricted = !already_exists;
        let warnings = crate::file::format::config::validate_detector_decl(&decl, from_restricted)?;
        for w in warnings {
            warn!(warning = %w, "file_format: user detector decl warning");
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => {
                return Err(crate::file::format::config::DetectorDeclError::InvalidId(
                    "lock poisoned".into(),
                ));
            }
        };
        install_one(
            &mut inner,
            &self.next_install_order,
            decl,
            RuleOrigin::User,
            false,
        );
        inner.dirty = true;
        Ok(())
    }

    /// 사용자 설정 파일을 다시 읽어 user origin contribution 만 교체. host + plugin 은 그대로.
    ///
    /// **Transactional**: 파일 read/parse 단계에서 실패하면 기존 user contribution 을 보존한다
    /// (write lock 잡기 전에 검증). 파일이 없으면 user contribution 만 제거 (= 설정 삭제).
    pub fn reload_user_config(&self, path: &Path) {
        let engine = &self.engine_state;
        let _ = engine;
        let (decls, priorities) = match std::fs::read_to_string(path) {
            Ok(text) => {
                let d = match parse_detector_section(&text) {
                    Ok(d) => d,
                    Err(e) => {
                        warn!(
                            path = %path.display(),
                            error = %e,
                            "file_format: reload aborted — parse failed, keeping previous user config",
                        );
                        return;
                    }
                };
                let p = parse_extension_priority_section(&text).unwrap_or_default();
                (d, p)
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => (Vec::new(), Vec::new()),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "file_format: reload aborted — read failed, keeping previous user config",
                );
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        // 기존 user origin contribution 모두 제거.
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(c.origin, RuleOrigin::User));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
            inner.install_order.remove(&id);
        }
        // 기존 user origin extension_priority 모두 제거.
        let user_keys: Vec<String> = inner
            .extension_priority
            .iter()
            .filter_map(|(k, v)| matches!(v.origin, RuleOrigin::User).then(|| k.clone()))
            .collect();
        for k in user_keys {
            inner.extension_priority.remove(&k);
        }
        // 새 user contribution install.
        for decl in decls {
            install_one(
                &mut inner,
                &self.next_install_order,
                decl,
                RuleOrigin::User,
                false,
            );
        }
        for p in priorities {
            install_extension_priority(&mut inner, p, RuleOrigin::User);
        }
        inner.dirty = true;
    }
}
