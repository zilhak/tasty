//! `FileHandlerRegistry` — 등록된 핸들러들을 관리하고 detector 별로 정렬해 반환.
//!
//! 출처별 contribution 을 보관해 plugin uninstall 시 그 plugin 의 handler 만 제거.
//! 같은 handler id 가 여러 출처에 등장하면 patch semantics (Host → Plugin → User
//! 마지막 출처가 명시한 필드만 덮어씀).

use std::collections::BTreeMap;
use std::sync::{Arc, RwLock};

use serde::Deserialize;
use tracing::warn;

use crate::file_format::{DetectorId, DetectorInfo};

use super::config::{
    validate_plugin_handler_decl, HandlerDecl, HandlerDeclError, HostHandlerActionDecl,
    PluginHandlerActionDecl, UserHandlerActionDecl,
};
use super::types::{
    is_valid_handler_short_name, FileHandler, HandlerAction, HandlerId, HandlerOwner,
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
        }
    }

    /// `DetectorInfo` 주입. 부팅 시 1회만. 중복 호출은 warn + 무시.
    pub fn attach_detector_info(&self, info: Arc<dyn DetectorInfo>) {
        let mut slot = match self.detector_info.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        if slot.is_some() {
            warn!("FileHandlerRegistry: detector_info already attached, ignoring");
            return;
        }
        *slot = Some(info);
    }

    /// 주입된 `DetectorInfo` 의 clone. `attach_detector_info` 호출 전이면 `None`.
    pub fn detector_info(&self) -> Option<Arc<dyn DetectorInfo>> {
        self.detector_info.read().ok().and_then(|g| g.clone())
    }

    pub fn handler(&self, id: &HandlerId) -> Option<FileHandler> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        inner.finalized.get(id).cloned()
    }

    pub fn list_handlers(&self) -> Vec<HandlerId> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        inner.finalized.keys().cloned().collect()
    }

    /// `detector` 에 attach 된 활성 handler 들. priority 오름차순, tie 시 owner 우선
    /// `user > plugin > host`, 그 후 handler id alphabetical.
    pub fn handlers_for(&self, detector: &DetectorId) -> Vec<FileHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
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
        let inner = self.inner.read().ok()?;
        inner.finalized.get(id).cloned()
    }

    /// Picker modal 용 — 모든 enabled handler.
    pub fn all_handlers(&self) -> Vec<FileHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut v: Vec<FileHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    // ── install / uninstall ────────────────────────────────────────────

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_host_handler_section(toml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "file_handler: failed to parse host defaults");
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        for decl in decls {
            install_user(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_plugin_handlers(
        &self,
        plugin_id: &str,
        decls: &[HandlerDecl<PluginHandlerActionDecl>],
    ) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
        let mut handlers = Vec::<toml::Value>::new();
        for (id, contribs) in inner.contributions.iter() {
            let user = contribs.iter().find(|c| matches!(c.owner, HandlerOwner::User));
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else {
            warn!(
                handler = id.as_str(),
                "file_handler: set_user_handler_disabled — unknown handler"
            );
            return;
        };
        if let Some(existing) = entry.iter_mut().find(|c| matches!(c.owner, HandlerOwner::User)) {
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else { return };
        let mut user_empty = false;
        if let Some(existing) = entry.iter_mut().find(|c| matches!(c.owner, HandlerOwner::User)) {
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else { return };
        entry.retain(|c| !matches!(c.owner, HandlerOwner::User));
        if entry.is_empty() {
            inner.contributions.remove(id);
        }
        inner.dirty = true;
    }

    /// Settings UI 가 user-origin handler 를 추가/갱신. 기존 host/plugin 이 있으면 patch.
    /// id 가 `<owner>/<short>` 형식이어야 하고, action 의 surface_kind/method 등은 호출자가
    /// 검증한 상태로 넘긴다 (UI 입력 단계에서 후보 dropdown 으로 강제).
    pub fn upsert_user_handler(
        &self,
        decl: UserHandlerUpsertDecl,
    ) -> Result<(), HandlerDeclError> {
        if !decl.id.contains('/') {
            return Err(HandlerDeclError::InvalidShortName(decl.id.clone()));
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return Err(HandlerDeclError::InvalidShortName("lock poisoned".into())),
        };
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
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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
    pub fn reload_user_config(&self, path: &std::path::Path) {
        let decls = match std::fs::read_to_string(path) {
            Ok(text) => match parse_user_handler_section(&text) {
                Ok(v) => v,
                Err(e) => {
                    warn!(
                        path = %path.display(),
                        error = %e,
                        "file_handler: reload aborted — parse failed, keeping previous user config",
                    );
                    return;
                }
            },
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "file_handler: reload aborted — read failed, keeping previous user config",
                );
                return;
            }
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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
        for decl in decls {
            install_user(&mut inner, decl);
        }
        inner.dirty = true;
    }

    fn ensure_finalized(&self) {
        let needs = self
            .inner
            .read()
            .map(|g| g.dirty)
            .unwrap_or(false);
        if !needs {
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
            // 1번째 contribution 이 base — detector / action 필수
            let Some(base) = contribs.first() else { continue };
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

            // detector + action 둘 다 있어야 등록.
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

fn sort_handlers(v: &mut [FileHandler]) {
    v.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| owner_rank(&a.owner).cmp(&owner_rank(&b.owner)))
            .then_with(|| a.id.cmp(&b.id))
    });
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
        warn!(short_name = decl.id.as_str(), "file_handler: invalid host handler short-name");
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

fn install_plugin(
    inner: &mut Inner,
    plugin_id: &str,
    decl: HandlerDecl<PluginHandlerActionDecl>,
) {
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

fn install_user(inner: &mut Inner, decl: UserHandlerSettingsDecl) {
    // user TOML 의 id 는 전역 id 형태로 적힌다 — 예: 자작 "user/<short>", 또는 기존
    // "host/<short>" / "<plugin>/<short>" 패치. 어느 경우든 contribution 의 owner 는
    // 항상 `User` (= 출처가 사용자 TOML). 그래야 base contribution(원 출처) 가
    // `push_contribution` 의 retain-by-owner 에서 보존되고, finalize 가 patch semantics
    // 로 메타만 덮어쓴다.
    let id_str = decl.id.clone();
    if !id_str.contains('/') {
        warn!(
            id = id_str.as_str(),
            "file_handler: user handler id missing owner prefix",
        );
        return;
    }
    push_contribution(
        inner,
        HandlerId(id_str),
        HandlerContribution {
            owner: HandlerOwner::User,
            detector: decl.detector,
            priority: decl.priority,
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: decl.disabled,
            action: decl.action.map(Into::into),
        },
    );
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
            t.insert(
                "param_key".into(),
                toml::Value::String(param_key.clone()),
            );
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
mod tests {
    use super::*;
    use crate::file_format::DetectorId;

    fn load_host(reg: &FileHandlerRegistry) {
        reg.install_host_defaults(include_str!("defaults/default-file-handlers.toml"));
    }

    #[test]
    fn host_default_loads_handlers_for_markdown() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id.as_str(), "host/markdown-viewer");
        matches!(v[0].action, HandlerAction::OpenSurface { .. });
    }

    #[test]
    fn plugin_handler_with_lower_priority_sorts_first() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "markdown".into(),
            priority: 10,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::OpenSurface {
                surface_kind: "mdx_view".into(),
                param_key: "file".into(),
            },
        }];
        reg.install_plugin_handlers("com.example.mdx", &decls);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v.len(), 2);
        assert_eq!(v[0].id.as_str(), "com.example.mdx/viewer");
        assert_eq!(v[1].id.as_str(), "host/markdown-viewer");
    }

    #[test]
    fn user_can_disable_host_handler() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let user_toml = r#"
            [[handler]]
            id = "host/markdown-viewer"
            disabled = true
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert!(v.is_empty());
    }

    #[test]
    fn uninstall_plugin_removes_only_its_handlers() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let decls = vec![HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "markdown".into(),
            priority: 10,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "com.example.mdx.open".into(),
            },
        }];
        reg.install_plugin_handlers("com.example.mdx", &decls);
        assert_eq!(reg.handlers_for(&DetectorId("markdown".into())).len(), 2);
        reg.uninstall_plugin("com.example.mdx");
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id.as_str(), "host/markdown-viewer");
    }

    #[test]
    fn reload_user_config_replaces_user_handlers_keeps_host() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        // 1차: user 가 markdown-viewer priority 만 override.
        std::fs::write(
            &p,
            r#"
                [[handler]]
                id = "host/markdown-viewer"
                priority = 10
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        let v = reg.handlers_for(&DetectorId("markdown".into()));
        assert_eq!(v[0].priority, 10);

        // 2차: user 가 priority override 빼고 새 user/handler 추가 → reload.
        std::fs::write(
            &p,
            r#"
                [[handler]]
                id = "user/my-md"
                detector = "markdown"
                priority = 20
                [handler.action]
                kind = "system"
            "#,
        )
        .unwrap();
        reg.reload_user_config(&p);

        let v = reg.handlers_for(&DetectorId("markdown".into()));
        // host/markdown-viewer 는 호스트 default priority (= 50) 로 복귀.
        let mdv = v.iter().find(|h| h.id.as_str() == "host/markdown-viewer").unwrap();
        assert_eq!(mdv.priority, 50);
        // user/my-md 가 잡혀야 함.
        assert!(v.iter().any(|h| h.id.as_str() == "user/my-md"));
    }

    #[test]
    fn reload_user_config_parse_error_keeps_previous_state() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(
            &p,
            r#"
                [[handler]]
                id = "user/my-md"
                detector = "markdown"
                priority = 20
                [handler.action]
                kind = "system"
            "#,
        )
        .unwrap();
        reg.install_user_config(&p);
        assert!(reg
            .handlers_for(&DetectorId("markdown".into()))
            .iter()
            .any(|h| h.id.as_str() == "user/my-md"));

        // 파일을 깨뜨림 → reload 거부, 기존 user 항목 보존.
        std::fs::write(&p, "[[handler\n id = broken").unwrap();
        reg.reload_user_config(&p);
        assert!(reg
            .handlers_for(&DetectorId("markdown".into()))
            .iter()
            .any(|h| h.id.as_str() == "user/my-md"));
    }

    #[test]
    fn handlers_for_priority_tiebreak_uses_owner_order() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        // plugin 과 user 모두 priority 50 (host 도 50)
        let p = vec![HandlerDecl::<PluginHandlerActionDecl> {
            id: "viewer".into(),
            detector: "markdown".into(),
            priority: 50,
            display_name_i18n_key: None,
            disabled: false,
            action: PluginHandlerActionDecl::Ipc {
                method: "com.example.x.open".into(),
            },
        }];
        reg.install_plugin_handlers("com.example.x", &p);
        let user_toml = r#"
            [[handler]]
            id = "user/my-viewer"
            detector = "markdown"
            priority = 50
            [handler.action]
            kind = "system"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let pth = dir.path().join("file-handlers.toml");
        std::fs::write(&pth, user_toml).unwrap();
        reg.install_user_config(&pth);

        let v = reg.handlers_for(&DetectorId("markdown".into()));
        // priority 모두 50 → tie-break: user > plugin > host
        let owners: Vec<&str> = v.iter().map(|h| h.id.as_str()).collect();
        assert_eq!(owners[0], "user/my-viewer");
        assert_eq!(owners[1], "com.example.x/viewer");
        assert_eq!(owners[2], "host/markdown-viewer");
    }

    #[test]
    fn all_handlers_returns_every_enabled() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let v = reg.all_handlers();
        // host default 4개
        assert_eq!(v.len(), 4);
    }

    // ── cross-module integration: file_format + file_handler ──────────
    //
    // 시나리오: 사용자가 PDF 로 새 detector 와 핸들러를 등록한 뒤
    // 1) `identify(*.pdf)` 가 user detector 를 반환하고
    // 2) `handlers_for(pdf)` 가 user handler 를 반환하는지 확인.

    use crate::file_format::{
        DetectDepth, FileFormatRegistry, FileTarget,
    };

    fn make_user_toml(toml_text: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, toml_text).unwrap();
        dir
    }

    #[test]
    fn user_pdf_detector_and_handler_round_trip() {
        let formats = FileFormatRegistry::new();
        formats.install_host_defaults(include_str!(
            "../file_format/defaults/default-file-format.toml"
        ));

        let handlers = FileHandlerRegistry::new();
        load_host(&handlers);

        let user_toml = r#"
            [[detector]]
            id = "pdf"
            [[detector.rule]]
            kind = "extension"
            values = ["pdf"]

            [[handler]]
            id = "user/pdf-preview"
            detector = "pdf"
            priority = 30
            [handler.action]
            kind = "system"
        "#;
        let dir = make_user_toml(user_toml);
        let p = dir.path().join("file-handlers.toml");
        formats.install_user_config(&p);
        handlers.install_user_config(&p);

        // identify
        let id = formats.identify(
            &FileTarget::new(std::path::PathBuf::from("docs/spec.pdf")),
            DetectDepth::Cheap,
        );
        assert_eq!(id, Some(crate::file_format::DetectorId("pdf".into())));

        // handlers_for
        let v = handlers.handlers_for(&crate::file_format::DetectorId("pdf".into()));
        assert_eq!(v.len(), 1);
        assert_eq!(v[0].id.as_str(), "user/pdf-preview");
        assert!(matches!(v[0].action, HandlerAction::System));
    }

    // ── export_user_config / save_user_config (MD4) ─────────────────────

    #[test]
    fn export_user_handler_emits_only_user_origin() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        // 사용자가 host/markdown-viewer 를 disable 하고 자기 핸들러 user/my-md 추가.
        let user_toml = r#"
            [[handler]]
            id = "host/markdown-viewer"
            disabled = true

            [[handler]]
            id = "user/my-md"
            detector = "markdown"
            priority = 20
            display_name_i18n_key = "user.md"
            [handler.action]
            kind = "system"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exported = reg.export_user_config();
        assert!(exported.contains("host/markdown-viewer"));
        assert!(exported.contains("disabled = true"));
        assert!(exported.contains("user/my-md"));
        assert!(exported.contains("\"markdown\""));
        // host default 의 markdown-viewer action(OpenSurface) 는 user 가 손대지 않았으므로
        // export 결과의 host/markdown-viewer entry 에는 action 이 없어야 한다.
        let lines: Vec<&str> = exported.split("[[handler]]").collect();
        let md_section = lines
            .iter()
            .find(|s| s.contains("host/markdown-viewer"))
            .expect("section present");
        assert!(
            !md_section.contains("kind = \"open_surface\""),
            "user export should not leak host action: {md_section}"
        );
    }

    #[test]
    fn export_user_handler_round_trip() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        let user_toml = r#"
            [[handler]]
            id = "user/my-md"
            detector = "markdown"
            priority = 25
            [handler.action]
            kind = "open_surface"
            surface_kind = "markdown"
            param_key = "file"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("file-handlers.toml");
        std::fs::write(&p, user_toml).unwrap();
        reg.install_user_config(&p);

        let exported = reg.export_user_config();

        let reg2 = FileHandlerRegistry::new();
        load_host(&reg2);
        let p2 = dir.path().join("re-emit.toml");
        std::fs::write(&p2, &exported).unwrap();
        reg2.install_user_config(&p2);

        let v1 = reg.handlers_for(&DetectorId("markdown".into()));
        let v2 = reg2.handlers_for(&DetectorId("markdown".into()));
        let ids1: Vec<_> = v1.iter().map(|h| h.id.as_str().to_string()).collect();
        let ids2: Vec<_> = v2.iter().map(|h| h.id.as_str().to_string()).collect();
        assert_eq!(ids1, ids2);
        // user/my-md 의 priority 가 보존되었는지
        let h2 = v2
            .iter()
            .find(|h| h.id.as_str() == "user/my-md")
            .expect("user handler present");
        assert_eq!(h2.priority, 25);
    }

    #[test]
    fn save_user_handler_atomic_write() {
        let reg = FileHandlerRegistry::new();
        let user_toml = r#"
            [[handler]]
            id = "user/my-md"
            detector = "markdown"
            priority = 25
            [handler.action]
            kind = "system"
        "#;
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("src.toml");
        std::fs::write(&src, user_toml).unwrap();
        reg.install_user_config(&src);

        let dst = dir.path().join("subdir").join("dst.toml");
        reg.save_user_config(&dst).unwrap();
        assert!(dst.exists());
        let written = std::fs::read_to_string(&dst).unwrap();
        assert!(written.contains("user/my-md"));
        assert!(written.contains("kind = \"system\""));
    }

    #[test]
    fn export_empty_when_no_user_contributions() {
        let reg = FileHandlerRegistry::new();
        load_host(&reg);
        assert_eq!(reg.export_user_config(), "");
    }

    #[test]
    fn directory_target_does_not_match_file_detectors() {
        let formats = FileFormatRegistry::new();
        formats.install_host_defaults(include_str!(
            "../file_format/defaults/default-file-format.toml"
        ));
        let handlers = FileHandlerRegistry::new();
        load_host(&handlers);

        let dir = tempfile::tempdir().unwrap();
        let target = FileTarget::new(dir.path().to_path_buf());
        let id = formats
            .identify(&target, DetectDepth::Cheap)
            .expect("directory should identify");
        assert_eq!(id.as_str(), "$directory");
        let v = handlers.handlers_for(&id);
        assert!(!v.is_empty(), "host should register a directory handler");
    }

    // ── DetectorInfo 주입 (Phase E ME1) ──────────────────────────────

    #[test]
    fn attach_detector_info_stores_arc_and_returns_clone() {
        use crate::file_format::FileFormatRegistry;
        let formats = std::sync::Arc::new(FileFormatRegistry::new());
        formats.install_host_defaults(include_str!(
            "../file_format/defaults/default-file-format.toml"
        ));

        let handlers = FileHandlerRegistry::new();
        assert!(handlers.detector_info().is_none());

        handlers.attach_detector_info(formats.clone());
        let info = handlers
            .detector_info()
            .expect("detector_info should be Some after attach");
        // 주입된 info 로 markdown 의 광고된 확장자 조회 가능.
        let exts = info.advertised_extensions(&DetectorId("markdown".into()));
        assert!(exts.contains(&"md".to_string()));
    }

    #[test]
    fn attach_detector_info_second_call_is_ignored() {
        use crate::file_format::FileFormatRegistry;
        let formats_a = std::sync::Arc::new(FileFormatRegistry::new());
        let formats_b = std::sync::Arc::new(FileFormatRegistry::new());
        formats_a.install_host_defaults(include_str!(
            "../file_format/defaults/default-file-format.toml"
        ));
        // formats_b 는 host default 안 깐 빈 registry.

        let handlers = FileHandlerRegistry::new();
        handlers.attach_detector_info(formats_a.clone());
        // 2번째 호출은 무시 → formats_b 가 주입되지 않음.
        handlers.attach_detector_info(formats_b.clone());

        let info = handlers.detector_info().expect("Some after first attach");
        // 첫번째 (formats_a) 가 보유한 markdown 광고가 보여야 함.
        let exts = info.advertised_extensions(&DetectorId("markdown".into()));
        assert!(!exts.is_empty(), "first registry should still be attached");
    }
}
