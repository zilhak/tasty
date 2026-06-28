#![forbid(unsafe_code)]

//! `tasty-gallery` — Tasty UI 컴포넌트 단독 시각 검증 도구.
//!
//! Storybook 류 별도 바이너리. 메인 앱(`tasty`) IPC/CLI 표면에 어떤 영향도
//! 주지 않으며, 본체와 동일한 lib crate (`tasty-egui-theme` 등) 의 함수를
//! 직접 호출해 "데모 = 메인 = 같은 코드" 를 보장한다.
//!
//! Phase 1 범위: Tier 1 카탈로그 (Theme 만 의존하는 항목)
//!   - color swatches
//!   - typography
//!   - spacing
//!   - hint_text widget
//!
//! Tier 2/3 (popup / sidebar / tab_bar 의 props 분리) 는 후속 phase.

pub mod catalog;
pub mod host_shell;
