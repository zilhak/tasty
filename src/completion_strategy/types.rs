//! 완료 판정의 ID·소유자·poll/push 사양. 실행 대상인 훅 action과는 별개다.
//! push는 완료 보고가 오지 않을 때 사용할 timeout_ms를 반드시 받는다.

use tasty_agent::task::PollSpec;

/// 전역 ID. owner prefix와 짧은 이름을 /로 연결한다.
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

pub fn is_valid_completion_strategy_short_name(s: &str) -> bool {
    if s.is_empty() || s.len() > 32 {
        return false;
    }
    s.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum CompletionStrategyOwner {
    Host,
    Plugin(String),
    User,
}

impl CompletionStrategyOwner {
    pub fn prefix(&self) -> &str {
        match self {
            Self::Host => "host",
            Self::Plugin(id) => id.as_str(),
            Self::User => "user",
        }
    }
}

/// poll은 상태를 조회하고 push는 외부 보고를 기다린다. push에는 대기 기한이 필요하다.
#[derive(Debug, Clone, PartialEq)]
pub enum CompletionStrategyKind {
    /// CLI 자동 대기와 같은 PollSpec을 사용한다.
    Poll(PollSpec),
    /// 등록 병합 뒤 notify_via의 존재와 owner 또는 host 소속을 확인한다.
    Push {
        notify_via: crate::hook_handler::HookHandlerId,
        timeout_ms: u64,
    },
}

#[derive(Debug, Clone)]
pub struct CompletionStrategy {
    pub id: CompletionStrategyId,
    pub priority: i32,
    pub owner: CompletionStrategyOwner,
    pub kind: CompletionStrategyKind,
    pub display_name_i18n_key: Option<String>,
    pub disabled: bool,
    /// 기본 전략으로 연결한 메서드. 레지스트리의 owner별 namespace 검사를 통과한 항목만 남는다.
    pub default_for_methods: Vec<String>,
}

/// priority가 작은 항목부터, 같으면 user·plugin·host, 마지막으로 ID 순으로 정렬한다.
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

/// task 생성 검증과 Custom 실행이 공유하는 이름 해석 오류.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StrategyResolveError {
    NotFound {
        name: String,
    },
    Disabled {
        name: String,
    },
    /// poll 인자로는 push 전략을 사용할 수 없다.
    NotPollKind {
        name: String,
    },
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
