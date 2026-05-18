//! `FileFormatRegistry` — 등록된 detector 들을 관리하고 file 을 identify 한다.
//!
//! 출처별 contribution 을 따로 보관해 plugin uninstall 시 원본 복원 가능.
//! finalize 는 incremental — install/uninstall 호출 후 dirty 표시 + identify 시 1회.

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

/// 같은 id 의 contribution 을 append. 기존 entry 가 있으면 patch semantics 로 메타데이터
/// 가 마지막 출처에 의해 덮어써진다 (finalize 단계에서 적용).
///
/// 최초 install 시점에만 `install_order` 카운터 값을 부여. 같은 id 의 후속 patch (다른
/// origin) 는 install_order 를 변경하지 않는다.
fn install_one(
    inner: &mut Inner,
    counter: &AtomicU64,
    decl: DetectorDecl,
    origin: RuleOrigin,
    from_plugin: bool,
) {
    // schema 검증
    let validation = validate_detector_decl(&decl, from_plugin);
    let warnings = match validation {
        Ok(w) => w,
        Err(e) => {
            warn!(error = %e, "file_format: rejecting detector decl");
            return;
        }
    };
    for w in warnings {
        warn!(warning = %w, "file_format: detector decl warning");
    }

    let id = DetectorId(decl.id.clone());
    inner
        .install_order
        .entry(id.clone())
        .or_insert_with(|| counter.fetch_add(1, Ordering::SeqCst));
    let entry = inner.contributions.entry(id).or_default();
    // 같은 origin 으로 재install (예: 사용자 설정 reload) 인 경우 기존 동일 origin 제거 후 push.
    entry.retain(|c| c.origin != origin);
    let rule_kinds: Vec<DetectorRuleKind> = decl
        .rule
        .into_iter()
        .filter_map(|r| decl_rule_to_kind(r))
        .collect();
    entry.push(DetectorContribution {
        origin,
        display_name_i18n_key: decl.display_name_i18n_key,
        icon: decl.icon,
        // decl.disabled 가 명시되었는지 schema 상 알 수 없으므로(`#[serde(default)]`),
        // patch semantics 를 위해 false 면 None 으로 취급 (= 끄지 않음). 사용자가 명시적으로
        // disable 하려면 다른 출처가 disabled = true 를 적어 last-writer-wins.
        disabled_override: if decl.disabled { Some(true) } else { None },
        rules: rule_kinds,
    });
}

/// 파일명에서 마지막 확장자를 추출해 소문자로 반환. 점 없음 / 마지막 점 뒤가 빈 문자열
/// 이면 `None`.
fn path_extension_lowercase(path: &Path) -> Option<String> {
    let ext = path.extension()?.to_str()?;
    if ext.is_empty() {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// 확장자 fast path. 광고 confirmed detector 들 중에서:
/// 1. `extension_priority` 표가 있으면 그 순서대로 첫 enabled + IsDirectory 아닌 detector,
/// 2. 표에 없거나 표의 detector 들이 모두 부적격이면 install_order 오름차순으로 첫 detector.
fn identify_by_extension_priority(inner: &Inner, ext: &str) -> Option<DetectorId> {
    // 광고 매칭 + enabled + IsDirectory 아님.
    let mut advertised: Vec<(u64, DetectorId)> = inner
        .finalized
        .iter()
        .filter(|(_, det)| !det.disabled)
        .filter(|(_, det)| {
            !det.rules
                .iter()
                .any(|r| matches!(r.kind, DetectorRuleKind::IsDirectory))
        })
        .filter_map(|(id, det)| {
            let advertises = det.rules.iter().any(|r| {
                matches!(
                    &r.kind,
                    DetectorRuleKind::Extension { values } if values.iter().any(|v| v == ext),
                )
            });
            advertises.then(|| (det.install_order, id.clone()))
        })
        .collect();
    if advertised.is_empty() {
        return None;
    }
    advertised.sort_by(|(a_ord, a_id), (b_ord, b_id)| a_ord.cmp(b_ord).then_with(|| a_id.cmp(b_id)));

    // extension_priority 표 적용 — 표에 적힌 detector 가 advertised 안에 있으면 그것이 먼저.
    if let Some(entry) = inner.extension_priority.get(ext) {
        for prio_id in &entry.order {
            if advertised.iter().any(|(_, id)| id == prio_id) {
                return Some(prio_id.clone());
            }
        }
    }
    // fallback: install_order 정렬의 첫 번째.
    advertised.into_iter().next().map(|(_, id)| id)
}

/// `[[extension_priority]]` 한 entry 등록. 같은 확장자 키에 last-writer-wins
/// (host → user 순서로 install 되므로 user 가 host 를 덮어쓰는 효과).
fn install_extension_priority(inner: &mut Inner, decl: ExtensionPriorityDecl, origin: RuleOrigin) {
    let key = decl.extension.trim_start_matches('.').to_ascii_lowercase();
    if key.is_empty() {
        warn!("file_format: extension_priority entry with empty extension — skipped");
        return;
    }
    if decl.order.is_empty() {
        // 빈 order 는 의미가 없음 — 제거 의도로 해석해 entry 삭제.
        inner.extension_priority.remove(&key);
        return;
    }
    let mut seen = std::collections::HashSet::new();
    let order: Vec<DetectorId> = decl
        .order
        .into_iter()
        .filter(|s| !s.is_empty() && seen.insert(s.clone()))
        .map(DetectorId)
        .collect();
    if order.is_empty() {
        return;
    }
    inner
        .extension_priority
        .insert(key, ExtensionPriorityEntry { order, origin });
}

fn decl_rule_to_kind(decl: DetectorRuleDecl) -> Option<DetectorRuleKind> {
    Some(match decl {
        DetectorRuleDecl::Extension { values } => DetectorRuleKind::Extension {
            values: values
                .into_iter()
                .map(|s| s.trim_start_matches('.').to_ascii_lowercase())
                .filter(|s| !s.is_empty())
                .collect(),
        },
        DetectorRuleDecl::PathGlob { pattern } => DetectorRuleKind::PathGlob { pattern },
        DetectorRuleDecl::Mime { types } => DetectorRuleKind::Mime { types },
        DetectorRuleDecl::Magic { offset, bytes_hex } => {
            let bytes = hex_to_bytes(&bytes_hex)?;
            DetectorRuleKind::Magic { offset, bytes }
        }
        DetectorRuleDecl::IsDirectory => DetectorRuleKind::IsDirectory,
        DetectorRuleDecl::Lua { script } => DetectorRuleKind::Lua { script },
        DetectorRuleDecl::StructureCheck { spec } => DetectorRuleKind::StructureCheck {
            spec_path: PathBuf::from(spec),
        },
        DetectorRuleDecl::Unknown { kind_name, raw } => DetectorRuleKind::Unknown {
            kind_name,
            raw,
        },
    })
}

/// `DetectorRuleKind` 을 TOML table 로 역직렬화. `parse_detector_section` 의 입력 형식과
/// 1:1 round-trip. `Unknown` 의 raw payload 는 그대로 보존.
fn rule_kind_to_toml(kind: &DetectorRuleKind) -> toml::value::Table {
    let mut t = toml::value::Table::new();
    match kind {
        DetectorRuleKind::Extension { values } => {
            t.insert("kind".into(), toml::Value::String("extension".into()));
            t.insert(
                "values".into(),
                toml::Value::Array(values.iter().cloned().map(toml::Value::String).collect()),
            );
        }
        DetectorRuleKind::PathGlob { pattern } => {
            t.insert("kind".into(), toml::Value::String("path_glob".into()));
            t.insert("pattern".into(), toml::Value::String(pattern.clone()));
        }
        DetectorRuleKind::Mime { types } => {
            t.insert("kind".into(), toml::Value::String("mime".into()));
            t.insert(
                "types".into(),
                toml::Value::Array(types.iter().cloned().map(toml::Value::String).collect()),
            );
        }
        DetectorRuleKind::Magic { offset, bytes } => {
            t.insert("kind".into(), toml::Value::String("magic".into()));
            t.insert("offset".into(), toml::Value::Integer(*offset as i64));
            let hex: String = bytes.iter().map(|b| format!("{:02x}", b)).collect();
            t.insert("bytes_hex".into(), toml::Value::String(hex));
        }
        DetectorRuleKind::IsDirectory => {
            t.insert("kind".into(), toml::Value::String("is_directory".into()));
        }
        DetectorRuleKind::Lua { script } => {
            t.insert("kind".into(), toml::Value::String("lua".into()));
            t.insert("script".into(), toml::Value::String(script.clone()));
        }
        DetectorRuleKind::StructureCheck { spec_path } => {
            t.insert("kind".into(), toml::Value::String("structure_check".into()));
            t.insert(
                "spec".into(),
                toml::Value::String(spec_path.to_string_lossy().into_owned()),
            );
        }
        DetectorRuleKind::Unknown { kind_name, raw } => {
            t.insert("kind".into(), toml::Value::String(kind_name.clone()));
            // raw 는 원래 table 통째였으나 manual parser 에서 모든 키를 보관했으므로
            // 그대로 평면 복사.
            if let toml::Value::Table(raw_t) = raw {
                for (k, v) in raw_t {
                    if k == "kind" {
                        continue;
                    }
                    t.insert(k.clone(), v.clone());
                }
            }
        }
    }
    t
}

fn hex_to_bytes(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn rule_kind_eq(a: &DetectorRuleKind, b: &DetectorRuleKind) -> bool {
    // Unknown 의 raw 비교는 toml::Value PartialEq 가 있어 가능.
    a == b
}

/// host default / user config 공통 표면: `[[detector]]` 섹션을 가진 TOML.
fn parse_detector_section(toml_text: &str) -> Result<Vec<DetectorDecl>, toml::de::Error> {
    #[derive(serde::Deserialize)]
    struct Wrap {
        #[serde(default, rename = "detector")]
        detectors: Vec<DetectorDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.detectors)
}

/// host default / user config: `[[extension_priority]]` 섹션. Phase E.
fn parse_extension_priority_section(
    toml_text: &str,
) -> Result<Vec<ExtensionPriorityDecl>, toml::de::Error> {
    #[derive(serde::Deserialize)]
    struct Wrap {
        #[serde(default)]
        extension_priority: Vec<ExtensionPriorityDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.extension_priority)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn target(p: &str) -> FileTarget {
        FileTarget::new(PathBuf::from(p))
    }

    #[test]
    fn host_default_loads_and_identifies_markdown() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let id = reg.identify(&target("a/b.md"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
        let id = reg.identify(&target("a/b.MARKDOWN"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
        let id = reg.identify(&target("a/b.html"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("html".into())));
        let id = reg.identify(&target("a/b.png"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("image".into())));
        let id = reg.identify(&target("a/b.unknownext"), DetectDepth::Cheap);
        assert_eq!(id, None);
    }

    #[test]
    fn plugin_extends_existing_detector() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // plugin 이 mdx 확장자 추가
        let decls = vec![DetectorDecl {
            id: "markdown".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Extension {
                values: vec!["mdx".into()],
            }],
        }];
        reg.install_plugin_detectors("com.example.mdx", &decls);
        let id = reg.identify(&target("a/b.mdx"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
        // 기존 md 매칭 유지
        let id = reg.identify(&target("a/b.md"), DetectDepth::Cheap);
        assert_eq!(id, Some(DetectorId("markdown".into())));
    }

    #[test]
    fn plugin_lua_rule_dropped_with_warn() {
        let reg = FileFormatRegistry::new();
        // plugin 이 Lua 와 Extension 을 섞어서 제공. Lua 만 drop 되고 Extension 은 유지.
        let decls = vec![DetectorDecl {
            id: "weird-fmt".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![
                DetectorRuleDecl::Lua {
                    script: "return true".into(),
                },
                DetectorRuleDecl::Extension {
                    values: vec!["wf".into()],
                },
            ],
        }];
        reg.install_plugin_detectors("com.example.weird", &decls);
        // Lua drop 후에도 Extension rule 이 살아 있어 매칭 가능.
        let id = reg.identify(&target("x.wf"), DetectDepth::Deep);
        assert_eq!(id, Some(DetectorId("weird-fmt".into())));
    }

    #[test]
    fn plugin_lua_only_detector_skipped() {
        let reg = FileFormatRegistry::new();
        // Lua 만 들어있는 detector — install 자체가 무의미해서 skip.
        let decls = vec![DetectorDecl {
            id: "lua-only".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Lua {
                script: "return true".into(),
            }],
        }];
        reg.install_plugin_detectors("com.example.lua-only", &decls);
        // detector 자체가 등록되지 않으므로 어떤 파일에도 안 잡힘.
        assert_eq!(
            reg.identify(&target("anything"), DetectDepth::Deep),
            None
        );
    }

    #[test]
    fn uninstall_plugin_removes_only_its_rules() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let decls = vec![DetectorDecl {
            id: "markdown".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Extension {
                values: vec!["mdx".into()],
            }],
        }];
        reg.install_plugin_detectors("com.example.mdx", &decls);
        assert_eq!(
            reg.identify(&target("a/b.mdx"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
        reg.uninstall_plugin("com.example.mdx");
        // 호스트의 md 는 유지
        assert_eq!(
            reg.identify(&target("a/b.md"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
        // plugin 의 mdx 는 사라짐
        assert_eq!(
            reg.identify(&target("a/b.mdx"), DetectDepth::Cheap),
            None
        );
    }

    #[test]
    fn directory_prefilter() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // tempfile 같은 디렉토리 만들기보다 root path 사용 — 디렉토리 매칭 동작만 확인.
        let dir = std::env::temp_dir();
        let t = FileTarget::new(dir);
        assert_eq!(
            reg.identify(&t, DetectDepth::Cheap),
            Some(DetectorId("$directory".into()))
        );
        // 파일 (확장자 없는 가짜 path) → IsDirectory 매칭 제외, 다른 detector 도 안 맞아 None
        let t = target("/nonexistent/file.no-such-ext");
        assert_eq!(reg.identify(&t, DetectDepth::Cheap), None);
    }

    #[test]
    fn identify_deep_matches_magic_when_cheap_misses() {
        let reg = FileFormatRegistry::new();
        // 호스트 default 는 사용 안 함 — 확장자가 없는 파일이 magic byte 로 매칭되는지 확인.
        // 사용자 정의 detector: extension 매칭 실패해도 magic 으로 매칭.
        let user_toml = r#"
            [[detector]]
            id = "png"
            [[detector.rule]]
            kind = "extension"
            values = ["png"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "89504E470D0A1A0A"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let cfg = dir.path().join("file-handlers.toml");
        std::fs::write(&cfg, user_toml).unwrap();
        reg.install_user_config(&cfg);

        // 확장자가 .dat 인 PNG 파일.
        let png_sig = [0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A];
        let img_path = dir.path().join("masquerade.dat");
        std::fs::write(&img_path, png_sig).unwrap();
        let t = FileTarget::new(img_path);

        // Cheap → 확장자 안 맞음, magic 평가 안 함 → None
        assert_eq!(reg.identify(&t, DetectDepth::Cheap), None);
        // Deep → magic 매칭 → Some("png")
        assert_eq!(
            reg.identify(&t, DetectDepth::Deep),
            Some(DetectorId("png".into()))
        );
    }

    #[test]
    fn reload_user_config_replaces_user_entries_keeps_host() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // 1차: 사용자가 pdf detector 추가.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert_eq!(
            reg.identify(&target("a/b.pdf"), DetectDepth::Cheap),
            Some(DetectorId("pdf".into()))
        );

        // 2차: 사용자가 pdf 를 빼고 csv 추가 → reload.
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "csv"
                [[detector.rule]]
                kind = "extension"
                values = ["csv"]
            "#,
        )
        .unwrap();
        reg.reload_user_config(&p);

        // pdf 는 host default 에 없으므로 (user 만) 사라져야 함.
        assert_eq!(reg.identify(&target("a/b.pdf"), DetectDepth::Cheap), None);
        // csv 는 새로 잡힘.
        assert_eq!(
            reg.identify(&target("a/b.csv"), DetectDepth::Cheap),
            Some(DetectorId("csv".into()))
        );
        // host default markdown 은 그대로.
        assert_eq!(
            reg.identify(&target("a/b.md"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
    }

    #[test]
    fn reload_user_config_missing_file_clears_user_entries() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_some());

        // 파일 삭제 후 reload → user origin 제거.
        std::fs::remove_file(&p).unwrap();
        reg.reload_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_none());
        // host markdown 은 보존.
        assert!(reg.detector(&DetectorId("markdown".into())).is_some());
    }

    #[test]
    fn reload_user_config_parse_error_keeps_previous_state() {
        let reg = FileFormatRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "pdf"
                [[detector.rule]]
                kind = "extension"
                values = ["pdf"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_some());

        // 파일을 의도적으로 깨뜨림 → reload 는 거부, 기존 user 항목 보존.
        std::fs::write(&p, "[[detector\n id = broken").unwrap();
        reg.reload_user_config(&p);
        assert!(reg.detector(&DetectorId("pdf".into())).is_some());
    }

    #[test]
    fn user_disabled_overrides_host() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // 사용자가 markdown detector 를 disable
        let user_toml = r#"
            [[detector]]
            id = "markdown"
            disabled = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);
        assert_eq!(reg.identify(&target("a/b.md"), DetectDepth::Cheap), None);
    }

    // ── export_user_config / save_user_config (MD4) ─────────────────────

    #[test]
    fn export_emits_user_only_origin() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // 사용자가 pdf 추가 + markdown disable.
        let user_toml = r#"
            [[detector]]
            id = "pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[detector]]
            id = "markdown"
            disabled = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exported = reg.export_user_config();
        // 호스트 detector 본문 (markdown 의 md 확장자 rule 등) 은 들어가면 안 됨.
        // 단 user 가 disable 한 markdown id 자체는 등장.
        assert!(exported.contains("pdf"), "exported = {exported}");
        assert!(exported.contains("markdown"));
        assert!(exported.contains("disabled = true"));
        // 호스트가 markdown 에 부여한 md 확장자는 user 가 만든 게 아니므로 미포함.
        // (확실히 하기 위해 user 가 등록한 pdf 의 'pdf' 확장자는 있어야).
        assert!(exported.contains("\"pdf\""));
    }

    #[test]
    fn export_round_trip_preserves_user_state() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let user_toml = r#"
            [[detector]]
            id = "pdf"
            display_name_i18n_key = "file_format.pdf"
            icon = "file-pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "255044462D"

            [[detector]]
            id = "markdown"
            disabled = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exported = reg.export_user_config();

        // 두 번째 registry 에 export 결과만 user origin 으로 로드.
        let reg2 = FileFormatRegistry::new();
        reg2.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let p2 = dir.path().join("export.toml");
        std::fs::write(&p2, &exported).unwrap();
        reg2.install_user_config(&p2);

        // identify 결과가 동일해야 함.
        // pdf 매칭 (extension)
        assert_eq!(
            reg.identify(&target("a/b.pdf"), DetectDepth::Cheap),
            reg2.identify(&target("a/b.pdf"), DetectDepth::Cheap),
        );
        // markdown 은 disabled — 둘 다 None
        assert_eq!(
            reg.identify(&target("a/b.md"), DetectDepth::Cheap),
            reg2.identify(&target("a/b.md"), DetectDepth::Cheap),
        );

        // 메타도 보존 — display_name / icon
        let pdf = reg2.detector(&DetectorId("pdf".into())).unwrap();
        assert_eq!(pdf.display_name_i18n_key.as_deref(), Some("file_format.pdf"));
        assert_eq!(pdf.icon.as_deref(), Some("file-pdf"));
    }

    #[test]
    fn export_preserves_unknown_rule_payload() {
        let reg = FileFormatRegistry::new();
        // forward-compat: 미지의 kind 도 round-trip 보존.
        let user_toml = r#"
            [[detector]]
            id = "futureproof"
            [[detector.rule]]
            kind = "ai_classify"
            model = "v2"
            confidence = 0.8
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exported = reg.export_user_config();
        assert!(exported.contains("ai_classify"));
        assert!(exported.contains("model"));
        assert!(exported.contains("\"v2\""));
        assert!(exported.contains("confidence"));
    }

    #[test]
    fn save_user_config_atomic_write() {
        let reg = FileFormatRegistry::new();
        let user_toml = r#"
            [[detector]]
            id = "pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml");
        std::fs::write(&src, user_toml).unwrap();
        reg.install_user_config(&src);

        let dst = dir.path().join("subdir").join("dst.toml");
        reg.save_user_config(&dst).unwrap();
        assert!(dst.exists());
        let written = std::fs::read_to_string(&dst).unwrap();
        assert!(written.contains("pdf"));
        assert!(written.contains("\"pdf\""));
    }

    #[test]
    fn export_empty_when_no_user_contributions() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        assert_eq!(reg.export_user_config(), "");
    }

    // ── DetectorInfo trait (Phase E ME1) ───────────────────────────────

    #[test]
    fn advertised_extensions_returns_only_extension_rule_values() {
        let reg = FileFormatRegistry::new();
        // 같은 detector 가 extension + magic 둘 다 가짐. trait 은 extension 만 반환.
        let user_toml = r#"
            [[detector]]
            id = "png"
            [[detector.rule]]
            kind = "extension"
            values = ["png", "PNG"]
            [[detector.rule]]
            kind = "magic"
            offset = 0
            bytes_hex = "89504E47"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exts = reg.advertised_extensions(&DetectorId("png".into()));
        // values 는 소문자 정규화됨 → 둘 다 "png" → dedup 결과 1개.
        assert_eq!(exts, vec!["png".to_string()]);

        // 없는 detector 는 빈 벡터.
        assert!(reg.advertised_extensions(&DetectorId("nope".into())).is_empty());
    }

    #[test]
    fn detectors_for_extension_orders_by_install_order() {
        let reg = FileFormatRegistry::new();
        // 1번째 install: "zzz" id (알파벳 후순) 이 먼저 들어옴 → install_order=0.
        let user_toml_a = r#"
            [[detector]]
            id = "zzz"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p1 = dir.path().join("a.toml");
        std::fs::write(&p1, user_toml_a).unwrap();
        reg.install_user_config(&p1);

        // 2번째 install (다른 origin — plugin): "aaa" id 가 같은 .md 광고. install_order=1.
        let decls = vec![DetectorDecl {
            id: "aaa".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Extension {
                values: vec!["md".into()],
            }],
        }];
        reg.install_plugin_detectors("com.example.aaa", &decls);

        let hits = reg.detectors_for_extension("md");
        // install_order 가 작은 zzz 가 먼저, 그 다음 aaa. (알파벳 정렬이 아님)
        assert_eq!(
            hits,
            vec![DetectorId("zzz".into()), DetectorId("aaa".into())]
        );
    }

    #[test]
    fn detectors_for_extension_skips_disabled() {
        let reg = FileFormatRegistry::new();
        let user_toml = r#"
            [[detector]]
            id = "x"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "y"
            disabled = true
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let hits = reg.detectors_for_extension("md");
        assert_eq!(hits, vec![DetectorId("x".into())]);
    }

    #[test]
    fn detectors_for_extension_accepts_leading_dot_and_uppercase() {
        let reg = FileFormatRegistry::new();
        let user_toml = r#"
            [[detector]]
            id = "x"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        // 점 prefix / 대문자 입력 모두 정규화 매칭.
        assert_eq!(reg.detectors_for_extension(".md"), vec![DetectorId("x".into())]);
        assert_eq!(reg.detectors_for_extension("MD"), vec![DetectorId("x".into())]);
        // 빈 문자열 / 점만 → 빈 결과.
        assert!(reg.detectors_for_extension("").is_empty());
        assert!(reg.detectors_for_extension(".").is_empty());
    }

    #[test]
    fn all_advertised_extensions_dedupes_and_sorts() {
        let reg = FileFormatRegistry::new();
        let user_toml = r#"
            [[detector]]
            id = "a"
            [[detector.rule]]
            kind = "extension"
            values = ["md", "markdown"]

            [[detector]]
            id = "b"
            [[detector.rule]]
            kind = "extension"
            values = ["mdx", "md"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exts = reg.all_advertised_extensions();
        // 알파벳 정렬, dedup.
        assert_eq!(
            exts,
            vec!["markdown".to_string(), "md".to_string(), "mdx".to_string()],
        );
    }

    #[test]
    fn is_enabled_reflects_disabled_field() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // host 의 markdown 은 enabled.
        assert!(reg.is_enabled(&DetectorId("markdown".into())));
        // 존재하지 않는 detector 는 false.
        assert!(!reg.is_enabled(&DetectorId("nope".into())));

        // user 가 disable 하면 false.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "markdown"
                disabled = true
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(!reg.is_enabled(&DetectorId("markdown".into())));
    }

    // ── ExtensionPriority parse/export (Phase E ME2) ───────────────────

    #[test]
    fn extension_priority_user_config_parsed_and_queryable() {
        let reg = FileFormatRegistry::new();
        let user_toml = r#"
            [[detector]]
            id = "x"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "y"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["y", "x"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let order = reg.extension_priority_order("md").expect("present");
        assert_eq!(order, vec![DetectorId("y".into()), DetectorId("x".into())]);
        // 점 prefix, 대문자 정규화도 동일 결과.
        assert_eq!(reg.extension_priority_order(".MD"), Some(order));
        // 미정의 확장자는 None.
        assert!(reg.extension_priority_order("zzz").is_none());
    }

    #[test]
    fn extension_priority_user_overrides_host() {
        let reg = FileFormatRegistry::new();
        // host default 가 .md 에 ["host-md"] 우선순위 적용.
        let host_toml = r#"
            [[detector]]
            id = "host-md"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["host-md"]
        "#;
        reg.install_host_defaults(host_toml);
        assert_eq!(
            reg.extension_priority_order("md"),
            Some(vec![DetectorId("host-md".into())])
        );

        // user 가 같은 키 덮어쓰기 — last-writer-wins.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["user-md"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert_eq!(
            reg.extension_priority_order("md"),
            Some(vec![DetectorId("user-md".into())])
        );
    }

    #[test]
    fn extension_priority_empty_order_removes_entry() {
        let reg = FileFormatRegistry::new();
        // host 가 priority 설치.
        reg.install_host_defaults(
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["host-md"]
            "#,
        );
        assert!(reg.extension_priority_order("md").is_some());

        // user 가 빈 order 로 명시 → 제거 의도로 entry 삭제.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "md"
                order = []
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg.extension_priority_order("md").is_none());
    }

    #[test]
    fn extension_priority_exported_only_user_origin() {
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["host-md"]
            "#,
        );
        // host-only — export 비어야 함.
        assert_eq!(reg.export_user_config(), "");

        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "json"
                order = ["json-strict", "json"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);

        let exported = reg.export_user_config();
        // user 가 적은 json 우선순위만 emit.
        assert!(exported.contains("extension_priority"), "got: {exported}");
        assert!(exported.contains("\"json\""));
        assert!(exported.contains("json-strict"));
        // host 의 md 우선순위는 emit 되지 않아야.
        assert!(!exported.contains("host-md"), "got: {exported}");
    }

    #[test]
    fn extension_priority_round_trip_through_export() {
        let reg = FileFormatRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["mdx-strict", "markdown"]

                [[extension_priority]]
                extension = "json"
                order = ["jsonc"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        let exported = reg.export_user_config();

        let reg2 = FileFormatRegistry::new();
        let p2 = dir.path().join("export.toml");
        std::fs::write(&p2, &exported).unwrap();
        reg2.install_user_config(&p2);

        assert_eq!(
            reg2.extension_priority_order("md"),
            Some(vec![
                DetectorId("mdx-strict".into()),
                DetectorId("markdown".into())
            ])
        );
        assert_eq!(
            reg2.extension_priority_order("json"),
            Some(vec![DetectorId("jsonc".into())])
        );
    }

    #[test]
    fn extension_priority_reload_clears_old_user_entries() {
        let reg = FileFormatRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");

        // 1차: md + json.
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["mdx"]

                [[extension_priority]]
                extension = "json"
                order = ["json"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg.extension_priority_order("md").is_some());
        assert!(reg.extension_priority_order("json").is_some());

        // 2차: md 만 (json 제거) → reload.
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["mdx"]
            "#,
        )
        .unwrap();
        reg.reload_user_config(&p);
        assert!(reg.extension_priority_order("md").is_some());
        assert!(
            reg.extension_priority_order("json").is_none(),
            "user reload should drop previous json entry",
        );
    }

    #[test]
    fn extension_priority_dedupes_duplicate_ids_in_order() {
        let reg = FileFormatRegistry::new();
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[extension_priority]]
                extension = "md"
                order = ["a", "b", "a", "b", "c"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);

        let order = reg.extension_priority_order("md").unwrap();
        assert_eq!(
            order,
            vec![
                DetectorId("a".into()),
                DetectorId("b".into()),
                DetectorId("c".into())
            ]
        );
    }

    // ── identify cheap path cutover (Phase E ME3) ──────────────────────

    #[test]
    fn identify_uses_extension_priority_table() {
        let reg = FileFormatRegistry::new();
        // 두 detector 가 .md 광고. priority 표가 "b" 우선이라 b 가 이김.
        let user_toml = r#"
            [[detector]]
            id = "a"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "b"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["b", "a"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let got = reg.identify(&target("hello.md"), DetectDepth::Cheap);
        assert_eq!(got, Some(DetectorId("b".into())));
    }

    #[test]
    fn identify_falls_back_to_install_order_without_priority_table() {
        let reg = FileFormatRegistry::new();
        // a 가 먼저 install 됨 → install_order 0. b 가 두번째 → 1.
        let user_toml = r#"
            [[detector]]
            id = "z"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        // 두번째 plugin install — 알파벳상 더 앞이지만 install_order 가 더 큼.
        let decls = vec![DetectorDecl {
            id: "a".into(),
            display_name_i18n_key: None,
            icon: None,
            disabled: false,
            rule: vec![DetectorRuleDecl::Extension {
                values: vec!["md".into()],
            }],
        }];
        reg.install_plugin_detectors("com.example.a", &decls);

        // priority 표 없음 → install_order 우선 → "z" 가 이김 (알파벳 아닌 install 순).
        let got = reg.identify(&target("hello.md"), DetectDepth::Cheap);
        assert_eq!(got, Some(DetectorId("z".into())));
    }

    #[test]
    fn identify_priority_entry_with_unknown_id_skips_to_next() {
        let reg = FileFormatRegistry::new();
        // priority 표가 미설치 detector "ghost" 를 1순위로 적었지만 그건 무시되고
        // advertised 후보 중 install_order 첫 번째인 "real" 이 이김.
        let user_toml = r#"
            [[detector]]
            id = "real"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["ghost", "real"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let got = reg.identify(&target("a.md"), DetectDepth::Cheap);
        assert_eq!(got, Some(DetectorId("real".into())));
    }

    #[test]
    fn identify_fast_path_skips_disabled_detectors() {
        let reg = FileFormatRegistry::new();
        let user_toml = r#"
            [[detector]]
            id = "off"
            disabled = true
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[detector]]
            id = "on"
            [[detector.rule]]
            kind = "extension"
            values = ["md"]

            [[extension_priority]]
            extension = "md"
            order = ["off", "on"]
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        // priority 1순위가 disabled → 2순위 on 이 이김.
        let got = reg.identify(&target("a.md"), DetectDepth::Cheap);
        assert_eq!(got, Some(DetectorId("on".into())));
    }

    #[test]
    fn identify_fast_path_does_not_apply_to_directory_target() {
        let reg = FileFormatRegistry::new();
        // 호스트 default 의 $directory 가 디렉토리에 매칭되어야 함 (fast path 가 디렉토리에는
        // 적용되지 않음을 확인).
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        // 사용자가 .tmp 확장자를 가진 detector 등록.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "junk"
                [[detector.rule]]
                kind = "extension"
                values = ["tmp"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);

        // 디렉토리 path 가 ".tmp" 로 끝나도 IsDirectory pre-filter 가 우선 — $directory 가 이김.
        let tmp_dir = dir.path().join("scratch.tmp");
        std::fs::create_dir_all(&tmp_dir).unwrap();
        let got = reg.identify(&FileTarget::new(tmp_dir), DetectDepth::Cheap);
        assert_eq!(got, Some(DetectorId("$directory".into())));
    }

    #[test]
    fn identify_existing_tests_still_pass_after_cutover() {
        // 빠른 회귀 — 기존의 단순 매칭 (host markdown / image) 이 깨지지 않음을 확인.
        let reg = FileFormatRegistry::new();
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        assert_eq!(
            reg.identify(&target("a/b.md"), DetectDepth::Cheap),
            Some(DetectorId("markdown".into()))
        );
        assert_eq!(
            reg.identify(&target("a/b.png"), DetectDepth::Cheap),
            Some(DetectorId("image".into()))
        );
    }

    #[test]
    fn install_order_persists_across_patch_from_other_origin() {
        let reg = FileFormatRegistry::new();
        // 1번째: host default 로 markdown install (install_order=0).
        reg.install_host_defaults(
            include_str!("defaults/default-file-format.toml"),
        );
        let initial = reg
            .detector(&DetectorId("markdown".into()))
            .unwrap()
            .install_order;
        // 2번째: 사용자가 같은 id 에 mdx 추가 → patch. install_order 변하지 않아야 함.
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("u.toml");
        std::fs::write(
            &p,
            r#"
                [[detector]]
                id = "markdown"
                [[detector.rule]]
                kind = "extension"
                values = ["mdx"]
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        let after = reg
            .detector(&DetectorId("markdown".into()))
            .unwrap()
            .install_order;
        assert_eq!(initial, after);
    }
}
