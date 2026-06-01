mod divider;
mod draw;
mod egui_panels;
mod sidebar;
mod tab_bar;

pub(crate) mod dialog;
pub(crate) mod drop_overlay;
pub mod font_registry;
pub(crate) mod info_modal;
pub mod layout_context;
pub(crate) mod notification;
pub mod popup;
pub mod preset;
pub(crate) mod search_bar;
pub mod surface;
pub mod terminal_link;
pub mod theme_bridge;
pub mod toast;
pub(crate) mod tools_menu;

pub mod input;

pub use divider::{draw_pane_dividers, draw_surface_highlights};
pub use draw::draw_ui;
pub use egui_panels::draw_egui_panels;
pub use layout_context::LayoutContext;
pub use notification::draw_popups;
pub use popup::{PopupAction, PopupManager};
pub use tab_bar::draw_pane_tab_bars;
pub use toast::{ToastKind, ToastManager, ToastScope};
