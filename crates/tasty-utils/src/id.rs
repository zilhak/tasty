//! Tasty 의 도메인 식별자 alias.
//!
//! 모든 식별자가 단순 `u32` alias — *typed wrapper (newtype)* 가 아니다.
//! Plugin protocol 이 `surface_id: u32` 식으로 raw 통신하는 것과 정합.
//!
//! 미래에 newtype 으로 강화 가능 (예: `pub struct SurfaceId(pub u32)`). 단 그 경우
//! plugin protocol / IPC payload / settings TOML 등 직렬화 표면 모두에 영향을 미치므로
//! 별도 plan 으로 진행.

/// Workspace 식별자.
pub type WorkspaceId = u32;

/// Workspace category(사이드바 폴더) 식별자.
///
/// `0` 은 예약값 — 항상 존재하는 `normal` 카테고리 전용. 발급기는 `1` 부터
/// 단조 증가하므로 사용자 카테고리와 충돌하지 않는다.
pub type WorkspaceCategoryId = u32;

/// 예약된 `normal` 카테고리의 고정 id.
pub const NORMAL_CATEGORY_ID: WorkspaceCategoryId = 0;

/// Pane 식별자.
pub type PaneId = u32;

/// Tab 식별자.
pub type TabId = u32;

/// Surface 식별자.
pub type SurfaceId = u32;
