//! 구조 저널 실행 계층: 저장 batch와 도메인 batch 사이의 변환, journal replay, 명령 실행기.
//!
//! `tasty-domain`의 순수 규칙을 `tasty-event-store`의 저장 계약에 연결한다. 아직 제품 경로에
//! 연결하지 않았으며 시험에서만 사용한다. 본 바이너리의 `CoreState`가 구조 상태의 유일한 원본이고
//! 이 모듈의 상태는 그와 동시에 원본이 되지 않는다.

// reason: 제품 배선 전의 시험 전용 모듈이라 시험 밖 빌드에서는 호출하는 곳이 없다.
// 제품 경로에 연결하면 이 허용을 지운다.
#![cfg_attr(not(test), allow(dead_code))]

pub(crate) mod command_executor;
pub(crate) mod journal;

#[cfg(test)]
mod tests;
