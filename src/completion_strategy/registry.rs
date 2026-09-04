//! 완료 판정 전략 레지스트리 — **훅 핸들러 풀미러**(상세: `docs/dev-guide/
//! agent-runner.md` "완료 판정 전략 레지스트리").
//!
//! `src/hook_handler/registry.rs` 를 정본 템플릿으로 3출처 병합을 갖춘다: host
//! embedded TOML + plugin manifest + user config(`~/.tasty/completion-strategies.toml`).
//! 같은 strategy id 가 여러 출처에 등장하면 **patch semantics**(Host → Plugin → User
//! 순서로 `Some` 필드만 덮어씀), 정렬은 priority↑ → owner tie-break(user>plugin>host)
//! → id.
//!
//! 훅 핸들러와의 차이:
//! - `source`(트리거 출처 게이트) 없음 — 대신 `kind`(poll/push)와 `default_for_methods`.
//! - actor 별 action 배제 불변식 없음(§config.rs 참고) — 셸 배제 같은 구조가 없다.
//! - **결정 2(namespace 제한)**: poll 형의 `poll_method`, `default_for_methods` 의
//!   모든 항목은 plugin 소유면 자기 namespace 만, host/user 소유면 어떤 plugin
//!   namespace 도 아니어야 한다(`_host` 권한 우회 방지) — finalize 에서 구조적으로 강제.
//! - **push 참조 무결성**: `notify_via` 가 가리키는 훅 핸들러가 존재하고 owner 가
//!   자기 자신 또는 `host` 인지 finalize 에서 검증(§B-3).
//! - 인스턴스가 아니라 **프로세스 전역 싱글턴**(`global()`) — 런너 스레드와 IPC 핸들러가
//!   같은 레지스트리를 봐야 하므로(훅 핸들러와 동일 이유).
//!
//! **재사용은 형태뿐** — 이 파일은 `hook_handler` 의 어떤 타입도 import 하지 않는다,
//! 단 하나의 예외는 push 형이 참조하는 [`crate::hook_handler::HookHandlerId`] 그
//! 자체다(§B-3: "notify_via 만 실제 참조다").

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
    /// strategy id → 출처별 contribution. install 순서 보존 (host → plugin → user).
    contributions: BTreeMap<CompletionStrategyId, Vec<Contribution>>,
    finalized: BTreeMap<CompletionStrategyId, CompletionStrategy>,
    /// 결정 6 충돌 해소 결과 — IPC 메서드 → 그 메서드의 기본 전략 id. finalize 가
    /// 함께 재계산한다(활성 전략만 참여, 정렬 승자 채택).
    default_for_method_index: BTreeMap<String, CompletionStrategyId>,
    dirty: bool,
}

/// 완료 판정 전략 레지스트리 (3출처 병합 + patch semantics + lazy finalize).
pub struct CompletionStrategyRegistry {
    inner: RwLock<Inner>,
    /// poison 을 이미 보고했는가 — 로그 폭주 방지용 1 회 게이트.
    poison_reported: std::sync::atomic::AtomicBool,
}

impl CompletionStrategyRegistry {
    /// Poison 을 복구해 read guard 를 잡는다.
    ///
    /// 이전에는 `read().ok()?` / `Err(_) => return` 으로 **조용히** 빠져나갔다. 그
    /// 결과는 "등록한 완료 전략이 반영 안 됨" 인데 관측 지점이 0 이라, 왜 전략이 안
    /// 먹는지 알 방법이 없었다 — 이 저장소의 다른 registry 들이 같은 상황에
    /// `tracing::error!` 를 남기는 것과도 갈렸다.
    ///
    /// `Inner` 는 `BTreeMap` 셋과 `bool` 하나뿐이고 임계구역은 자료구조 조작만 한다.
    /// 패닉이 나도 불변식은 성립하므로 복구가 맞다
    /// ([`error-handling.md`](../../docs/dev-guide/error-handling.md) "락 poison").
    fn lock_read(&self) -> std::sync::RwLockReadGuard<'_, Inner> {
        crate::poison::recover_read(
            self.inner.read(),
            "completion strategy registry",
            &self.poison_reported,
        )
    }

    /// Poison 을 복구해 write guard 를 잡는다. 근거는 [`Self::lock_read`] 와 같다.
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

    // ── 조회 ────────────────────────────────────────────────────────────

    /// id 로 단건 lookup (owned, 비활성 포함). 없으면 `None`.
    pub fn get(&self, id: &CompletionStrategyId) -> Option<CompletionStrategy> {
        self.ensure_finalized();
        let inner = self.lock_read();
        inner.finalized.get(id).cloned()
    }

    /// 이름 참조가 `poll` 자리에서 쓰일 때의 해석 — 존재/활성/kind 를 한 번에 검증
    /// 하고 `PollSpec` 을 돌려준다. `task_create` 검증과 `Custom` dispatch 이름
    /// 해석(runner_host.rs) 양쪽이 공유한다.
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

    /// kind-agnostic 조회 — 존재/활성만 검증하고 poll/push 를 가리지 않고
    /// `CompletionStrategy` 전체(kind 포함)를 반환한다. `resolve_poll_spec` 은
    /// poll 전용 소비자(예: 다른 곳에서도 poll spec 만 필요로 하는 호출부가
    /// 있을 수 있다)를 위해 그대로 유지 — 이 메서드는 `Custom` dispatch
    /// (`runner_host.rs`)처럼 poll/push 를 모두 다뤄야 하는 호출부가 쓴다.
    /// push-kind 의 timeout 필수는 `CompletionStrategyKind::
    /// Push.timeout_ms` 가 `Option` 이 아닌 값 타입이라 타입 레벨에서 이미
    /// 강제된다 — 이 메서드에서 별도 검증이 필요 없다.
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

    /// 결정 6 — 주어진 IPC 메서드의 기본 완료 전략(활성 + 충돌 승자만). 없으면
    /// `None`(기존 즉시-성공 동작 유지, 하위호환).
    pub fn resolve_default_for_method(&self, method: &str) -> Option<CompletionStrategy> {
        self.ensure_finalized();
        let inner = self.lock_read();
        let id = inner.default_for_method_index.get(method)?;
        inner.finalized.get(id).cloned()
    }

    /// **비활성 포함** 전체 전략 (priority↑ → owner tie-break → id). 진단/테스트용 +
    /// `completion_strategy.list` IPC 조회가 사용.
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

    // ── install (3출처) ─────────────────────────────────────────────────

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

    /// plugin uninstall/disable 시 그 plugin 이 기여한 전략을 집합에서 제거
    /// (훅 핸들러 `uninstall_plugin` 미러).
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

/// 한 strategy id 의 출처별 contribution 을 patch semantics 로 병합하고, 결정 2·
/// push 참조 무결성·결정 6 namespace 제한을 검증한다. 위반 시 `None`(그 id 전체를
/// finalize 결과에서 제외) — `ensure_finalized` 의 루프 본체를 분리한 것뿐, 단일
/// 호출자 전용.
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

    // 결정 6 — default_for_methods 전원 owner namespace 여야 함. 개별 위반 항목만
    // 걸러낸다(manifest 단계는 전체 reject 이지만, host/user 는 graceful degrade —
    // 훅 핸들러가 개별 contribution 을 drop 하는 것과 같은 관용).
    let default_for_methods: Vec<String> = default_for_methods
        .unwrap_or_default()
        .into_iter()
        .filter(|m| {
            let ok = method_allowed_for_owner(&owner, m);
            if !ok {
                warn!(
                    strategy_id = id.as_str(),
                    method = m.as_str(),
                    "completion_strategy: default_for_methods entry outside owner namespace — dropped (decision 6)"
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

/// 결정 2(poll_method namespace 제한) + push 참조 무결성(notify_via 존재·owner
/// 자기 자신 또는 host) 검증. 위반 시 warn 후 `false`.
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

/// 결정 2 — poll_method 는 owner namespace 로 제한.
fn poll_method_allowed(
    id: &CompletionStrategyId,
    owner: &CompletionStrategyOwner,
    poll_method: &str,
) -> bool {
    let ok = method_allowed_for_owner(owner, poll_method);
    if !ok {
        warn!(
            strategy_id = id.as_str(),
            poll_method,
            "completion_strategy: poll_method outside owner namespace — dropped (decision 2)"
        );
    }
    ok
}

/// push 참조 무결성 — notify_via 가 가리키는 훅 핸들러가 존재하고 owner 가
/// 자기 자신 또는 host 인지 검증.
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

/// 결정 6 충돌 해소 — 같은 메서드를 여러 활성 전략이 default 로 올리면 정렬
/// 승자(priority↑ → owner tie-break → id)를 채택하고 패자는 warn.
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

/// 결정 2 — `owner` 가 `method` 를 poll_method/default_for_methods 로 쓸 수 있는가.
/// - `Plugin(id)`: prefix 가 정확히 자기 `id` 여야 함.
/// - `Host`/`User`: prefix 가 어떤 plugin 의 등록된 namespace 도 아니어야 함
///   (`_host` 권한으로 남의 plugin namespace 를 호출하는 우회 차단).
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

/// User TOML schema. 모든 필드 optional 로 patch 가능(훅 핸들러
/// `UserHookHandlerSettingsDecl` 미러).
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

// ── 프로세스 전역 싱글턴 ────────────────────────────────────────────────

static REGISTRY: OnceLock<CompletionStrategyRegistry> = OnceLock::new();

/// 전역 완료 판정 전략 레지스트리. 런너 스레드와 IPC 핸들러 스레드가 공유한다
/// (`&'static` — 내부 `RwLock` 로 동기화).
pub fn global() -> &'static CompletionStrategyRegistry {
    REGISTRY.get_or_init(CompletionStrategyRegistry::new)
}

/// `~/.tasty/completion-strategies.toml` — 사용자 완료 판정 전략 설정. 홈 결정
/// 실패 시 `None`(훅 핸들러 `user_config_path` 미러).
pub fn user_config_path() -> Option<PathBuf> {
    tasty_utils::path::tasty_home().map(|d| d.join("completion-strategies.toml"))
}

/// Plugin manager 가 `CompletionStrategyRegistryPort` 로 전략 contribute 를
/// 등록/해제할 때 쓰는 호스트 어댑터(훅 핸들러 `HostHookHandlerPort` 미러).
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

/// 부팅 공용 헬퍼 — host embedded 기본값 + user config 를 전역 레지스트리에 install.
/// `hook_handler::install_default_sources` 와 대칭 — GUI/headless 부팅에서 호출.
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
