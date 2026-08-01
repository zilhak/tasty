//! 완료 판정 전략 레지스트리 (TODO80 §B) — "임의 IPC dispatch 가 끝났는지"를
//! 이름으로 부를 수 있게 하는 독립 레지스트리.
//!
//! `src/hook_handler/` 를 정본 템플릿으로 미러링한다 — 3출처 병합(host embedded
//! TOML + plugin manifest + user config) + patch semantics + id 규약까지 동일
//! 형태다. **재사용하는 것은 형태이지 코드가 아니다**(TODO80 §B-1/§B-2):
//! `HookSource`/`TriggerSource`/`HookHandlerAction` 은 이 모듈로 가져오지 않는다.
//! 유일한 진짜 참조는 push 형이 완료 보고 주체로 쓰는
//! [`crate::hook_handler::HookHandlerId`] 뿐이다.
//!
//! 핵심 불변식은 [`types`] 모듈 문서 참조 — push 형 timeout 필수, 결정 2(owner
//! namespace 제한), notify_via 참조 무결성(owner 자기 자신 또는 host).
//!
//! **범위 경고**: 이 모듈은 TODO80 §B(레지스트리)만 구현한다. §A(`AutoWaitDecl`
//! 통합), §C(트리거 컨텍스트 통합 + push 완료 신호 실배선), §D(내장 전략 실제
//! 발화), §E(claude/codex 실증 소비자)는 각각 별도 트랙 — 이 모듈의 push 형은
//! **선언과 참조 무결성 검증까지만** 하고, 실제로 완료를 보고받는 배선(hook_id →
//! task_id 매핑, 트리거 payload 의 exit_code 전달)은 아직 없다.

pub mod config;
pub mod registry;
pub mod types;

pub use registry::{HostCompletionStrategyPort, global, install_default_sources};
pub use types::{CompletionStrategyId, CompletionStrategyKind};
