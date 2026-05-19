//! `FileFormatRegistry` — 등록된 detector 들을 관리하고 file 을 identify 한다.
//!
//! 출처별 contribution 을 따로 보관해 plugin uninstall 시 원본 복원 가능.
//! finalize 는 incremental — install/uninstall 호출 후 dirty 표시 + identify 시 1회.

mod helpers;
#[cfg(test)]
mod tests;

use helpers::{
    decl_rule_to_kind, hex_to_bytes, identify_by_extension_priority, install_extension_priority,
    install_one, parse_detector_section, parse_extension_priority_section,
    path_extension_lowercase, rule_kind_eq, rule_kind_to_toml,
};

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::RwLock;

use tracing::warn;

use super::config::{
    validate_detector_decl, DetectorDecl, DetectorRuleDecl, ExtensionPriorityDecl,
};
use super::evaluator::{evaluate_cheap, evaluate_deep, DeepCtx};
use super::info::DetectorInfo;
use super::types::{
    DetectDepth, DetectorId, DetectorRule, DetectorRuleKind, FileFormatDetector, FileTarget,
    RuleOrigin,
};

/// 한 출처가 단일 detector 에 기여한 내용.
#[derive(Debug, Clone)]
struct DetectorContribution {
    origin: RuleOrigin,
    display_name_i18n_key: Option<String>,
    icon: Option<String>,
    /// `disabled` 는 명시된 출처만 적용 (patch). `None` 이면 무시.
    disabled_override: Option<bool>,
    rules: Vec<DetectorRuleKind>,
}

/// 확장자별 우선순위 항목 (출처 메타 포함). user export 시 origin 으로 필터.
#[derive(Debug, Clone)]
struct ExtensionPriorityEntry {
    order: Vec<DetectorId>,
    origin: RuleOrigin,
}

struct Inner {
    /// detector id → 출처별 contribution. install 순서대로 push (host → plugin → user).
    contributions: BTreeMap<DetectorId, Vec<DetectorContribution>>,
    /// detector id → 최초 install 시점의 monotonic counter 값. 후속 patch 에 의해 변하지
    /// 않는다 (`install_one` 이 entry 가 비었을 때만 부여).
    install_order: BTreeMap<DetectorId, u64>,
    /// 확장자별 우선순위 표 (Phase E). 같은 확장자에 둘 이상의 출처가 적으면
    /// last-writer-wins (install 순서 host → user). user export 시에는 user origin 만 emit.
    extension_priority: BTreeMap<String, ExtensionPriorityEntry>,
    /// finalize 결과 cache. dirty 시 lazy 재계산.
    finalized: BTreeMap<DetectorId, FileFormatDetector>,
    dirty: bool,
}

pub struct FileFormatRegistry {
    inner: RwLock<Inner>,
    /// 다음 install 시 부여할 monotonic 카운터. install_one 이 새 detector id 에 대해
    /// fetch_add 로 받아온다.
    next_install_order: AtomicU64,
}

impl FileFormatRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                install_order: BTreeMap::new(),
                extension_priority: BTreeMap::new(),
                finalized: BTreeMap::new(),
                dirty: false,
            }),
            next_install_order: AtomicU64::new(0),
        }
    }

    /// `extension` 에 대한 사용자 우선순위 표. 적힌 detector id 들 (등록 여부 무관).
    /// 표에 없으면 `None`.
    pub fn extension_priority_order(&self, extension: &str) -> Option<Vec<DetectorId>> {
        let key = extension.trim_start_matches('.').to_ascii_lowercase();
        let inner = self.inner.read().ok()?;
        inner.extension_priority.get(&key).map(|e| e.order.clone())
    }

    /// Settings UI 가 확장자 우선순위를 변경할 때 호출. `RuleOrigin::User` 로 entry 를
    /// 덮어쓴다 (last-writer-wins). `order` 가 비면 entry 제거.
    pub fn set_user_extension_priority(&self, extension: &str, order: Vec<DetectorId>) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let decl = ExtensionPriorityDecl {
            extension: extension.to_string(),
            order: order.into_iter().map(|id| id.0).collect(),
        };
        install_extension_priority(&mut inner, decl, RuleOrigin::User);
    }

    /// 사용자가 명시적으로 우선순위를 해제. `extension_priority` 표에서 해당 entry 삭제.
    /// (host default 가 있던 entry 라 해도 함께 제거 — single-entry 구조)
    pub fn clear_user_extension_priority(&self, extension: &str) {
        let key = extension.trim_start_matches('.').to_ascii_lowercase();
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        inner.extension_priority.remove(&key);
    }

    /// 등록된 모든 `extension_priority` entry 의 키 (`md`, `json` 등). UI 에서 "현재 표가
    /// 있는 확장자" 를 보여주기 위해 사용.
    pub fn extension_priority_keys(&self) -> Vec<String> {
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        inner.extension_priority.keys().cloned().collect()
    }

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

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_detector_section(toml_text) {
            Ok(d) => d,
            Err(e) => {
                warn!(error = %e, "file_format: failed to parse host defaults");
                return;
            }
        };
        let priorities = parse_extension_priority_section(toml_text).unwrap_or_default();
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_one(
                &mut inner,
                &self.next_install_order,
                decl,
                RuleOrigin::HostDefault,
                false,
            );
        }
        for p in priorities {
            install_extension_priority(&mut inner, p, RuleOrigin::HostDefault);
        }
        inner.dirty = true;
    }

    pub fn install_user_config(&self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_format: user config read failed");
                return;
            }
        };
        let decls = match parse_detector_section(&text) {
            Ok(d) => d,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_format: user config parse failed");
                return;
            }
        };
        let priorities = parse_extension_priority_section(&text).unwrap_or_default();
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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

    pub fn install_plugin_detectors(&self, plugin_id: &str, decls: &[DetectorDecl]) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            // Plugin 출처의 Lua detector rule 은 차단: host/user 만 사용자 권한으로
            // 실행되는 신뢰 영역. plugin 의 임의 Lua 는 sandbox 가 있어도 정책상 금지.
            let mut decl = decl.clone();
            let before = decl.rule.len();
            decl.rule
                .retain(|r| !matches!(r, DetectorRuleDecl::Lua { .. }));
            let dropped = before - decl.rule.len();
            if dropped > 0 {
                warn!(
                    plugin = plugin_id,
                    detector = %decl.id,
                    dropped,
                    "file_format: dropping Lua detector rules from plugin (host/user only)",
                );
            }
            if decl.rule.is_empty() {
                // 모든 rule 이 Lua 였다면 detector 자체 install 의미 없음.
                continue;
            }
            install_one(
                &mut inner,
                &self.next_install_order,
                decl,
                RuleOrigin::Plugin(plugin_id.to_string()),
                true,
            );
        }
        inner.dirty = true;
    }

    /// Settings UI 가 host/plugin detector 를 user-origin override 로 disable/enable.
    /// 명시적 user 의도를 표현하므로 항상 `disabled_override = Some(value)` 로 push 한다.
    /// "default 로 되돌리기" 는 `clear_user_detector_override` 또는 `remove_user_detector`.
    pub fn set_user_detector_disabled(&self, id: &DetectorId, disabled: bool) {
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
        if let Some(existing) = entry.iter_mut().find(|c| matches!(c.origin, RuleOrigin::User)) {
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else { return };
        let mut empty_user = false;
        if let Some(existing) = entry.iter_mut().find(|c| matches!(c.origin, RuleOrigin::User)) {
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else { return };
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
    ) -> Result<(), super::config::DetectorDeclError> {
        // user 영역도 `$` 예약 id 는 host 만 정의 가능 → from_plugin = true 와 동일 규칙.
        // 단 user 가 host 의 예약 detector ($directory) 를 patch 하는 시나리오는 있을 수 있어,
        // 이미 등록된 id 면 허용.
        let already_exists = self
            .inner
            .read()
            .map(|g| g.contributions.contains_key(&DetectorId(decl.id.clone())))
            .unwrap_or(false);
        let from_restricted = !already_exists;
        let warnings = super::config::validate_detector_decl(&decl, from_restricted)?;
        for w in warnings {
            warn!(warning = %w, "file_format: user detector decl warning");
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => {
                return Err(super::config::DetectorDeclError::InvalidId(
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

    /// user 출처 contribution 만 모아 TOML 문자열로 직렬화. Settings UI 가 변경 사항을
    /// `~/.tasty/file-formats.toml` 에 저장할 때 사용.
    ///
    /// host default / plugin contribution 은 포함하지 않는다 (그것들은 자기 출처가 다시
    /// install 한다). Round-trip 보장 — `parse_detector_section(&export)` 로 원래 user
    /// contribution 을 그대로 재현 가능. `Unknown` rule 의 raw payload 도 보존.
    ///
    /// 주의: TOML 주석/공백/key 순서는 보존하지 않는다 (재발급). 사용자 손편집 친화적
    /// round-trip 이 필요해지면 `toml_edit` 도입.
    pub fn export_user_config(&self) -> String {
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
        let mut doc = toml::value::Table::new();
        let mut detectors = Vec::<toml::Value>::new();
        for (id, contribs) in inner.contributions.iter() {
            let user = contribs.iter().find(|c| matches!(c.origin, RuleOrigin::User));
            let Some(user) = user else { continue };
            if user.rules.is_empty()
                && user.display_name_i18n_key.is_none()
                && user.icon.is_none()
                && user.disabled_override.is_none()
            {
                continue;
            }
            let mut det = toml::value::Table::new();
            det.insert(
                "id".to_string(),
                toml::Value::String(id.as_str().to_string()),
            );
            if let Some(k) = &user.display_name_i18n_key {
                det.insert(
                    "display_name_i18n_key".to_string(),
                    toml::Value::String(k.clone()),
                );
            }
            if let Some(icon) = &user.icon {
                det.insert("icon".to_string(), toml::Value::String(icon.clone()));
            }
            if let Some(d) = user.disabled_override {
                det.insert("disabled".to_string(), toml::Value::Boolean(d));
            }
            let rules: Vec<toml::Value> = user
                .rules
                .iter()
                .map(|k| toml::Value::Table(rule_kind_to_toml(k)))
                .collect();
            if !rules.is_empty() {
                det.insert("rule".to_string(), toml::Value::Array(rules));
            }
            detectors.push(toml::Value::Table(det));
        }
        // user origin extension_priority emit.
        let mut priorities = Vec::<toml::Value>::new();
        for (ext, entry) in inner.extension_priority.iter() {
            if !matches!(entry.origin, RuleOrigin::User) {
                continue;
            }
            let mut t = toml::value::Table::new();
            t.insert("extension".into(), toml::Value::String(ext.clone()));
            t.insert(
                "order".into(),
                toml::Value::Array(
                    entry
                        .order
                        .iter()
                        .map(|id| toml::Value::String(id.as_str().to_string()))
                        .collect(),
                ),
            );
            priorities.push(toml::Value::Table(t));
        }
        if detectors.is_empty() && priorities.is_empty() {
            return String::new();
        }
        if !detectors.is_empty() {
            doc.insert("detector".to_string(), toml::Value::Array(detectors));
        }
        if !priorities.is_empty() {
            doc.insert(
                "extension_priority".to_string(),
                toml::Value::Array(priorities),
            );
        }
        toml::to_string(&doc).unwrap_or_default()
    }

    /// `export_user_config` 의 결과를 `path` 에 atomic write. tempfile + rename 으로
    /// 부분 쓰기 방지. 빈 결과 (user contribution 없음) 면 path 가 존재하면 빈 파일로
    /// 덮어쓴다 — 사용자가 모든 항목을 지웠다는 의미.
    pub fn save_user_config(&self, path: &Path) -> std::io::Result<()> {
        use std::io::Write;
        let text = self.export_user_config();
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(text.as_bytes())?;
        tmp.flush()?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
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

    fn ensure_finalized(&self) {
        let needs_finalize = self
            .inner
            .read()
            .map(|g| g.dirty)
            .unwrap_or(false);
        if !needs_finalize {
            return;
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        if !inner.dirty {
            return;
        }
        let mut next = BTreeMap::new();
        for (id, contribs) in inner.contributions.iter() {
            let mut display = None;
            let mut icon = None;
            let mut disabled = false;
            let mut rules: Vec<DetectorRule> = Vec::new();
            for c in contribs {
                if c.display_name_i18n_key.is_some() {
                    display = c.display_name_i18n_key.clone();
                }
                if c.icon.is_some() {
                    icon = c.icon.clone();
                }
                if let Some(d) = c.disabled_override {
                    disabled = d;
                }
                for kind in &c.rules {
                    let candidate = DetectorRule {
                        kind: kind.clone(),
                        origin: c.origin.clone(),
                    };
                    // dedupe — 동일 kind 두 번 등록되면 처음 origin 만 보존.
                    if !rules.iter().any(|r| rule_kind_eq(&r.kind, &candidate.kind)) {
                        rules.push(candidate);
                    }
                }
            }
            let install_order = inner.install_order.get(id).copied().unwrap_or(u64::MAX);
            next.insert(
                id.clone(),
                FileFormatDetector {
                    id: id.clone(),
                    display_name_i18n_key: display,
                    icon,
                    rules,
                    disabled,
                    install_order,
                },
            );
        }
        inner.finalized = next;
        inner.dirty = false;
    }
}

impl DetectorInfo for FileFormatRegistry {
    fn advertised_extensions(&self, detector: &DetectorId) -> Vec<String> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let Some(det) = inner.finalized.get(detector) else {
            return Vec::new();
        };
        let mut out: Vec<String> = det
            .rules
            .iter()
            .filter_map(|r| match &r.kind {
                DetectorRuleKind::Extension { values } => Some(values.iter().cloned()),
                _ => None,
            })
            .flatten()
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn detectors_for_extension(&self, ext: &str) -> Vec<DetectorId> {
        self.ensure_finalized();
        let ext_lower = ext.trim_start_matches('.').to_ascii_lowercase();
        if ext_lower.is_empty() {
            return Vec::new();
        }
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut hits: Vec<(u64, DetectorId)> = inner
            .finalized
            .iter()
            .filter(|(_, det)| !det.disabled)
            .filter_map(|(id, det)| {
                let advertises = det.rules.iter().any(|r| {
                    matches!(
                        &r.kind,
                        DetectorRuleKind::Extension { values } if values.iter().any(|v| v == &ext_lower),
                    )
                });
                advertises.then(|| (det.install_order, id.clone()))
            })
            .collect();
        hits.sort_by(|(a_ord, a_id), (b_ord, b_id)| a_ord.cmp(b_ord).then_with(|| a_id.cmp(b_id)));
        hits.into_iter().map(|(_, id)| id).collect()
    }

    fn all_advertised_extensions(&self) -> Vec<String> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut out: Vec<String> = inner
            .finalized
            .values()
            .filter(|det| !det.disabled)
            .flat_map(|det| {
                det.rules.iter().filter_map(|r| match &r.kind {
                    DetectorRuleKind::Extension { values } => Some(values.clone()),
                    _ => None,
                })
            })
            .flatten()
            .collect();
        out.sort();
        out.dedup();
        out
    }

    fn is_enabled(&self, detector: &DetectorId) -> bool {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        inner.finalized.get(detector).is_some_and(|d| !d.disabled)
    }
}

impl Default for FileFormatRegistry {
    fn default() -> Self {
        Self::new()
    }
}
