//! host·plugin·사용자 선언을 병합한다. 지정된 필드만 덮고 사용자 설정을 마지막에 적용한다.
//! 조회 정렬은 priority 오름차순, 같은 값은 사용자·plugin·host, ID 순서다.
//! 셸 action은 source=hook만 허용한다. 웹훅과 IPC가 같은 전역 등록부를 사용한다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::Deserialize;
use tracing::warn;

use super::config::{
    HookHandlerDecl, HookHandlerDeclError, HostHookHandlerActionDecl, PluginHookHandlerActionDecl,
    UserHookHandlerActionDecl, validate_host_hook_handler_decl, validate_plugin_hook_handler_decl,
};
use super::types::{
    HookHandler, HookHandlerAction, HookHandlerId, HookHandlerOwner, HookSource, TriggerSource,
    is_valid_hook_handler_short_name,
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    ShellMustBeHookSource { id: String },
}

impl std::fmt::Display for RegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::ShellMustBeHookSource { id } => write!(
                f,
                "hook handler '{id}' is a shell command and must declare source = hook"
            ),
        }
    }
}

#[derive(Debug, Clone)]
struct HookHandlerContribution {
    owner: HookHandlerOwner,
    source: Option<HookSource>,
    priority: Option<i32>,
    display_name_i18n_key: Option<String>,
    disabled_override: Option<bool>,
    action: Option<HookHandlerAction>,
}

struct Inner {
    contributions: BTreeMap<HookHandlerId, Vec<HookHandlerContribution>>,
    finalized: BTreeMap<HookHandlerId, HookHandler>,
    dirty: bool,
}

pub struct HookHandlerRegistry {
    inner: RwLock<Inner>,
    poison_reported: std::sync::atomic::AtomicBool,
}

impl HookHandlerRegistry {
    /// poison을 로그로 알리고 기존 상태를 사용한다. 부분 갱신을 되돌리거나 정합을 재검증하지는 않는다.
    fn lock_read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        crate::poison::recover_read(
            self.inner.read(),
            "hook handler registry",
            &self.poison_reported,
        )
    }

    fn lock_write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        crate::poison::recover_write(
            self.inner.write(),
            "hook handler registry",
            &self.poison_reported,
        )
    }

    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                finalized: BTreeMap::new(),
                dirty: false,
            }),
            poison_reported: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn get(&self, id: &HookHandlerId) -> Option<HookHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.get(id).cloned()
    }

    // 이유: 아래 조회 API는 현재 검사에서 사용하며 공통 등록부 인터페이스를 유지한다.
    #[allow(dead_code)]
    pub fn handler(&self, id: &HookHandlerId) -> Option<HookHandler> {
        self.get(id)
    }

    #[allow(dead_code)] // 이유: 현재 검사에서만 사용한다.
    pub fn contains(&self, id: &HookHandlerId) -> bool {
        self.ensure_finalized();
        self.lock_read().finalized.contains_key(id)
    }

    #[allow(dead_code)] // 이유: 현재 검사에서만 사용한다.
    pub fn list_handlers(&self) -> Vec<HookHandlerId> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.keys().cloned().collect()
    }

    #[allow(dead_code)] // 이유: 현재 검사에서만 사용한다.
    pub fn all_handlers(&self) -> Vec<HookHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let mut v: Vec<HookHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    /// 비활성 항목도 포함한다. 관리 화면에서 다시 활성화할 대상을 보여준다.
    pub fn all_handlers_including_disabled(&self) -> Vec<HookHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let mut v: Vec<HookHandler> = inner.finalized.values().cloned().collect();
        sort_handlers(&mut v);
        v
    }

    #[allow(dead_code)] // 이유: 현재 검사에서만 사용한다.
    pub fn handlers_for_source(&self, trigger: TriggerSource) -> Vec<HookHandler> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let mut v: Vec<HookHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled && h.source.accepts(trigger))
            .filter(|h| trigger != TriggerSource::Webhook || h.action.is_webhook_bindable())
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_host_handler_section(toml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "hook_handler: failed to parse host defaults");
                return;
            }
        };
        let mut inner = self.lock_write();
        for decl in decls {
            install_host(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_user_config(&self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "hook_handler: user config read failed");
                return;
            }
        };
        let decls = match parse_user_handler_section(&text) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "hook_handler: user config parse failed");
                return;
            }
        };
        let mut inner = self.lock_write();
        for decl in decls {
            install_user(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_plugin_handlers(
        &self,
        plugin_id: &str,
        decls: &[HookHandlerDecl<PluginHookHandlerActionDecl>],
    ) {
        let mut inner = self.lock_write();
        for decl in decls {
            install_plugin(&mut inner, plugin_id, decl.clone());
        }
        inner.dirty = true;
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let mut inner = self.lock_write();
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(&c.owner, HookHandlerOwner::Plugin(p) if p == plugin_id));
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
        }
        inner.dirty = true;
    }

    /// 같은 ID·owner의 등록을 교체한다. 셸 action이면 source=hook이어야 한다.
    pub fn upsert_full_handler(&self, handler: HookHandler) -> Result<(), RegistryError> {
        if matches!(handler.action, HookHandlerAction::ShellCommand { .. })
            && handler.source != HookSource::Hook
        {
            return Err(RegistryError::ShellMustBeHookSource {
                id: handler.id.0.clone(),
            });
        }
        let mut inner = self.lock_write();
        push_contribution(
            &mut inner,
            handler.id.clone(),
            HookHandlerContribution {
                owner: handler.owner,
                source: Some(handler.source),
                priority: Some(handler.priority),
                display_name_i18n_key: handler.display_name_i18n_key,
                disabled_override: Some(handler.disabled),
                action: Some(handler.action),
            },
        );
        inner.dirty = true;
        Ok(())
    }

    pub fn export_user_config(&self) -> String {
        let inner = self.lock_read();
        let mut handlers = Vec::<toml::Value>::new();
        for (id, contribs) in inner.contributions.iter() {
            let Some(user) = contribs
                .iter()
                .find(|c| matches!(c.owner, HookHandlerOwner::User))
            else {
                continue;
            };
            if user.source.is_none()
                && user.priority.is_none()
                && user.display_name_i18n_key.is_none()
                && user.disabled_override.is_none()
                && user.action.is_none()
            {
                continue;
            }
            let mut t = toml::value::Table::new();
            t.insert("id".into(), toml::Value::String(id.as_str().to_string()));
            if let Some(src) = user.source {
                if let Ok(v) = toml::Value::try_from(src) {
                    t.insert("source".into(), v);
                }
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
                match toml::Value::try_from(action) {
                    Ok(v) => {
                        t.insert("action".into(), v);
                    }
                    Err(e) => warn!(
                        handler = id.as_str(),
                        error = %e,
                        "hook_handler: user action not TOML-serializable — omitted"
                    ),
                }
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

    /// 같은 디렉터리의 임시 파일을 쓴 뒤 대상 경로로 교체한다. fsync 내구성까지 보장하지는 않는다.
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

    #[cfg(any(feature = "gui", test))]
    pub fn set_user_handler_disabled(&self, id: &HookHandlerId, disabled: bool) {
        let mut inner = self.lock_write();
        let Some(entry) = inner.contributions.get_mut(id) else {
            warn!(
                handler = id.as_str(),
                "hook_handler: set_user_handler_disabled — unknown handler"
            );
            return;
        };
        if let Some(existing) = entry
            .iter_mut()
            .find(|c| matches!(c.owner, HookHandlerOwner::User))
        {
            existing.disabled_override = Some(disabled);
        } else {
            entry.push(HookHandlerContribution {
                owner: HookHandlerOwner::User,
                source: None,
                priority: None,
                display_name_i18n_key: None,
                disabled_override: Some(disabled),
                action: None,
            });
        }
        inner.dirty = true;
    }

    #[allow(dead_code)] // 이유: 현재 검사에서만 사용한다.
    pub fn clear_user_handler_override(&self, id: &HookHandlerId) {
        let mut inner = self.lock_write();
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        let mut user_empty = false;
        if let Some(existing) = entry
            .iter_mut()
            .find(|c| matches!(c.owner, HookHandlerOwner::User))
        {
            existing.disabled_override = None;
            user_empty = existing.source.is_none()
                && existing.priority.is_none()
                && existing.display_name_i18n_key.is_none()
                && existing.action.is_none();
        }
        if user_empty {
            entry.retain(|c| !matches!(c.owner, HookHandlerOwner::User));
            if entry.is_empty() {
                inner.contributions.remove(id);
            }
        }
        inner.dirty = true;
    }

    pub fn remove_user_handler(&self, id: &HookHandlerId) {
        let mut inner = self.lock_write();
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        entry.retain(|c| !matches!(c.owner, HookHandlerOwner::User));
        if entry.is_empty() {
            inner.contributions.remove(id);
        }
        inner.dirty = true;
    }

    /// 이전 사용자 값과 host·plugin 값 위에 지정한 필드만 덮는다. None은 삭제가 아니다.
    /// ID는 슬래시 유무만 확인하며 전체 prefix·short-name 문법 검사는 하지 않는다.
    pub fn upsert_user_handler(
        &self,
        decl: UserHookHandlerUpsertDecl,
    ) -> Result<(), HookHandlerDeclError> {
        if !decl.id.contains('/') {
            return Err(HookHandlerDeclError::InvalidShortName(decl.id.clone()));
        }
        let id = HookHandlerId(decl.id.clone());
        let mut inner = self.lock_write();
        let prev = inner
            .contributions
            .get(&id)
            .and_then(|v| v.iter().find(|c| matches!(c.owner, HookHandlerOwner::User)));
        let merged = HookHandlerContribution {
            owner: HookHandlerOwner::User,
            source: decl.source.or_else(|| prev.and_then(|c| c.source)),
            priority: decl.priority.or_else(|| prev.and_then(|c| c.priority)),
            display_name_i18n_key: decl
                .display_name_i18n_key
                .or_else(|| prev.and_then(|c| c.display_name_i18n_key.clone())),
            disabled_override: decl
                .disabled
                .or_else(|| prev.and_then(|c| c.disabled_override)),
            action: decl
                .action
                .map(Into::into)
                .or_else(|| prev.and_then(|c| c.action.clone())),
        };
        // 이전 사용자 값과 합친 결과로 검사해야 명령만 수정할 때 기존 source를 유지할 수 있다.
        if matches!(merged.action, Some(HookHandlerAction::ShellCommand { .. }))
            && merged.source != Some(HookSource::Hook)
        {
            return Err(HookHandlerDeclError::ShellMustBeHookSource { handler: decl.id });
        }
        push_contribution(&mut inner, id, merged);
        inner.dirty = true;
        Ok(())
    }

    /// 읽기·TOML 파싱에 성공하면 사용자 설정을 교체한다. 파일 부재는 빈 설정이다.
    /// 개별 선언 검증 실패는 그 항목만 제외하므로 기존 설정 전체를 보존하는 경우와 구별한다.
    pub fn reload_user_config(&self, path: &Path) {
        let Some(decls) = read_user_decls(path) else {
            return;
        };
        let mut inner = self.lock_write();
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(|c| !matches!(c.owner, HookHandlerOwner::User));
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
            if let Some(handler) = merge_contribution(id, contribs) {
                next.insert(id.clone(), handler);
            }
        }
        inner.finalized = next;
        inner.dirty = false;
    }
}

/// 읽기·파싱 실패는 None으로 돌려 기존 설정을 보존하게 한다. 파일 부재는 빈 목록이다.
fn read_user_decls(path: &Path) -> Option<Vec<UserHookHandlerSettingsDecl>> {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse_user_handler_section(&text) {
            Ok(v) => Some(v),
            Err(e) => {
                warn!(
                    path = %path.display(),
                    error = %e,
                    "hook_handler: reload aborted — parse failed, keeping previous user config",
                );
                None
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Err(e) => {
            warn!(
                path = %path.display(),
                error = %e,
                "hook_handler: reload aborted — read failed, keeping previous user config",
            );
            None
        }
    }
}

struct MergeAcc {
    source: Option<HookSource>,
    priority: Option<i32>,
    display: Option<String>,
    disabled: bool,
    action: Option<HookHandlerAction>,
    owner: HookHandlerOwner,
}

fn apply_contribution(acc: &mut MergeAcc, c: &HookHandlerContribution) {
    if c.source.is_some() {
        acc.source = c.source;
    }
    if c.priority.is_some() {
        acc.priority = c.priority;
    }
    if c.display_name_i18n_key.is_some() {
        acc.display = c.display_name_i18n_key.clone();
    }
    if let Some(d) = c.disabled_override {
        acc.disabled = d;
    }
    if c.action.is_some() {
        acc.action = c.action.clone();
    }
    acc.owner = c.owner.clone();
}

/// host·plugin·사용자 순서로 지정된 필드를 덮는다. source·action 누락이나 셸 제한 위반은 제외한다.
fn merge_contribution(
    id: &HookHandlerId,
    contribs: &[HookHandlerContribution],
) -> Option<HookHandler> {
    let ordered = merge_order(contribs);
    let base = *ordered.first()?;
    let mut acc = MergeAcc {
        source: base.source,
        priority: base.priority,
        display: base.display_name_i18n_key.clone(),
        disabled: base.disabled_override.unwrap_or(false),
        action: base.action.clone(),
        owner: base.owner.clone(),
    };
    for c in ordered.iter().skip(1) {
        apply_contribution(&mut acc, c);
    }

    let (Some(source), Some(action)) = (acc.source, acc.action) else {
        warn!(
            handler_id = id.as_str(),
            "hook_handler: handler missing required source or action — dropped"
        );
        return None;
    };
    if matches!(action, HookHandlerAction::ShellCommand { .. }) && source != HookSource::Hook {
        warn!(
            handler_id = id.as_str(),
            "hook_handler: shell command with non-hook source — dropped (invariant)"
        );
        return None;
    }
    let priority = acc.priority.unwrap_or(100);
    Some(HookHandler {
        id: id.clone(),
        source,
        priority,
        owner: acc.owner,
        action,
        display_name_i18n_key: acc.display,
        disabled: acc.disabled,
    })
}

impl Default for HookHandlerRegistry {
    fn default() -> Self {
        Self::new()
    }
}

fn sort_handlers(v: &mut [HookHandler]) {
    v.sort_by(|a, b| {
        a.priority
            .cmp(&b.priority)
            .then_with(|| owner_rank(&a.owner).cmp(&owner_rank(&b.owner)))
            .then_with(|| a.id.cmp(&b.id))
    });
}

/// 설치 시점에 상관없이 사용자 설정을 마지막에 적용한다. 같은 owner 안에서는 기존 순서를 유지한다.
fn merge_order(contribs: &[HookHandlerContribution]) -> Vec<&HookHandlerContribution> {
    let mut ordered: Vec<&HookHandlerContribution> = contribs.iter().collect();
    ordered.sort_by_key(|c| std::cmp::Reverse(owner_rank(&c.owner)));
    ordered
}

fn owner_rank(owner: &HookHandlerOwner) -> u8 {
    match owner {
        HookHandlerOwner::User => 0,
        HookHandlerOwner::Plugin(_) => 1,
        HookHandlerOwner::Host => 2,
    }
}

fn install_host(inner: &mut Inner, decl: HookHandlerDecl<HostHookHandlerActionDecl>) {
    if let Err(e) = validate_host_hook_handler_decl(&decl) {
        warn!(error = %e, "hook_handler: rejecting host handler decl");
        return;
    }
    let id_str = format!("host/{}", decl.id);
    push_contribution(
        inner,
        HookHandlerId(id_str),
        HookHandlerContribution {
            owner: HookHandlerOwner::Host,
            source: Some(decl.source),
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
    decl: HookHandlerDecl<PluginHookHandlerActionDecl>,
) {
    if let Err(e) = validate_plugin_hook_handler_decl(&decl) {
        warn!(plugin = plugin_id, error = %e, "hook_handler: rejecting plugin handler decl");
        return;
    }
    let id_str = format!("{}/{}", plugin_id, decl.id);
    push_contribution(
        inner,
        HookHandlerId(id_str),
        HookHandlerContribution {
            owner: HookHandlerOwner::Plugin(plugin_id.to_string()),
            source: Some(decl.source),
            priority: Some(decl.priority),
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: if decl.disabled { Some(true) } else { None },
            action: Some(decl.action.into()),
        },
    );
}

fn install_user(inner: &mut Inner, decl: UserHookHandlerSettingsDecl) {
    let id_str = decl.id.clone();
    if !id_str.contains('/') {
        warn!(
            id = id_str.as_str(),
            "hook_handler: user handler id missing owner prefix",
        );
        return;
    }
    if let Some(short) = id_str.split('/').next_back() {
        if !is_valid_hook_handler_short_name(short) {
            warn!(
                id = id_str.as_str(),
                "hook_handler: user handler invalid short-name"
            );
            return;
        }
    }
    push_contribution(
        inner,
        HookHandlerId(id_str),
        HookHandlerContribution {
            owner: HookHandlerOwner::User,
            source: decl.source,
            priority: decl.priority,
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: decl.disabled,
            action: decl.action.map(Into::into),
        },
    );
}

fn push_contribution(inner: &mut Inner, id: HookHandlerId, contrib: HookHandlerContribution) {
    let entry = inner.contributions.entry(id).or_default();
    entry.retain(|c| !same_owner(&c.owner, &contrib.owner));
    entry.push(contrib);
}

fn same_owner(a: &HookHandlerOwner, b: &HookHandlerOwner) -> bool {
    match (a, b) {
        (HookHandlerOwner::Host, HookHandlerOwner::Host) => true,
        (HookHandlerOwner::User, HookHandlerOwner::User) => true,
        (HookHandlerOwner::Plugin(x), HookHandlerOwner::Plugin(y)) => x == y,
        _ => false,
    }
}

/// None 필드는 이전 사용자 값을 유지한다. 필드를 지우는 요청 형식은 아니다.
#[derive(Debug, Clone)]
pub struct UserHookHandlerUpsertDecl {
    pub id: String,
    pub source: Option<HookSource>,
    pub priority: Option<i32>,
    pub display_name_i18n_key: Option<String>,
    pub disabled: Option<bool>,
    pub action: Option<UserHookHandlerActionDecl>,
}

#[derive(Debug, Clone, Deserialize)]
struct UserHookHandlerSettingsDecl {
    id: String,
    #[serde(default)]
    source: Option<HookSource>,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    display_name_i18n_key: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    action: Option<UserHookHandlerActionDecl>,
}

fn parse_host_handler_section(
    toml_text: &str,
) -> Result<Vec<HookHandlerDecl<HostHookHandlerActionDecl>>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "handler")]
        handlers: Vec<HookHandlerDecl<HostHookHandlerActionDecl>>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.handlers)
}

fn parse_user_handler_section(
    toml_text: &str,
) -> Result<Vec<UserHookHandlerSettingsDecl>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "handler")]
        handlers: Vec<UserHookHandlerSettingsDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.handlers)
}

static REGISTRY: OnceLock<HookHandlerRegistry> = OnceLock::new();

pub fn global() -> &'static HookHandlerRegistry {
    REGISTRY.get_or_init(HookHandlerRegistry::new)
}

/// 사용자 설정 경로. 홈을 못 찾으면 None이다.
pub fn user_config_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("hook-handlers.toml"))
}

/// plugin 선언 JSON을 검증하고 전역 등록부에 전달하는 host 어댑터.
pub struct HostHookHandlerPort;

impl tasty_plugin_protocol::host_port::HookHandlerRegistryPort for HostHookHandlerPort {
    fn install_plugin_hook_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]) {
        let mut decls: Vec<HookHandlerDecl<PluginHookHandlerActionDecl>> =
            Vec::with_capacity(handlers.len());
        for v in handlers {
            match serde_json::from_value::<HookHandlerDecl<PluginHookHandlerActionDecl>>(v.clone())
            {
                Ok(d) => {
                    if let Err(e) = validate_plugin_hook_handler_decl(&d) {
                        warn!(plugin = plugin_id, error = %e, "plugin hook handler decl rejected");
                        continue;
                    }
                    decls.push(d);
                }
                Err(e) => warn!(
                    plugin = plugin_id,
                    error = %e,
                    "plugin hook handler decode failed"
                ),
            }
        }
        global().install_plugin_handlers(plugin_id, &decls);
    }

    fn uninstall_plugin(&self, plugin_id: &str) {
        global().uninstall_plugin(plugin_id);
    }
}

pub fn install_default_sources() {
    let reg = global();
    reg.install_host_defaults(include_str!("defaults/default-hook-handlers.toml"));
    if let Some(path) = user_config_path() {
        reg.install_user_config(&path);
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
