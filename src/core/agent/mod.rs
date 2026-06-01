//! Agent 도메인 (`tasty_agent`) 의 Core wrapper 모음.
//!
//! 5 sub-domain (task / barrier / lease / ratelimit / semaphore) 의 store
//! mutate / read 메서드를 `Core::*` 진입점으로 정리. handler 는 param 파싱과
//! 응답 직렬화만 담당하고, `core.with_memory(...) + ...Store::new(...)` 의
//! store 조립은 본 모듈로 모은다.
//!
//! §0.1 분류표상 거의 모든 메서드가 **Method call** (응답 데이터를 가진
//! mutate). fire-and-forget DomainIntent / CoreEvent 은 본 단계에서 신설하지
//! 않는다 — TODO-agent.md §3 참조.

pub(crate) mod barrier;
pub(crate) mod lease;
pub(crate) mod ratelimit;
pub(crate) mod semaphore;
pub(crate) mod task;
