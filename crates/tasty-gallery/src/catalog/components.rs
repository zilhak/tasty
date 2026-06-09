//! Tier 3 components — popup / sidebar / tab_bar 등의 props-분리된 view 데모.
//!
//! 각 컴포넌트는 본체 (`crate tasty`) 의 view 함수 시그니처
//! `fn draw_xxx_view(ui, theme, &XxxProps) -> XxxAction` 와 동일한 형태를
//! 로컬에 재현한다. 갤러리는 본체 binary 에 직접 의존할 수 없어 props 타입과
//! 시각 layout 을 *복제* 한다 — 본체 update 시 시각 동등성은 수동 검증.

pub mod convert;
pub mod markdown_open;
pub mod update;
