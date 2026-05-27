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

/// Pane 식별자.
pub type PaneId = u32;

/// Tab 식별자.
pub type TabId = u32;

/// Surface 식별자.
pub type SurfaceId = u32;
