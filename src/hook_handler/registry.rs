//! 공유 훅 핸들러 레지스트리 — **S1b: 파일 핸들러 풀미러(정식화)**.
//!
//! 파일 핸들러(`src/file/handler/registry.rs`)를 정본 템플릿으로 3출처 병합을 갖춘다:
//! host embedded TOML + plugin manifest + user config(`~/.tasty/hook-handlers.toml`).
//! 같은 handler id 가 여러 출처에 등장하면 **patch semantics**(Host → Plugin → User
//! 순서로 `Some` 필드만 덮어씀), 정렬은 priority↑ → owner tie-break(user>plugin>host)
//! → id.
//!
//! 파일 핸들러와의 차이:
//! - `detector` 대신 트리거 출처 게이트 `source`(`HookSource`).
//! - `System` 대신 셸 action `ShellCommand` — plugin 은 못 쓰고(타입 배제), host/user 만.
//! - **셸 불변식**: `ShellCommand` 는 `source == Hook` 만 허용 → finalize 에서 구조적으로
//!   강제한다(위반 시 drop + warn). 웹훅(외부 HTTP)→셸 경로가 어떤 출처로도 성립 불가.
//! - 인스턴스가 아니라 **프로세스 전역 싱글턴**(`global()`) — 웹훅 리스너(off-main
//!   thread)와 IPC 핸들러(main thread)가 같은 레지스트리를 봐야 하므로.

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

/// 런타임 등록(익명 웹훅 핸들러 등) 실패 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryError {
    /// 셸 action 은 `source = Hook` 만 허용(불변식 강제).
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
    /// handler id → 출처별 contribution. install 순서 보존 (host → plugin → user).
    contributions: BTreeMap<HookHandlerId, Vec<HookHandlerContribution>>,
    finalized: BTreeMap<HookHandlerId, HookHandler>,
    dirty: bool,
}

/// 공유 훅 핸들러 레지스트리 (3출처 병합 + patch semantics + lazy finalize).
pub struct HookHandlerRegistry {
    inner: RwLock<Inner>,
}

impl HookHandlerRegistry {
    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                finalized: BTreeMap::new(),
                dirty: false,
            }),
        }
    }

    // ── 조회 ────────────────────────────────────────────────────────────

    /// id 로 단건 lookup (owned). 없으면 `None`.
    pub fn get(&self, id: &HookHandlerId) -> Option<HookHandler> {
        self.ensure_finalized();
        let inner = self.inner.read().ok()?;
        inner.finalized.get(id).cloned()
    }

    /// `get` 별칭 (파일 핸들러 `handler()` 미러).
    // 아래 조회 API 군(handler/contains/list_handlers/all_handlers/
    // handlers_for_source/clear_user_handler_override)은 현재 registry_tests 만
    // 사용한다 — 파일 핸들러 레지스트리와의 API 대칭 유지 목적으로 남긴다.
    #[allow(dead_code)]
    pub fn handler(&self, id: &HookHandlerId) -> Option<HookHandler> {
        self.get(id)
    }

    #[allow(dead_code)] // 상동 — 테스트 전용, API 대칭 유지
    pub fn contains(&self, id: &HookHandlerId) -> bool {
        self.ensure_finalized();
        self.inner
            .read()
            .map(|g| g.finalized.contains_key(id))
            .unwrap_or(false)
    }

    /// 전체 핸들러 id (정렬순). 포커스 독립 — 전 범위 조회.
    #[allow(dead_code)] // 상동 — 테스트 전용, API 대칭 유지
    pub fn list_handlers(&self) -> Vec<HookHandlerId> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        inner.finalized.keys().cloned().collect()
    }

    /// 활성 핸들러 전체 (priority↑ → owner tie-break → id).
    #[allow(dead_code)] // 상동 — 테스트 전용, API 대칭 유지
    pub fn all_handlers(&self) -> Vec<HookHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut v: Vec<HookHandler> = inner
            .finalized
            .values()
            .filter(|h| !h.disabled)
            .cloned()
            .collect();
        sort_handlers(&mut v);
        v
    }

    /// **비활성 포함** 전체 핸들러 (priority↑ → owner tie-break → id). 관리·조회
    /// 표면(`hook_handler.list`)이 disabled 핸들러도 보여줘 재활성 대상을 노출한다.
    pub fn all_handlers_including_disabled(&self) -> Vec<HookHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let mut v: Vec<HookHandler> = inner.finalized.values().cloned().collect();
        sort_handlers(&mut v);
        v
    }

    /// 주어진 트리거 출처에 바인딩 가능한 활성 핸들러들 (파일 핸들러 `handlers_for`
    /// 미러). `source.accepts(trigger)` + 웹훅이면 `is_webhook_bindable()` 통과.
    /// priority↑ → owner tie-break → id.
    #[allow(dead_code)] // 상동 — 테스트 전용, API 대칭 유지
    pub fn handlers_for_source(&self, trigger: TriggerSource) -> Vec<HookHandler> {
        self.ensure_finalized();
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
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

    // ── install (3출처) ─────────────────────────────────────────────────

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_host_handler_section(toml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "hook_handler: failed to parse host defaults");
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
        decls: &[HookHandlerDecl<PluginHookHandlerActionDecl>],
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

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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

    // ── 런타임 등록(익명 웹훅 핸들러) ──────────────────────────────────

    /// 완전 지정된 핸들러를 런타임 등록/갱신한다(익명 웹훅 핸들러 등). 같은
    /// id·owner 는 덮어쓴다. 셸 불변식(`ShellCommand` ⇒ `source == Hook`) 적용.
    ///
    /// 파일 핸들러엔 없는 hook 전용 경로 — 인라인 `sequence` 로 등록된 웹훅 핸들러가
    /// 조회 일관성을 위해 레지스트리에 반영될 때 쓴다.
    pub fn upsert_full_handler(&self, handler: HookHandler) -> Result<(), RegistryError> {
        if matches!(handler.action, HookHandlerAction::ShellCommand { .. })
            && handler.source != HookSource::Hook
        {
            return Err(RegistryError::ShellMustBeHookSource {
                id: handler.id.0.clone(),
            });
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return Ok(()),
        };
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

    // ── user config 편집 (Settings UI, S13) ─────────────────────────────

    /// user 출처 contribution 만 모아 TOML 문자열로 직렬화. Settings UI 변경 저장에 사용.
    pub fn export_user_config(&self) -> String {
        let inner = match self.inner.read() {
            Ok(g) => g,
            Err(_) => return String::new(),
        };
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

    /// `export_user_config` 결과를 `path` 에 atomic write.
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

    /// host/plugin/user handler 를 user-origin override 로 disable/enable.
    pub fn set_user_handler_disabled(&self, id: &HookHandlerId, disabled: bool) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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

    /// User-origin contribution 의 `disabled_override` 만 비운다. 다른 user 필드 보존.
    #[allow(dead_code)] // 상동 — 테스트 전용, API 대칭 유지
    pub fn clear_user_handler_override(&self, id: &HookHandlerId) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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

    /// user-origin contribution 전체 제거. host/plugin 은 보존.
    pub fn remove_user_handler(&self, id: &HookHandlerId) {
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
        let Some(entry) = inner.contributions.get_mut(id) else {
            return;
        };
        entry.retain(|c| !matches!(c.owner, HookHandlerOwner::User));
        if entry.is_empty() {
            inner.contributions.remove(id);
        }
        inner.dirty = true;
    }

    /// Settings UI 가 user-origin handler 를 추가/갱신 (patch). 기존 host/plugin 이
    /// 있으면 그 위에 덮는다. id 는 `<owner>/<short>` 형식이어야 한다.
    pub fn upsert_user_handler(
        &self,
        decl: UserHookHandlerUpsertDecl,
    ) -> Result<(), HookHandlerDeclError> {
        if !decl.id.contains('/') {
            return Err(HookHandlerDeclError::InvalidShortName(decl.id.clone()));
        }
        let action: Option<HookHandlerAction> = decl.action.map(Into::into);
        // 셸 불변식: user 가 ShellCommand 를 non-hook source 로 upsert 하려 하면 거부.
        if let Some(HookHandlerAction::ShellCommand { .. }) = &action {
            if decl.source != Some(HookSource::Hook) {
                return Err(HookHandlerDeclError::ShellMustBeHookSource {
                    handler: decl.id.clone(),
                });
            }
        }
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => {
                return Err(HookHandlerDeclError::InvalidShortName(
                    "lock poisoned".into(),
                ));
            }
        };
        push_contribution(
            &mut inner,
            HookHandlerId(decl.id),
            HookHandlerContribution {
                owner: HookHandlerOwner::User,
                source: decl.source,
                priority: decl.priority,
                display_name_i18n_key: decl.display_name_i18n_key,
                disabled_override: decl.disabled,
                action,
            },
        );
        inner.dirty = true;
        Ok(())
    }

    /// 사용자 설정 파일을 다시 읽어 user owner contribution 만 교체. host + plugin 은 그대로.
    ///
    /// **Transactional**: read/parse 실패 시 기존 user contribution 보존.
    pub fn reload_user_config(&self, path: &Path) {
        let Some(decls) = read_user_decls(path) else {
            return;
        };
        let mut inner = match self.inner.write() {
            Ok(g) => g,
            Err(_) => return,
        };
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
        let needs = self.inner.read().map(|g| g.dirty).unwrap_or(false);
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
            if let Some(handler) = merge_contribution(id, contribs) {
                next.insert(id.clone(), handler);
            }
        }
        inner.finalized = next;
        inner.dirty = false;
    }
}

/// 사용자 설정 파일을 읽고 파싱한다. read/parse 실패는 `None`(호출자는 기존
/// user contribution 을 그대로 보존해야 함) — 실패 사유는 여기서 warn 로그로
/// 남긴다. 파일 부재(`NotFound`)는 실패가 아니라 "빈 설정"으로 취급한다.
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

/// [`merge_contribution`] 의 patch-fold 누산기 — Host → Plugin → User 순서로
/// 순회하며 `Some`/override 필드만 덮어쓴 중간 상태.
struct MergeAcc {
    source: Option<HookSource>,
    priority: Option<i32>,
    display: Option<String>,
    disabled: bool,
    action: Option<HookHandlerAction>,
    owner: HookHandlerOwner,
}

/// `acc` 에 `c` 의 override 필드를 patch semantics(설정된 필드만 덮어씀)로 접는다.
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

/// 한 handler id 의 3출처 contribution 을 patch semantics(Host → Plugin → User
/// 순서로 `Some` 필드만 덮어씀)로 병합해 최종 `HookHandler` 를 만든다. 필수
/// 필드(source/action) 누락이나 셸 불변식 위반이면 `None`(호출자는 drop) —
/// 사유는 여기서 warn 로그로 남긴다.
fn merge_contribution(
    id: &HookHandlerId,
    contribs: &[HookHandlerContribution],
) -> Option<HookHandler> {
    let base = contribs.first()?;
    let mut acc = MergeAcc {
        source: base.source,
        priority: base.priority,
        display: base.display_name_i18n_key.clone(),
        disabled: base.disabled_override.unwrap_or(false),
        action: base.action.clone(),
        owner: base.owner.clone(),
    };
    for c in contribs.iter().skip(1) {
        apply_contribution(&mut acc, c);
    }

    // source + action 둘 다 있어야 등록.
    let (Some(source), Some(action)) = (acc.source, acc.action) else {
        warn!(
            handler_id = id.as_str(),
            "hook_handler: handler missing required source or action — dropped"
        );
        return None;
    };
    // 셸 불변식(구조적 강제): ShellCommand 는 source == Hook 만 산다.
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

/// tie-break 시 owner 우선순위 — 작을수록 우선. `user > plugin > host`.
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
    // user TOML 의 id 는 전역 id 형태(`host/<short>` · `<plugin>/<short>` · `user/<short>`).
    // 어느 경우든 contribution owner 는 항상 `User`(= 출처가 사용자 TOML). base
    // contribution(원 출처)은 retain-by-owner 로 보존되고 finalize 가 patch 로 덮는다.
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
    // 같은 origin 으로 재install 시 기존 동일 origin 제거 후 push.
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

/// Settings UI 가 `upsert_user_handler` 호출에 사용하는 입력. 모든 필드 optional
/// (patch semantics).
#[derive(Debug, Clone)]
pub struct UserHookHandlerUpsertDecl {
    pub id: String,
    pub source: Option<HookSource>,
    pub priority: Option<i32>,
    pub display_name_i18n_key: Option<String>,
    pub disabled: Option<bool>,
    pub action: Option<UserHookHandlerActionDecl>,
}

/// User TOML schema. 모든 필드 optional 로 patch 가능.
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

// ── 프로세스 전역 싱글턴 ────────────────────────────────────────────────

static REGISTRY: OnceLock<HookHandlerRegistry> = OnceLock::new();

/// 전역 훅 핸들러 레지스트리. 웹훅 리스너 thread 와 IPC 핸들러 thread 가 공유한다
/// (`&'static` — 내부 `RwLock` 로 동기화).
pub fn global() -> &'static HookHandlerRegistry {
    REGISTRY.get_or_init(HookHandlerRegistry::new)
}

/// `~/.tasty/hook-handlers.toml` — 사용자 훅 핸들러 설정. 홈 결정 실패 시 임시 경로.
pub fn user_config_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("hook-handlers.toml"))
}

/// Plugin manager 가 `HookHandlerRegistryPort` 로 훅 핸들러 contribute 를 등록/해제할
/// 때 쓰는 호스트 어댑터. 레지스트리가 프로세스 전역 싱글턴(`global()`)이라 상태를
/// 갖지 않는 zero-sized 어댑터로 충분하다 — 파일 핸들러(`FileHandlerRegistry` 가
/// 직접 impl)와 달리 여기서는 opaque JSON 을 concrete plugin decl 로 디코드해
/// `global()` 에 위임한다.
pub struct HostHookHandlerPort;

impl tasty_plugin_protocol::host_port::HookHandlerRegistryPort for HostHookHandlerPort {
    fn install_plugin_hook_handlers(&self, plugin_id: &str, handlers: &[serde_json::Value]) {
        let mut decls: Vec<HookHandlerDecl<PluginHookHandlerActionDecl>> =
            Vec::with_capacity(handlers.len());
        for v in handlers {
            match serde_json::from_value::<HookHandlerDecl<PluginHookHandlerActionDecl>>(v.clone())
            {
                Ok(d) => {
                    // schema 검증(short-name). plugin 은 `ShellCommand` 를 타입상 못
                    // 쓰므로 셸 게이트는 불필요하다(config::validate_plugin_hook_handler_decl).
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

/// 부팅 공용 헬퍼 — host embedded 기본값 + user config 를 전역 레지스트리에 install.
/// GUI/headless 부팅에서 웹훅 리스너 init 직전에 호출한다. 중복 호출은 owner 기준
/// retain 으로 idempotent.
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
