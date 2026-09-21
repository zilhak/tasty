//! 영속/세션 상태 저장소들.
//!
//! 일부 저장소의 API 는 소비자가 GUI 뿐이라(메뉴·popup·설정 화면) headless 빌드에
//! 호출자가 없다. 그 자리는 정의 옆에서 `cfg` 로 가른다 — 모듈이 통째로 GUI 전용이면
//! 아래 선언에 붙이고, 일부 항목만이면 그 항목에 붙인다
//! (docs/dev-guide/headless-build-boundaries.md).

pub mod audit;
pub mod log_retention;
pub mod notification;
pub mod recent_files;
pub mod scrollback;

/// 튜토리얼 이력은 GUI 화면의 상태다. headless 는 `db::migrations` 자체가 없어
/// 스키마도 안 적용한다 — 모듈 통째가 그 경계 안쪽이다.
#[cfg(any(feature = "gui", test))]
pub(crate) mod tutorial_progress;
