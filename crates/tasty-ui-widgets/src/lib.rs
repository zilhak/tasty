#![forbid(unsafe_code)]

//! `tasty-ui-widgets` — 본체와 갤러리가 공유하는 egui layout / 위젯 primitive.
//!
//! `tasty-egui-theme` (색·폰트·spacing 토큰) 위에 *layout idiom* (frame + sub-tab 패널 등)
//! 을 얹는다. 본체 settings/plugins 와 갤러리가 동일 함수를 호출 → 시각 100% 동기화.
//!
//! 글로벌 `theme()` 호출 금지 — 모든 함수는 `&Theme` 을 명시적으로 받는다.
//! 본체 (`tasty`) 미의존 — 이 crate 는 본체 state 를 모른다.
//!
//! 위젯 함수는 후속 step 에서 점진적으로 추가된다.

mod button;
mod chip;
mod control;
mod help_hint;
mod horizontal_tab_bar;
mod icon_button;
mod input;
mod menu_item;
mod segmented;
mod select;
mod spacing;
mod spinner;
mod status_dot;
mod tooltip;
mod tab_content_frame;
mod table;
mod toggle;
pub mod tokens;
mod tree_row;
mod two_depth;
mod warning_callout;
pub use button::{Button, ButtonVariant};
pub use chip::{BadgeVariant, TagVariant, badge, badge_dot, kbd, num_keycap, tag};
pub use control::ControlSize;
pub use help_hint::HelpHint;
pub use horizontal_tab_bar::horizontal_tab_bar_with_arrows;
pub use icon_button::{IconButton, IconButtonVariant, IconPainter};
pub use input::Input;
pub use menu_item::{MenuItemVariant, menu_item, menu_separator};
pub use segmented::segmented;
pub use select::select;
pub use spacing::{hspace, margin_all, margin_sym, vspace};
pub use spinner::Spinner;
pub use status_dot::{StatusKind, status_dot};
pub use tab_content_frame::tab_content_frame;
pub use table::{Table, TableAlign, TableColumn, TableColumnWidth, TableOutput, TableSortDir};
pub use tooltip::{Tooltip, TooltipPlacement};
pub use toggle::{checkbox, switch};
pub use tree_row::tree_row;
pub use two_depth::{two_depth_layout, two_depth_layout_filtered};
pub use warning_callout::warning_callout;
