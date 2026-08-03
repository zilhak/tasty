//! 완료 판정 전략 레지스트리의 도메인 타입(상세: `docs/dev-guide/agent-runner.md`
//! "완료 판정 전략 레지스트리").
//!
//! `src/hook_handler/types.rs` 구조를 미러링하되, 게이트가 아니라 "끝났는지 판정하는
//! 방법"을 표현한다. **재사용하는 것은 형태이지 코드가 아니다** — `HookSource` /
//! `TriggerSource` / `HookHandlerAction` 은 이 모듈이 import 하지 않는다
//! (action 은 "실행 대상", 전략은 "판정 기준" — 다른 개념).
//!
//! ## 불변식 (타입으로 강제)
//! - **push 전략은 timeout 이 필수**다 — `CompletionStrategyKind::Push.timeout_ms` 는
//!   `Option` 이 아니라 `u64`. 보고 유실 시 task 가 영구 Running 에 남지 않도록 하는
//!   유일한 안전망이다.

use tasty_agent::task::PollSpec;

/// 완료 판정 전략의 전역 유일 식별자.
///
/// 형식은 훅 핸들러와 동일: `host/<short>` · `<plugin_id>/<short>` · `user/<short>`.
/// `<short>` 패턴: `[a-z0-9-]{1,32}`.
#[derive(
    Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct CompletionStrategyId(pub String);

impl CompletionStrategyId {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for CompletionStrategyId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// short-name 패턴 검증 — `[a-z0-9-]{1,32}` (훅 핸들러와 동일 규약).
pub fn is_valid_completion_strategy_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

/// 전략의 출처(누가 등록했나).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CompletionStrategyOwner {
    Host,
    Plugin(String),
    User,
}

impl CompletionStrategyOwner {
    /// CompletionStrategyId prefix segment (`host` · `<plugin_id>` · `user`).
    pub fn prefix(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Plugin(id) => id.as_str(),
            Self::User => "user",
        }
    }
}

/// 완료를 어떻게 판정하는가 — poll(자체 폴링) 또는 push(외부 보고).
///
/// **불변식**: `Push` 는 `timeout_ms` 가 값 타입(필수) — 보고 주체(훅 핸들러)가
/// disable/uninstall 되어도 task 가 영원히 Running 에 남지 않도록 하는 지연
/// 파손 방지 안전망이다.
#[derive(Debug, Clone, PartialEq)]
pub enum CompletionStrategyKind {
    /// 자체 폴링 사양. 매니페스트 decl → host 변환 결과가 여기 담긴다.
    /// `tasty-agent::task::PollSpec` 을 그대로 재사용 — 새 타입을 만들지 않는다.
    Poll(PollSpec),
    /// 외부(훅 핸들러) 보고. 참조 무결성은 등록 시점에 검증(존재 여부) +
    /// owner 를 자기 자신 또는 `host` 로 제한(finalize 강제).
    Push {
        notify_via: crate::hook_handler::HookHandlerId,
        timeout_ms: u64,
    },
}

/// 등록된 완료 판정 전략.
#[derive(Debug, Clone)]
pub struct CompletionStrategy {
    pub id: CompletionStrategyId,
    pub priority: i32,
    pub owner: CompletionStrategyOwner,
    pub kind: CompletionStrategyKind,
    pub display_name_i18n_key: Option<String>,
    pub disabled: bool,
    /// 이 전략이 기본 완료 판정이 되는 IPC 메서드 목록(결정 6, 역방향 소유).
    /// 목록의 모든 메서드는 이 전략의 owner namespace 안이어야 한다(finalize 강제).
    pub default_for_methods: Vec<String>,
}

/// `default_for_methods` 충돌(같은 메서드를 두 전략이 기본으로 선언) 해소에 쓰는
/// 정렬 키 — 훅 핸들러 레지스트리의 우선순위 정렬(priority↑ → owner tie-break
/// user>plugin>host → id)을 그대로 재사용한다(`docs/dev-guide/agent-runner.md`
/// 결정 6 참고).
pub fn strategy_sort_key(s: &CompletionStrategy) -> (i32, u8, &str) {
    (s.priority, owner_rank(&s.owner), s.id.as_str())
}

pub fn owner_rank(owner: &CompletionStrategyOwner) -> u8 {
    match owner {
        CompletionStrategyOwner::User => 0,
        CompletionStrategyOwner::Plugin(_) => 1,
        CompletionStrategyOwner::Host => 2,
    }
}

/// 이름 참조가 실패하는 사유 — `task_create` 검증 및 `Custom` dispatch 이름
/// 해석(runner_host.rs) 양쪽에서 공유한다.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyResolveError {
    /// 그 이름의 전략이 레지스트리에 없음(오타 등).
    NotFound { name: String },
    /// 전략은 있으나 비활성화됨.
    Disabled { name: String },
    /// `poll` 자리에서 참조했는데 전략이 push 형임(문법 오용) — poll 필드는
    /// PollSpec 을 산출하는 전략만 참조할 수 있다.
    NotPollKind { name: String },
}

impl std::fmt::Display for StrategyResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound { name } => write!(f, "completion strategy '{name}' is not registered"),
            Self::Disabled { name } => write!(f, "completion strategy '{name}' is disabled"),
            Self::NotPollKind { name } => write!(
                f,
                "completion strategy '{name}' is a push strategy and cannot be referenced by 'poll' (poll only accepts poll-kind strategies)"
            ),
        }
    }
}
