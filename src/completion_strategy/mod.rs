//! 완료 판정 전략 레지스트리(상세: `docs/dev-guide/agent-runner.md` "완료 판정
//! 전략 레지스트리") — "임의 IPC dispatch 가 끝났는지"를 이름으로 부를 수 있게
//! 하는 독립 레지스트리.
//!
//! `src/hook_handler/` 를 정본 템플릿으로 미러링한다 — 3출처 병합(host embedded
//! TOML + plugin manifest + user config) + patch semantics + id 규약까지 동일
//! 형태다. **재사용하는 것은 형태이지 코드가 아니다**:
//! `HookSource`/`TriggerSource`/`HookHandlerAction` 은 이 모듈로 가져오지 않는다.
//! 유일한 진짜 참조는 push 형이 완료 보고 주체로 쓰는
//! [`crate::hook_handler::HookHandlerId`] 뿐이다.
//!
//! 핵심 불변식은 [`types`] 모듈 문서 참조 — push 형 timeout 필수, 결정 2(owner
//! namespace 제한), notify_via 참조 무결성(owner 자기 자신 또는 host).
//!
//! **범위**: 이 모듈은 선언·설치·이름 해석·참조 무결성 검증까지만 다룬다.
//! push 형이 실제로 완료를 보고받는 배선(host executor 의
//! `dispatch_push_strategy` 가 push-kind 기본 전략 dispatch 시
//! [`crate::core::agent::hook_wait::HookTaskWaits::register`] 를 호출해
//! `AwaitExternal` 로 전이하는 경로)은 이미 구현돼 있다(상세: `docs/dev-guide/
//! agent-runner.md` "완료 판정 전략 레지스트리").

pub mod config;
pub mod registry;
pub mod types;

pub use registry::{HostCompletionStrategyPort, global, install_default_sources};
pub use types::{CompletionStrategyId, CompletionStrategyKind};
