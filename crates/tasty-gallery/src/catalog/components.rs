//! Tier 3 components — popup / sidebar / tab_bar 등의 props-분리된 view 데모.
//!
//! 각 컴포넌트는 본체 (`crate tasty`) 의 view 함수 시그니처
//! `fn draw_xxx_view(ui, theme, &XxxProps) -> XxxAction` 와 동일한 형태를
//! 로컬에 재현한다. 갤러리는 본체 binary 에 직접 의존할 수 없어 props 타입과
//! 시각 layout 을 *복제* 한다 — 본체 update 시 시각 동등성은 수동 검증.

pub mod apply_preset;
pub mod approval;
pub mod clipboard_viewer;
pub mod command_palette;
pub mod convert;
pub mod explorer_context_menu;
pub mod explorer_favorite_popup;
pub mod explorer_rename_popup;
pub mod explorer_tab_bar;
pub mod explorer_view_cells;
pub mod file_handler_picker;
pub mod git_viewer;
pub mod glyph;
pub mod markdown_open;
pub mod plugin_settings;
pub mod port_scanner;
pub mod preset_editor;
pub mod prim_button;
pub mod prim_chips;
pub mod prim_forms;
pub mod prim_icon_button;
pub mod prim_input;
pub mod prim_nav;
pub mod prim_spinner;
pub mod prim_status_dot;
pub mod prim_status_resolution;
pub mod prim_tab;
pub mod prim_table;
pub mod remote;
pub mod rename_popup;
pub mod search_bar;
pub mod segmented;
pub mod settings;
pub mod sidebar;
pub mod surface_highlights;
pub mod switch_overlay;
pub mod tab_bar;
pub mod toast;
pub mod tools_menu;
