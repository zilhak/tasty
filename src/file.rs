//! 파일 작업 — 형식 식별 / 핸들러 / 드래그 / 디스패치.

pub mod dispatch;
#[cfg(feature = "gui")]
pub mod drag;
pub mod format;
pub mod handler;
#[cfg(feature = "gui")]
pub mod identify_worker;
