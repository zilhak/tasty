//! Cross-cutting utility helpers for Tasty.
//!
//! **Leaf crate** — 어떤 `tasty-*` crate 도 의존하지 않는다. cross-cutting 인프라
//! (경로 해석 등) 의 단일 출처.
//!
//! 현재 제공:
//! - [`path`] — `tasty_home()` 등 사용자 데이터 디렉토리 경로
//!
//! 도메인별 경로 (themes 디렉토리, memory db 경로, config 파일 위치 등) 는 각
//! 도메인 crate 가 [`path::tasty_home`] 위에 자체 정의한다. utils 는 *공통 기반*
//! 만 노출한다.

pub mod path;
