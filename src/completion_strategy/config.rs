//! Completion strategy TOML/manifest schema (파일 핸들러/훅 핸들러 `config.rs`
//! 미러 — 상세: `docs/dev-guide/agent-runner.md` "완료 판정 전략 레지스트리").
//!
//! 훅 핸들러와 달리 actor(host/plugin/user)별 spec 종류 차이가 없다 — poll/push
//! 둘 다 세 출처 모두 선언 가능하다(셸 action 처럼 특정 actor 만 배제하는 불변식이
//! 없음). 그래서 `HookHandlerDecl<A>` 같은 actor-generic 래퍼 대신 concrete
//! `CompletionStrategyDecl` 하나만 둔다 — 필요 없는 제네릭은 만들지 않는다.
//!
//! push 형의 `notify_via` owner 제한(자기 자신 또는 `host`)과 poll 형의
//! `poll_method`/`default_for_methods` namespace 제한(결정 2·6)은 스키마
//! 차원이 아니라 owner 컨텍스트가 있어야 판정 가능하므로 `registry.rs` finalize
//! 단계에서 강제한다.

use std::fmt;

use serde::Deserialize;

use super::types::{
    CompletionStrategyId, CompletionStrategyKind, CompletionStrategyOwner,
    is_valid_completion_strategy_short_name,
};
use crate::core::agent::completion_strategy::completion_strategy_to_poll_spec;
use crate::hook_handler::HookHandlerId;
/// 트랙 A(`tasty-plugin-manifest`)가 CLI `AutoWaitDecl.strategy` 경로용으로 이미
/// 정의한 poll decl 타입 — 필드 대응 단일 지점(§A-3)을 그대로 재사용한다. 본
/// 파일의 `CompletionStrategyDecl`(plugin contribution 최상위 decl)과 이름이
/// 겹치므로 별칭을 둔다.
use tasty_plugin_manifest::CompletionStrategyDecl as PollStrategyDecl;

#[derive(Debug, Clone, Deserialize)]
pub struct CompletionStrategyDecl {
    /// short-name. 전역 id 로 합쳐질 때 `<owner_prefix>/<short-name>` 이 된다.
    pub id: String,
    pub priority: i32,
    #[serde(default)]
    pub display_name_i18n_key: Option<String>,
    #[serde(default)]
    pub disabled: bool,
    /// 이 전략이 기본 판정이 되는 IPC 메서드 목록(결정 6). 비어 있으면 이름으로만
    /// 참조 가능.
    #[serde(default)]
    pub default_for_methods: Vec<String>,
    pub spec: CompletionStrategySpecDecl,
}

/// poll/push 둘 중 하나 — `[contributes.completion_strategy.spec] kind = "poll" | "push"`.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompletionStrategySpecDecl {
    /// 자체 폴링. 필드 의미·기본값은 트랙 A 의 [`PollStrategyDecl`]
    /// (`tasty_plugin_manifest::CompletionStrategyDecl`)을 그대로 재사용한다 —
    /// 여기서 필드를 다시 나열하면 두 크레이트가 같은 개념을 독립적으로 정의하는
    /// 중복이 재발한다(Gate4 리뷰 지적).
    Poll(PollStrategyDecl),
    /// 외부 보고. `notify_via` 는 훅 핸들러 id(`<owner>/<short>`) 문자열.
    Push { notify_via: String, timeout_ms: u64 },
}

/// decl → 런타임 `CompletionStrategyKind` 변환. **단일 지점** — poll 형은 트랙 A
/// 가 만든 [`completion_strategy_to_poll_spec`]으로 위임한다(필드 대응은 그
/// 함수의 단위테스트가 고정). 필드 대응이 어긋나면 그 테스트가
/// 깨진다.
impl From<CompletionStrategySpecDecl> for CompletionStrategyKind {
    fn from(d: CompletionStrategySpecDecl) -> Self {
        match d {
            CompletionStrategySpecDecl::Poll(decl) => {
                CompletionStrategyKind::Poll(completion_strategy_to_poll_spec(&decl))
            }
            CompletionStrategySpecDecl::Push {
                notify_via,
                timeout_ms,
            } => CompletionStrategyKind::Push {
                notify_via: HookHandlerId::new(notify_via),
                timeout_ms,
            },
        }
    }
}

/// Completion strategy decl schema 검증 실패 사유.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CompletionStrategyDeclError {
    InvalidShortName(String),
}

impl fmt::Display for CompletionStrategyDeclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidShortName(s) => write!(
                f,
                "invalid completion strategy short-name '{s}' (must match [a-z0-9-]{{1,32}})"
            ),
        }
    }
}

impl std::error::Error for CompletionStrategyDeclError {}

pub fn validate_completion_strategy_decl(
    decl: &CompletionStrategyDecl,
) -> Result<(), CompletionStrategyDeclError> {
    if !is_valid_completion_strategy_short_name(&decl.id) {
        return Err(CompletionStrategyDeclError::InvalidShortName(
            decl.id.clone(),
        ));
    }
    Ok(())
}

/// owner prefix 를 씌워 전역 id 를 만든다 (`install_host`/`install_plugin` 공용).
pub fn global_id(owner: &CompletionStrategyOwner, short: &str) -> CompletionStrategyId {
    CompletionStrategyId(format!("{}/{}", owner.prefix(), short))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Deserialize, Debug)]
    struct Wrap {
        #[serde(rename = "strategy")]
        strategies: Vec<CompletionStrategyDecl>,
    }

    fn parse(s: &str) -> Result<Wrap, toml::de::Error> {
        toml::from_str(s)
    }

    #[test]
    fn poll_spec_parses() {
        let t = r#"
            [[strategy]]
            id = "spawn-wait"
            priority = 100
            default_for_methods = ["claude.spawn"]
            [strategy.spec]
            kind = "poll"
            poll_method = "claude.wait"
            state_field = "state"
            terminal_states = ["idle", "needs_input"]
        "#;
        let w = parse(t).expect("parse");
        assert_eq!(w.strategies.len(), 1);
        match &w.strategies[0].spec {
            CompletionStrategySpecDecl::Poll(decl) => {
                assert_eq!(decl.poll_method, "claude.wait");
                assert_eq!(decl.interval_ms, 500); // 기본값 (PollStrategyDecl 소유)
            }
            CompletionStrategySpecDecl::Push { .. } => panic!("expected poll"),
        }
    }

    #[test]
    fn push_spec_requires_timeout() {
        let t = r#"
            [[strategy]]
            id = "notify-done"
            priority = 100
            [strategy.spec]
            kind = "push"
            notify_via = "host/webhook-notify"
        "#;
        // timeout_ms 없음 — push 는 timeout 필수(타입 레벨, Option 아님)이므로 파싱 실패.
        assert!(parse(t).is_err());
    }

    #[test]
    fn push_spec_parses_with_timeout() {
        let t = r#"
            [[strategy]]
            id = "notify-done"
            priority = 100
            [strategy.spec]
            kind = "push"
            notify_via = "host/webhook-notify"
            timeout_ms = 30000
        "#;
        let w = parse(t).expect("parse");
        match &w.strategies[0].spec {
            CompletionStrategySpecDecl::Push {
                notify_via,
                timeout_ms,
            } => {
                assert_eq!(notify_via, "host/webhook-notify");
                assert_eq!(*timeout_ms, 30000);
            }
            CompletionStrategySpecDecl::Poll(_) => panic!("expected push"),
        }
    }

    #[test]
    fn validate_rejects_bad_short_name() {
        let decl = CompletionStrategyDecl {
            id: "Bad/Name".into(),
            priority: 1,
            display_name_i18n_key: None,
            disabled: false,
            default_for_methods: vec![],
            spec: CompletionStrategySpecDecl::Push {
                notify_via: "host/x".into(),
                timeout_ms: 1000,
            },
        };
        assert!(matches!(
            validate_completion_strategy_decl(&decl),
            Err(CompletionStrategyDeclError::InvalidShortName(_))
        ));
    }
}
