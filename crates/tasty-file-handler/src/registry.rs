//! 핸들러를 detector별로 조회한다. 출처별 선언을 보관해 플러그인 제거 시 해당 선언만 지운다.
//! 같은 ID는 Host → Plugin → User 순으로 명시된 필드를 덮어쓴다.

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::Deserialize;
use tracing::warn;

use tasty_file_format::{DetectorId, DetectorInfo};

use super::config::{
    HandlerDecl, HandlerDeclError, HostHandlerActionDecl, PluginHandlerActionDecl,
    UserHandlerActionDecl, validate_plugin_handler_decl,
};
use super::types::{
    FileHandler, HandlerAction, HandlerId, HandlerOwner, is_valid_handler_short_name,
};

#[derive(Debug, Clone)]
struct HandlerContribution {
    owner: HandlerOwner,
    detector: Option<String>,
    priority: Option<i32>,
    display_name_i18n_key: Option<String>,
    disabled_override: Option<bool>,
    action: Option<HandlerAction>,
}

struct Inner {
    /// handler id → 출처별 contribution. install 순서 보존 (host → plugin → user).
    contributions: BTreeMap<HandlerId, Vec<HandlerContribution>>,
    finalized: BTreeMap<HandlerId, FileHandler>,
    dirty: bool,
}

pub struct FileHandlerRegistry {
    inner: RwLock<Inner>,
    /// poison 을 이미 보고했는가 — 로그 폭주 방지용 1 회 게이트. `inner` 와
    /// `detector_info` 가 공유한다(둘 다 같은 사고의 결과다).
    poison_reported: std::sync::atomic::AtomicBool,
    /// `DetectorInfo` 주입 슬롯. 부팅 시 `attach_detector_info` 1회 호출.
    /// Settings UI 의 Extension Mapping 탭 등이 detector 의 확장자 광고를 조회할 때 사용.
    detector_info: RwLock<Option<Arc<dyn DetectorInfo>>>,
}

impl FileHandlerRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                finalized: BTreeMap::new(),
                dirty: false,
            }),
            detector_info: RwLock::new(None),
            poison_reported: std::sync::atomic::AtomicBool::new(false),
        }
    }

    /// poison을 보고하고 읽기 락을 복구한다. 임계구역은 메모리 자료구조만 변경한다.
    fn lock_read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        tasty_utils::poison::recover_read(
            self.inner.read(),
            "file handler registry",
            &self.poison_reported,
        )
    }

    /// Poison 을 복구해 write guard 를 잡는다. 근거는 [`Self::lock_read`] 와 같다.
    fn lock_write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        tasty_utils::poison::recover_write(
            self.inner.write(),
            "file handler registry",
            &self.poison_reported,
        )
    }

    /// `DetectorInfo` 주입. 부팅 시 1회만. 중복 호출은 warn + 무시.
    pub fn attach_detector_info(&self, info: Arc<dyn DetectorInfo>) {
        let mut slot = tasty_utils::poison::recover_write(
            self.detector_info.write(),
            "file handler detector_info",
            &self.poison_reported,
        );
        if slot.is_some() {
            warn!("FileHandlerRegistry: detector_info already attached, ignoring");
            return;
        }
        *slot = Some(info);
    }

    /// 주입된 `DetectorInfo` 의 clone. `attach_detector_info` 호출 전이면 `None`.
    pub fn detector_info(&self) -> Option<Arc<dyn DetectorInfo>> {
        tasty_utils::poison::recover_read(
            self.detector_info.read(),
            "file handler detector_info",
            &self.poison_reported,
        )
        .clone()
    }

    pub fn handler(&self, id: &HandlerId) -> Option<FileHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.get(id).cloned()
    }

    pub fn list_handlers(&self) -> Vec<HandlerId> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.keys().cloned().collect()
    }

    /// `detector` 에 attach 된 활성 handler 들. priority 오름차순, tie 시 owner 우선
    /// `user > plugin > host`, 그 후 handler id alphabetical.
    pub fn handlers_for(&self, detector: &DetectorId) -> Vec<FileHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let mut v: Vec<FileHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled && &h.detector == detector)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    /// id 로 단건 lookup. picker 가 선택 결과(`HandlerId`) 를 dispatch 할 때 사용.
    pub fn get(&self, id: &HandlerId) -> Option<FileHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.get(id).cloned()
    }

    /// Picker modal 용 — 모든 enabled handler.
    pub fn all_handlers(&self) -> Vec<FileHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let mut v: Vec<FileHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_host_handler_section(toml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "file_handler: failed to parse host defaults");
                return;
            }
        };
        let mut inner = self.lock_write();
        for decl in decls {
            install_host(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_user_config(&self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_handler: user config read failed");
                return;
            }
        };
        let decls = match parse_user_handler_section(&text) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "file_handler: user config parse failed");
                return;
            }
        };
        let mut inner = self.lock_write();
        // 부팅 로드는 적용되지 않은 항목을 돌려줄 호출자가 없다 — 접두사 누락은 install_user 가,
        // detector·action 누락은 finalize 가 각자 warn 으로 남긴다.
        install_user_decls(&mut inner, decls);
        inner.dirty = true;
    }

    pub fn install_plugin_handlers(
        &self,
        plugin_id: &str,
        decls: &[HandlerDecl<PluginHandlerActionDecl>],
    ) {
        let mut inner = self.lock_write();
        for decl in decls {
            install_plugin(&mut inner, plugin_id, decl.clone());
        }
        inner.dirty = true;
    }

    /// user 출처 contribution 만 모아 TOML 문자열로 직렬화. Settings UI 변경 저장에 사용.
    ///
    /// 각 contribution 의 모든 optional 필드 중 `Some(_)` 만 emit (patch semantics 유지).
    /// id 는 contribution 의 전역 id 그대로 (host/plugin/user 어느 owner 의 base contribution
    /// 을 patch 하든 그 id 를 유지). 빈 결과는 빈 문자열.
    pub fn export_user_config(&self) -> String {
        let inner = self.lock_read();
        let mut handlers = Vec::<toml::Value>::new();
        for (id, contribs) in inner.contributions.iter() {
            let user = contribs
                .iter()
                .find(|c| matches!(c.owner, HandlerOwner::User));
            let Some(user) = user else { continue };
            // 적어도 한 필드가 Some 이어야 user TOML 에 의미 있는 entry 가 됨.
            if user.detector.is_none()
                && user.priority.is_none()
                && user.display_name_i18n_key.is_none()
                && user.disabled_override.is_none()
                && user.action.is_none()
            {
                continue;
            }
            let mut t = toml::value::Table::new();
            t.insert("id".into(), toml::Value::String(id.as_str().to_string()));
            if let Some(det) = &user.detector {
                t.insert("detector".into(), toml::Value::String(det.clone()));
            }
            if let Some(p) = user.priority {
                t.insert("priority".into(), toml::Value::Integer(p as i64));
            }
            if let Some(k) = &user.display_name_i18n_key {
                t.insert(
                    "display_name_i18n_key".into(),
                    toml::Value::String(k.clone()),
                );
            }
            if let Some(d) = user.disabled_override {
                t.insert("disabled".into(), toml::Value::Boolean(d));
            }
            if let Some(action) = &user.action {
                t.insert(
                    "action".into(),
                    toml::Value::Table(handler_action_to_toml(action)),
                );
            }
            handlers.push(toml::Value::Table(t));
        }
        if handlers.is_empty() {
            return String::new();
        }
        let mut doc = toml::value::Table::new();
        doc.insert("handler".into(), toml::Value::Array(handlers));
        toml::to_string(&doc).unwrap_or_default()
    }

    /// `export_user_config` 결과를 `path` 에 atomic write.
    pub fn save_user_config(&self, path: &std::path::Path) -> std::io::Result<()> {
        use std::io::Write;
        let text = self.export_user_config();
        let parent = path.parent().unwrap_or_else(|| std::path::Path::new("."));
        if !parent.exists() {
            std::fs::create_dir_all(parent)?;
        }
        let mut tmp = tempfile::NamedTempFile::new_in(parent)?;
        tmp.write_all(text.as_bytes())?;
        tmp.flush()?;
        tmp.persist(path).map_err(|e| e.error)?;
        Ok(())
    }

    /// Settings UI 가 host/plugin handler 를 user-origin override 로 disable/enable.
    /// 명시적 user 의도 → 항상 `disabled_override = Some(value)`.
    pub fn set_user_handler_disabled(&self, id: &HandlerId, disabled: bool) {
        let mut inner = self.lock_write();
        let Some(entry) = inner.contributions.get_mut(id) else {
            warn!(
                handler = id.as_str(),
                "file_handler: set_user_handler_disabled — unknown handler"
            );
            return;
        };
        if let Some(existing) = entry
            .iter_mut()
            .find(|c| matches!(c.owner, HandlerOwner::User))
        {
            existing.disabled_override = Some(disabled);
        } else {
            entry.push(HandlerContribution {
                owner: HandlerOwner::User,
                detector: None,
                priority: None,
                display_name_i18n_key: None,
                disabled_override: Some(disabled),
                action: None,
            });
        }
        inner.dirty = true;
    }

    /// User-origin contribution 의 `disabled_override` 만 None 으로 비운다. 다른 user 필드
    /// (priority/action/detector 등) 는 보존.
    pub fn clear_user_handler_override(&self, id: &HandlerId) {
        let mut inner = self.lock_write();
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        let mut user_empty = false;
        if let Some(existing) = entry
            .iter_mut()
            .find(|c| matches!(c.owner, HandlerOwner::User))
        {
            existing.disabled_override = None;
            user_empty = existing.detector.is_none()
                && existing.priority.is_none()
                && existing.display_name_i18n_key.is_none()
                && existing.action.is_none();
        }
        if user_empty {
            entry.retain(|c| !matches!(c.owner, HandlerOwner::User));
            if entry.is_empty() {
                inner.contributions.remove(id);
            }
        }
        inner.dirty = true;
    }

    /// Settings UI 가 user-origin contribution 전체를 제거. host/plugin 은 보존.
    pub fn remove_user_handler(&self, id: &HandlerId) {
        let mut inner = self.lock_write();
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        entry.retain(|c| !matches!(c.owner, HandlerOwner::User));
        if entry.is_empty() {
            inner.contributions.remove(id);
        }
        inner.dirty = true;
    }

    /// Settings UI 가 user-origin handler 를 추가/갱신. 기존 host/plugin 이 있으면 patch.
    /// id 가 `<owner>/<short>` 형식이어야 하고, action 의 surface_kind/method 등은 호출자가
    /// 검증한 상태로 넘긴다 (UI 입력 단계에서 후보 dropdown 으로 강제).
    pub fn upsert_user_handler(&self, decl: UserHandlerUpsertDecl) -> Result<(), HandlerDeclError> {
        if !decl.id.contains('/') {
            return Err(HandlerDeclError::InvalidShortName(decl.id.clone()));
        }
        let mut inner = self.lock_write();
        push_contribution(
            &mut inner,
            HandlerId(decl.id),
            HandlerContribution {
                owner: HandlerOwner::User,
                detector: decl.detector,
                priority: decl.priority,
                display_name_i18n_key: decl.display_name_i18n_key,
                disabled_override: decl.disabled,
                action: decl.action.map(Into::into),
            },
        );
        inner.dirty = true;
        Ok(())
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let mut inner = self.lock_write();
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(&c.owner, HandlerOwner::Plugin(p) if p == plugin_id));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
        }
        inner.dirty = true;
    }

    /// 사용자 설정 파일을 다시 읽어 user owner contribution 만 교체. host + plugin 은 그대로.
    ///
    /// **Transactional**: read/parse 실패 시 기존 user contribution 보존 (write lock 잡기 전에
    /// 검증). 파일이 없으면 user contribution 만 제거.
    ///
    /// 돌려주는 것은 **이 reload 가 적용하지 않은 user 항목**이다 — 경고 로그와 같은 사실을 호출자가
    /// 응답에 실을 수 있게 한다(docs/features/file-handler/index.md#인터페이스).
    /// read/parse 실패로 reload 자체가 멈춘 경우는 항목을 모르므로 빈 목록이다.
    pub fn reload_user_config(&self, path: &std::path::Path) -> Vec<RejectedUserHandler> {
        let Some(decls) = Self::load_user_handler_decls(path) else {
            return Vec::new();
        };
        let mut inner = self.lock_write();
        Self::purge_user_owned(&mut inner);
        let rejected = install_user_decls(&mut inner, decls);
        inner.dirty = true;
        rejected
    }

    /// user config 파일을 읽어 handler 선언을 파싱한다. 파일이 없으면 빈 목록(=
    /// user override 전부 해제), 읽기/파싱 실패는 이미 warn 로그 후 `None`(호출자는
    /// 기존 user config 를 그대로 유지).
    fn load_user_handler_decls(path: &std::path::Path) -> Option<Vec<UserHandlerSettingsDecl>> {
        match std::fs::read_to_string(path) {
            Ok(text) => match parse_user_handler_section(&text) {
                Ok(v) => Some(v),
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "file_handler: reload aborted — parse failed, keeping previous user config",
                    );
                    None
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "file_handler: reload aborted — read failed, keeping previous user config",
                );
                None
            }
        }
    }

    /// 기존 user-owned contribution 을 모두 제거한다 (reload 는 항상 전체
    /// 재설치이므로 재설치 전 정리).
    fn purge_user_owned(inner: &mut Inner) {
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(c.owner, HandlerOwner::User));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
        }
    }

    fn ensure_finalized(&self) {
        let needs = self.lock_read().dirty;
        if !needs {
            return;
        }
        let mut inner = self.lock_write();
        if !inner.dirty {
            return;
        }
        let mut next = BTreeMap::new();
        for (id, contribs) in inner.contributions.iter() {
            let contribs = merge_order(contribs);
            // 1번째 contribution 이 base — detector / action 필수
            let Some(base) = contribs.first() else {
                continue;
            };
            let mut detector = base.detector.clone();
            let mut priority = base.priority;
            let mut display = base.display_name_i18n_key.clone();
            let mut disabled = base.disabled_override.unwrap_or(false);
            let mut action = base.action.clone();
            let mut owner = base.owner.clone();

            for c in contribs.iter().skip(1) {
                if c.detector.is_some() {
                    detector = c.detector.clone();
                }
                if c.priority.is_some() {
                    priority = c.priority;
                }
                if c.display_name_i18n_key.is_some() {
                    display = c.display_name_i18n_key.clone();
                }
                if let Some(d) = c.disabled_override {
                    disabled = d;
                }
                if c.action.is_some() {
                    action = c.action.clone();
                }
                // owner: 마지막 출처가 이김 (e.g. user override 시 user)
                owner = c.owner.clone();
            }

            // detector + action 둘 다 있어야 등록. reload 가 보고하는 미등록(`is_complete`)과
            // 같은 판정이다 — 마지막 non-None 이 이기므로 "어느 출처든 하나라도 있는가" 와 같다.
            let (Some(detector), Some(action)) = (detector, action) else {
                warn!(
                    handler_id = id.as_str(),
                    "file_handler: handler missing required detector or action — dropped"
                );
                continue;
            };
            let priority = priority.unwrap_or(100);
            next.insert(
                id.clone(),
                FileHandler {
                    id: id.clone(),
                    detector: DetectorId(detector),
                    priority,
                    owner,
                    action,
                    display_name_i18n_key: display,
                    disabled,
                },
            );
        }
        inner.finalized = next;
        inner.dirty = false;
    }
}

impl Default for FileHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl tasty_plugin_protocol::host_port::FileHandlerRegistryPort for FileHandlerRegistry {
    fn install_plugin_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]) {
        let mut decls: Vec<HandlerDecl<PluginHandlerActionDecl>> =
            Vec::with_capacity(handlers.len());
        for v in handlers {
            match serde_json::from_value::<HandlerDecl<PluginHandlerActionDecl>>(v.clone()) {
                Ok(d) => decls.push(d),
                Err(e) => warn!(
                    plugin = plugin_id,
                    error = %e,
                    "plugin handler decode failed"
                ),
            }
        }
        FileHandlerRegistry::install_plugin_handlers(self, plugin_id, &decls);
    }

    fn uninstall_plugin(&self, plugin_id: &str) {
        FileHandlerRegistry::uninstall_plugin(self, plugin_id);
    }
}

fn sort_handlers(v: &mut [FileHandler]) {
    v.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| owner_rank(&a.owner).cmp(&owner_rank(&b.owner)))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// Host → Plugin → User 순으로 병합하고 같은 출처 안에서는 설치 순서를 유지한다.
/// 플러그인이 늦게 등록돼도 사용자 설정을 덮지 않도록 user를 마지막에 적용한다.
fn merge_order(contribs: &[HandlerContribution]) -> Vec<&HandlerContribution> {
    let mut ordered: Vec<&HandlerContribution> = contribs.iter().collect();
    ordered.sort_by_key(|c| std::cmp::Reverse(owner_rank(&c.owner)));
    ordered
}

/// tie-break 시 owner 우선순위 — 작을수록 우선. `user > plugin > host`.
fn owner_rank(owner: &HandlerOwner) -> u8 {
    match owner {
        HandlerOwner::User => 0,
        HandlerOwner::Plugin(_) => 1,
        HandlerOwner::Host => 2,
    }
}

fn install_host(inner: &mut Inner, decl: HandlerDecl<HostHandlerActionDecl>) {
    let id_str = format!("host/{}", decl.id);
    if !is_valid_handler_short_name(&decl.id) {
        warn!(
            short_name = decl.id.as_str(),
            "file_handler: invalid host handler short-name"
        );
        return;
    }
    push_contribution(
        inner,
        HandlerId(id_str),
        HandlerContribution {
            owner: HandlerOwner::Host,
            detector: Some(decl.detector),
            priority: Some(decl.priority),
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: if decl.disabled { Some(true) } else { None },
            action: Some(decl.action.into()),
        },
    );
}

fn install_plugin(inner: &mut Inner, plugin_id: &str, decl: HandlerDecl<PluginHandlerActionDecl>) {
    if let Err(e) = validate_plugin_handler_decl(&decl) {
        warn!(plugin = plugin_id, error = %e, "file_handler: rejecting plugin handler decl");
        return;
    }
    let id_str = format!("{}/{}", plugin_id, decl.id);
    let action = decl.action.into_handler_action(plugin_id);
    push_contribution(
        inner,
        HandlerId(id_str),
        HandlerContribution {
            owner: HandlerOwner::Plugin(plugin_id.to_string()),
            detector: Some(decl.detector),
            priority: Some(decl.priority),
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: if decl.disabled { Some(true) } else { None },
            action: Some(action),
        },
    );
    let _: Option<HandlerDeclError> = None; // suppress unused import if all paths Ok
}

/// user 설정 reload 가 **적용하지 않은** 항목 하나 — 그 id 와 사유. 버려진 것도 있고
/// (`MissingOwnerPrefix` · `MissingDetectorOrAction`), 남아 있다가 대상이 나타나면 적용되는 것도
/// 있다(`TargetNotContributed`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RejectedUserHandler {
    /// user TOML 에 적힌 그대로의 id.
    pub id: String,
    pub reason: UserHandlerRejectReason,
}

/// user 항목이 적용되지 않은 사유. 응답에는 [`UserHandlerRejectReason::as_str`] 의 코드로 나간다.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UserHandlerRejectReason {
    /// id 에 `<owner>/` 접두사가 없다 — 설치하지 않는다.
    MissingOwnerPrefix,
    /// user 가 만든 항목(`user/…`)인데 detector 나 action 이 비어 finalize 가 버린다.
    MissingDetectorOrAction,
    /// 다른 출처(host · plugin)의 handler 를 patch 하는 항목인데 그 대상이 지금 registry 에
    /// 없다 — plugin 이 안 떠 있거나 id 가 틀렸다. 둘은 여기서 가를 수 없다. 항목은
    /// 버려지지 않고 남아 있어, 대상이 contribute 되면 그대로 적용된다.
    TargetNotContributed,
}

impl UserHandlerRejectReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::MissingOwnerPrefix => "missing_owner_prefix",
            Self::MissingDetectorOrAction => "missing_detector_or_action",
            Self::TargetNotContributed => "target_not_contributed",
        }
    }
}

/// 설치 전에 거절한 항목과 설치 후 필요한 필드가 없어 적용되지 않을 항목을 보고한다.
/// finalize와 같은 is_complete 판정을 사용해 실제 등록 결과와 맞춘다.
fn install_user_decls(
    inner: &mut Inner,
    decls: Vec<UserHandlerSettingsDecl>,
) -> Vec<RejectedUserHandler> {
    let mut rejected = Vec::new();
    let mut installed = Vec::new();
    for decl in decls {
        match install_user(inner, decl) {
            Ok(id) => installed.push(id),
            Err(r) => rejected.push(r),
        }
    }
    for id in installed {
        let reason = inner
            .contributions
            .get(&id)
            .and_then(|contribs| incomplete_reason(&id, contribs));
        // 같은 id 가 파일에 두 번 적혔으면 한 번만 보고한다.
        if let Some(reason) = reason
            && !rejected.iter().any(|r| r.id == id.0)
        {
            rejected.push(RejectedUserHandler { id: id.0, reason });
        }
    }
    rejected
}

/// finalize 가 이 id 를 등록하지 않는다면 그 사유. 등록하면 `None`.
///
/// `user/` 가 아닌 id 에 user contribution 만 있으면 patch 대상이 아직 없는 것이다 — 항목은
/// 남아 있다가 대상이 contribute 되면 적용되므로 "버렸다" 가 아니다
/// (docs/features/file-handler/index.md#인터페이스).
fn incomplete_reason(
    id: &HandlerId,
    contribs: &[HandlerContribution],
) -> Option<UserHandlerRejectReason> {
    if is_complete(contribs) {
        return None;
    }
    let patches_another_owner = !id.as_str().starts_with("user/");
    // host/plugin 선언은 detector와 action을 갖는다. 이후 그 전제가 바뀌어도
    // 실제 대상이 있는 patch를 대상 부재로 오인하지 않도록 출처를 확인한다.
    let only_user = contribs
        .iter()
        .all(|c| matches!(c.owner, HandlerOwner::User));
    if patches_another_owner && only_user {
        Some(UserHandlerRejectReason::TargetNotContributed)
    } else {
        Some(UserHandlerRejectReason::MissingDetectorOrAction)
    }
}

/// finalize 가 이 id 를 등록하는가 — 어느 출처든 detector 와 action 을 하나씩 가졌는가.
fn is_complete(contribs: &[HandlerContribution]) -> bool {
    contribs.iter().any(|c| c.detector.is_some()) && contribs.iter().any(|c| c.action.is_some())
}

fn install_user(
    inner: &mut Inner,
    decl: UserHandlerSettingsDecl,
) -> Result<HandlerId, RejectedUserHandler> {
    // 전역 ID가 host/plugin을 가리켜도 사용자 설정의 출처는 User다.
    // 원래 출처를 삭제하지 않고 명시한 필드만 덮어쓰기 위해 구분한다.
    let id_str = decl.id.clone();
    if !id_str.contains('/') {
        warn!(
            id = id_str.as_str(),
            "file_handler: user handler id missing owner prefix",
        );
        return Err(RejectedUserHandler {
            id: id_str,
            reason: UserHandlerRejectReason::MissingOwnerPrefix,
        });
    }
    let id = HandlerId(id_str);
    push_contribution(
        inner,
        id.clone(),
        HandlerContribution {
            owner: HandlerOwner::User,
            detector: decl.detector,
            priority: decl.priority,
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: decl.disabled,
            action: decl.action.map(Into::into),
        },
    );
    Ok(id)
}

fn push_contribution(inner: &mut Inner, id: HandlerId, contrib: HandlerContribution) {
    let entry = inner.contributions.entry(id).or_default();
    // 같은 origin 으로 재install 시 기존 동일 origin 제거 후 push.
    entry.retain(|c| !same_owner(&c.owner, &contrib.owner));
    entry.push(contrib);
}

fn same_owner(a: &HandlerOwner, b: &HandlerOwner) -> bool {
    match (a, b) {
        (HandlerOwner::Host, HandlerOwner::Host) => true,
        (HandlerOwner::User, HandlerOwner::User) => true,
        (HandlerOwner::Plugin(x), HandlerOwner::Plugin(y)) => x == y,
        _ => false,
    }
}

/// Settings UI 가 `upsert_user_handler` 호출에 사용하는 입력. 모든 필드 optional
/// (patch semantics) — 기존 host/plugin contribution 의 메타데이터를 부분 덮어쓴다.
#[derive(Debug, Clone)]
pub struct UserHandlerUpsertDecl {
    pub id: String,
    pub detector: Option<String>,
    pub priority: Option<i32>,
    pub display_name_i18n_key: Option<String>,
    pub disabled: Option<bool>,
    pub action: Option<UserHandlerActionDecl>,
}

/// User TOML schema. 모든 필드 optional 로 patch 가능.
#[derive(Debug, Clone, Deserialize)]
struct UserHandlerSettingsDecl {
    id: String,
    #[serde(default)]
    detector: Option<String>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    display_name_i18n_key: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    action: Option<UserHandlerActionDecl>,
}

/// `HandlerAction` 을 user TOML 의 `action = { kind = "...", ... }` 표현으로 역변환.
/// `UserHandlerActionDecl` 의 serde tag/snake_case 규칙과 1:1.
fn handler_action_to_toml(action: &HandlerAction) -> toml::value::Table {
    let mut t = toml::value::Table::new();
    match action {
        HandlerAction::OpenSurface {
            surface_kind,
            param_key,
        } => {
            t.insert("kind".into(), toml::Value::String("open_surface".into()));
            t.insert(
                "surface_kind".into(),
                toml::Value::String(surface_kind.clone()),
            );
            t.insert("param_key".into(), toml::Value::String(param_key.clone()));
        }
        HandlerAction::Ipc { method, .. } => {
            t.insert("kind".into(), toml::Value::String("ipc".into()));
            t.insert("method".into(), toml::Value::String(method.clone()));
        }
        HandlerAction::System => {
            t.insert("kind".into(), toml::Value::String("system".into()));
        }
    }
    t
}

fn parse_host_handler_section(
    toml_text: &str,
) -> Result<Vec<HandlerDecl<HostHandlerActionDecl>>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "handler")]
        handlers: Vec<HandlerDecl<HostHandlerActionDecl>>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.handlers)
}

fn parse_user_handler_section(
    toml_text: &str,
) -> Result<Vec<UserHandlerSettingsDecl>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "handler")]
        handlers: Vec<UserHandlerSettingsDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.handlers)
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
