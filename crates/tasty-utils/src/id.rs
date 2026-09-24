//! 공용 식별자. 모두 u32 alias이며 서로 구별되는 newtype은 아니다.

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
