//! 영속 상태와 세션 상태를 보관하는 저장소.
//! GUI 전용 API의 cfg 범위는 docs/dev-guide/headless-build-boundaries.md를 따른다.

pub mod audit;
pub mod log_retention;
pub mod notification;
pub mod recent_files;
pub mod scrollback;

// 튜토리얼 이력과 관련 DB 스키마는 GUI에서 사용한다.
#[cfg(any(feature = "gui", test))]
pub(crate) mod tutorial_progress;
