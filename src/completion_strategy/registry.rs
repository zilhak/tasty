//! host 기본값·플러그인 선언·사용자 설정의 완료 전략을 병합한다.
//! 같은 ID는 실제 등록 순서대로 Some 필드를 덮어쓰며 owner도 마지막 기여자로 바뀐다.
//! 병합 뒤 namespace·push 참조를 검사하고 활성 전략의 기본 메서드 충돌은 priority·owner·ID로 정한다.
//! 런너와 IPC가 같은 프로세스 전역 레지스트리를 공유한다.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{OnceLock, RwLock};

use serde::Deserialize;
use tracing::warn;

use super::config::{
    CompletionStrategyDecl, CompletionStrategySpecDecl, global_id,
    validate_completion_strategy_decl,
};
use super::types::{
    CompletionStrategy, CompletionStrategyId, CompletionStrategyKind, CompletionStrategyOwner,
    StrategyResolveError, is_valid_completion_strategy_short_name, strategy_sort_key,
};
use tasty_agent::task::PollSpec;

#[derive(Debug, Clone)]
struct Contribution {
    owner: CompletionStrategyOwner,
    priority: Option<i32>,
    display_name_i18n_key: Option<String>,
    disabled_override: Option<bool>,
    default_for_methods: Option<Vec<String>>,
    kind: Option<CompletionStrategyKind>,
}

struct Inner {
    /// 실제 등록 순서. 같은 owner의 재등록은 이전 기여를 지우고 끝에 추가한다.
    contributions: BTreeMap<CompletionStrategyId, Vec<Contribution>>,
    finalized: BTreeMap<CompletionStrategyId, CompletionStrategy>,
    /// 활성 전략 중 정렬로 선택한 기본 메서드별 전략. finalize에서 다시 계산한다.
    default_for_method_index: BTreeMap<String, CompletionStrategyId>,
    dirty: bool,
}

pub struct CompletionStrategyRegistry {
    inner: RwLock<Inner>,
    poison_reported: std::sync::atomic::AtomicBool,
}

impl CompletionStrategyRegistry {
    /// poison을 로그로 알리고 남은 값을 사용한다. 패닉 직전 여러 표의 갱신까지 완료됐다는 보장은 아니다.
    fn lock_read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        crate::poison::recover_read(
            self.inner.read(),
            "completion strategy registry",
            &self.poison_reported,
        )
    }

    fn lock_write(&self) -> std::sync::RwLockWriteGuard<'_, Inner> {
        crate::poison::recover_write(
            self.inner.write(),
            "completion strategy registry",
            &self.poison_reported,
        )
    }

    pub fn new() -> Self {
        Self {
            inner: RwLock::new(Inner {
                contributions: BTreeMap::new(),
                finalized: BTreeMap::new(),
                default_for_method_index: BTreeMap::new(),
                dirty: false,
            }),
            poison_reported: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn get(&self, id: &CompletionStrategyId) -> Option<CompletionStrategy> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.get(id).cloned()
    }

    /// poll 전용 참조는 존재·활성 여부와 종류를 확인한다.
    pub fn resolve_poll_spec(
        &self,
        id: &CompletionStrategyId,
    ) -> Result<PollSpec, StrategyResolveError> {
        let name = id.as_str().to_string();
        let Some(s) = self.get(id) else {
            return Err(StrategyResolveError::NotFound { name });
        };
        if s.disabled {
            return Err(StrategyResolveError::Disabled { name });
        }
        match s.kind {
            CompletionStrategyKind::Poll(spec) => Ok(spec),
            CompletionStrategyKind::Push { .. } => Err(StrategyResolveError::NotPollKind { name }),
        }
    }

    /// poll과 push를 모두 허용하되 존재·활성 여부는 확인한다.
    pub fn resolve_strategy(
        &self,
        id: &CompletionStrategyId,
    ) -> Result<CompletionStrategy, StrategyResolveError> {
        let name = id.as_str().to_string();
        let Some(s) = self.get(id) else {
            return Err(StrategyResolveError::NotFound { name });
        };
        if s.disabled {
            return Err(StrategyResolveError::Disabled { name });
        }
        Ok(s)
    }

    pub fn resolve_default_for_method(&self, method: &str) -> Option<CompletionStrategy> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let id = inner.default_for_method_index.get(method)?;
        inner.finalized.get(id).cloned()
    }

    /// 비활성 항목도 포함해 priority·owner·ID 순서로 반환한다.
    pub fn all_strategies_including_disabled(&self) -> Vec<CompletionStrategy> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let mut v: Vec<CompletionStrategy> = inner.finalized.values().cloned().collect();
        v.sort_by(|a, b| {
            let (pa, oa, ida) = strategy_sort_key(a);
            let (pb, ob, idb) = strategy_sort_key(b);
            pa.cmp(&pb)
                .then_with(|| oa.cmp(&ob))
                .then_with(|| ida.cmp(idb))
        });
        v
    }

    pub fn install_host_defaults(&self, toml_text: &str) {
        let decls = match parse_strategy_section(toml_text) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "completion_strategy: failed to parse host defaults");
                return;
            }
        };
        let mut inner = self.lock_write();
        for decl in decls {
            install_owned(&mut inner, CompletionStrategyOwner::Host, decl);
        }
        inner.dirty = true;
    }

    pub fn install_user_config(&self, path: &Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "completion_strategy: user config read failed");
                return;
            }
        };
        let decls: Vec<UserCompletionStrategyDecl> = match parse_user_strategy_section(&text) {
            Ok(v) => v,
            Err(e) => {
                warn!(path = %path.display(), error = %e, "completion_strategy: user config parse failed");
                return;
            }
        };
        let mut inner = self.lock_write();
        for decl in decls {
            install_user(&mut inner, decl);
        }
        inner.dirty = true;
    }

    pub fn install_plugin_strategies(&self, plugin_id: &str, decls: &[CompletionStrategyDecl]) {
        let mut inner = self.lock_write();
        for decl in decls {
            if let Err(e) = validate_completion_strategy_decl(decl) {
                warn!(plugin = plugin_id, error = %e, "completion_strategy: rejecting plugin decl");
                continue;
            }
            install_owned(
                &mut inner,
                CompletionStrategyOwner::Plugin(plugin_id.to_string()),
                decl.clone(),
            );
        }
        inner.dirty = true;
    }

    pub fn uninstall_plugin(&self, plugin_id: &str) {
        let mut inner = self.lock_write();
        let mut empty_ids = Vec::new();
        for (id, contribs) in inner.contributions.iter_mut() {
            contribs.retain(
                |c| !matches!(&c.owner, CompletionStrategyOwner::Plugin(p) if p == plugin_id),
            );
            if contribs.is_empty() {
                empty_ids.push(id.clone());
            }
        }
        for id in empty_ids {
            inner.contributions.remove(&id);
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
            if let Some(s) = merge_contribution(id, contribs) {
                next.insert(id.clone(), s);
            }
        }
        let default_for_method_index = resolve_default_for_method_conflicts(&next);

        inner.finalized = next;
        inner.default_for_method_index = default_for_method_index;
        inner.dirty = false;
    }
}

/// 등록 순서로 Some 필드를 덮어쓴 뒤 검증한다. spec·kind가 잘못되면 전략을 제외하고,
/// 잘못된 default_for_methods는 해당 메서드만 제외한다.
fn merge_contribution(
    id: &CompletionStrategyId,
    contribs: &[Contribution],
) -> Option<CompletionStrategy> {
    let base = contribs.first()?;
    let mut priority = base.priority;
    let mut display = base.display_name_i18n_key.clone();
    let mut disabled = base.disabled_override.unwrap_or(false);
    let mut default_for_methods = base.default_for_methods.clone();
    let mut kind = base.kind.clone();
    let mut owner = base.owner.clone();

    for c in contribs.iter().skip(1) {
        if c.priority.is_some() {
            priority = c.priority;
        }
        if c.display_name_i18n_key.is_some() {
            display = c.display_name_i18n_key.clone();
        }
        if let Some(d) = c.disabled_override {
            disabled = d;
        }
        if c.default_for_methods.is_some() {
            default_for_methods = c.default_for_methods.clone();
        }
        if c.kind.is_some() {
            kind = c.kind.clone();
        }
        owner = c.owner.clone();
    }

    let Some(kind) = kind else {
        warn!(
            strategy_id = id.as_str(),
            "completion_strategy: strategy missing required spec — dropped"
        );
        return None;
    };

    if !kind_allowed(id, &owner, &kind) {
        return None;
    }

    // 기본 메서드의 namespace 위반은 전략 전체가 아니라 해당 항목만 제외한다.
    let default_for_methods: Vec<String> = default_for_methods
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            let ok = method_allowed_for_owner(&owner, m);
            if !ok {
                warn!(
                    strategy_id = id.as_str(),
                    method = m.as_str(),
                    "completion_strategy: default_for_methods entry outside owner namespace — dropped"
                );
            }
            ok
        })
        .collect();

    Some(CompletionStrategy {
        id: id.clone(),
        priority: priority.unwrap_or(100),
        owner,
        kind,
        display_name_i18n_key: display,
        disabled,
        default_for_methods,
    })
}

fn kind_allowed(
    id: &CompletionStrategyId,
    owner: &CompletionStrategyOwner,
    kind: &CompletionStrategyKind,
) -> bool {
    match kind {
        CompletionStrategyKind::Poll(spec) => poll_method_allowed(id, owner, &spec.poll_method),
        CompletionStrategyKind::Push { notify_via, .. } => {
            push_notify_via_valid(id, owner, notify_via)
        }
    }
}

fn poll_method_allowed(
    id: &CompletionStrategyId,
    owner: &CompletionStrategyOwner,
    poll_method: &str,
) -> bool {
    let ok = method_allowed_for_owner(owner, poll_method);
    if !ok {
        warn!(
            strategy_id = id.as_str(),
            poll_method, "completion_strategy: poll_method outside owner namespace — dropped"
        );
    }
    ok
}

/// notify_via가 존재하고 ID prefix가 전략 owner 또는 host인지 확인한다.
fn push_notify_via_valid(
    id: &CompletionStrategyId,
    owner: &CompletionStrategyOwner,
    notify_via: &crate::hook_handler::HookHandlerId,
) -> bool {
    if crate::hook_handler::global().get(notify_via).is_none() {
        warn!(
            strategy_id = id.as_str(),
            notify_via = notify_via.as_str(),
            "completion_strategy: notify_via hook handler does not exist — dropped"
        );
        return false;
    }
    let notify_owner_prefix = notify_via.as_str().split('/').next().unwrap_or("");
    if notify_owner_prefix != owner.prefix() && notify_owner_prefix != "host" {
        warn!(
            strategy_id = id.as_str(),
            notify_via = notify_via.as_str(),
            "completion_strategy: notify_via owner must be self or host — dropped"
        );
        return false;
    }
    true
}

/// 같은 메서드의 활성 전략 중 priority가 작은 항목, owner 순위, ID 순으로 선택한다.
fn resolve_default_for_method_conflicts(
    finalized: &BTreeMap<CompletionStrategyId, CompletionStrategy>,
) -> BTreeMap<String, CompletionStrategyId> {
    let mut candidates: BTreeMap<String, Vec<&CompletionStrategy>> = BTreeMap::new();
    for s in finalized.values().filter(|s| !s.disabled) {
        for m in &s.default_for_methods {
            candidates.entry(m.clone()).or_default().push(s);
        }
    }
    let mut default_for_method_index = BTreeMap::new();
    for (method, mut strategies) in candidates {
        strategies.sort_by(|a, b| {
            let (pa, oa, ida) = strategy_sort_key(a);
            let (pb, ob, idb) = strategy_sort_key(b);
            pa.cmp(&pb)
                .then_with(|| oa.cmp(&ob))
                .then_with(|| ida.cmp(idb))
        });
        if let Some(winner) = strategies.first() {
            if strategies.len() > 1 {
                warn!(
                    method = method.as_str(),
                    winner = winner.id.as_str(),
                    losers = ?strategies[1..].iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
                    "completion_strategy: default_for_methods conflict — sort winner adopted"
                );
            }
            default_for_method_index.insert(method, winner.id.clone());
        }
    }
    default_for_method_index
}

impl Default for CompletionStrategyRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// plugin은 owner prefix와 같은 메서드만 허용한다. host·user는 등록된 plugin prefix를 사용할 수 없다.
fn method_allowed_for_owner(owner: &CompletionStrategyOwner, method: &str) -> bool {
    let prefix = method.split('.').next().unwrap_or("");
    match owner {
        CompletionStrategyOwner::Plugin(id) => prefix == id,
        CompletionStrategyOwner::Host | CompletionStrategyOwner::User => {
            !tasty_ipc::method_meta::is_registered_plugin_prefix(prefix)
        }
    }
}

fn install_owned(inner: &mut Inner, owner: CompletionStrategyOwner, decl: CompletionStrategyDecl) {
    let id = global_id(&owner, &decl.id);
    push_contribution(
        inner,
        id,
        Contribution {
            owner,
            priority: Some(decl.priority),
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: if decl.disabled { Some(true) } else { None },
            default_for_methods: Some(decl.default_for_methods),
            kind: Some(decl.spec.into()),
        },
    );
}

fn install_user(inner: &mut Inner, decl: UserCompletionStrategyDecl) {
    let id_str = decl.id.clone();
    if !id_str.contains('/') {
        warn!(
            id = id_str.as_str(),
            "completion_strategy: user strategy id missing owner prefix",
        );
        return;
    }
    if let Some(short) = id_str.split('/').next_back()
        && !is_valid_completion_strategy_short_name(short)
    {
        warn!(
            id = id_str.as_str(),
            "completion_strategy: user strategy invalid short-name"
        );
        return;
    }
    push_contribution(
        inner,
        CompletionStrategyId(id_str),
        Contribution {
            owner: CompletionStrategyOwner::User,
            priority: decl.priority,
            display_name_i18n_key: decl.display_name_i18n_key,
            disabled_override: decl.disabled,
            default_for_methods: decl.default_for_methods,
            kind: decl.spec.map(Into::into),
        },
    );
}

fn push_contribution(inner: &mut Inner, id: CompletionStrategyId, contrib: Contribution) {
    let entry = inner.contributions.entry(id).or_default();
    entry.retain(|c| !same_owner(&c.owner, &contrib.owner));
    entry.push(contrib);
}

fn same_owner(a: &CompletionStrategyOwner, b: &CompletionStrategyOwner) -> bool {
    match (a, b) {
        (CompletionStrategyOwner::Host, CompletionStrategyOwner::Host) => true,
        (CompletionStrategyOwner::User, CompletionStrategyOwner::User) => true,
        (CompletionStrategyOwner::Plugin(x), CompletionStrategyOwner::Plugin(y)) => x == y,
        _ => false,
    }
}

/// ID를 지정하고 제공한 설정 필드만 덮어쓰는 사용자 TOML.
#[derive(Debug, Clone, Deserialize)]
struct UserCompletionStrategyDecl {
    id: String,
    #[serde(default)]
    priority: Option<i32>,
    #[serde(default)]
    display_name_i18n_key: Option<String>,
    #[serde(default)]
    disabled: Option<bool>,
    #[serde(default)]
    default_for_methods: Option<Vec<String>>,
    #[serde(default)]
    spec: Option<CompletionStrategySpecDecl>,
}

fn parse_strategy_section(toml_text: &str) -> Result<Vec<CompletionStrategyDecl>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "strategy")]
        strategies: Vec<CompletionStrategyDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.strategies)
}

fn parse_user_strategy_section(
    toml_text: &str,
) -> Result<Vec<UserCompletionStrategyDecl>, toml::de::Error> {
    #[derive(Deserialize)]
    struct Wrap {
        #[serde(default, rename = "strategy")]
        strategies: Vec<UserCompletionStrategyDecl>,
    }
    let w: Wrap = toml::from_str(toml_text)?;
    Ok(w.strategies)
}

static REGISTRY: OnceLock<CompletionStrategyRegistry> = OnceLock::new();

/// 런너와 IPC가 공유하며 내부 RwLock으로 접근을 조율한다.
pub fn global() -> &'static CompletionStrategyRegistry {
    REGISTRY.get_or_init(CompletionStrategyRegistry::new)
}

/// 사용자 설정 경로. 홈을 찾지 못하면 None이다.
pub fn user_config_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("completion-strategies.toml"))
}

/// PluginManager의 전략 등록·해제를 호스트 레지스트리에 연결한다.
pub struct HostCompletionStrategyPort;

impl tasty_plugin_protocol::host_port::CompletionStrategyRegistryPort
    for HostCompletionStrategyPort
{
    fn install_plugin_completion_strategies(
        &self,
        plugin_id: &str,
        strategies: &[serde_json::Value],
    ) {
        let mut decls: Vec<CompletionStrategyDecl> = Vec::with_capacity(strategies.len());
        for v in strategies {
            match serde_json::from_value::<CompletionStrategyDecl>(v.clone()) {
                Ok(d) => decls.push(d),
                Err(e) => warn!(
                    plugin = plugin_id,
                    error = %e,
                    "plugin completion strategy decode failed"
                ),
            }
        }
        global().install_plugin_strategies(plugin_id, &decls);
    }

    fn uninstall_plugin(&self, plugin_id: &str) {
        global().uninstall_plugin(plugin_id);
    }
}

/// 부팅 때 host 기본값을 넣고 사용자 설정을 적용한다.
pub fn install_default_sources() {
    let reg = global();
    reg.install_host_defaults(include_str!("defaults/default-completion-strategies.toml"));
    if let Some(path) = user_config_path() {
        reg.install_user_config(&path);
    }
}

#[cfg(test)]
#[path = "registry_tests.rs"]
mod tests;
