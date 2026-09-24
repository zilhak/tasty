//! 완료 판정 전략의 선언·등록·병합·이름 해석을 담당한다.
//! poll은 상태를 조회하고 push는 훅 핸들러의 완료 보고를 기다린다.
//! 실행과 대기는 HostExecutor가 담당한다. docs/dev-guide/agent-runner.md를 참고한다.

pub mod config;
pub mod registry;
pub mod types;

pub use registry::{HostCompletionStrategyPort, global, install_default_sources};
pub use types::{CompletionStrategyId, CompletionStrategyKind};
