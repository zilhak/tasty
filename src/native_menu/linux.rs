//! Linux native context menu using GTK.
//! TODO: Implement using gtk::Menu + popup_at_pointer

use winit::raw_window_handle::HasWindowHandle;
use super::MenuItem;

pub fn show_context_menu(
    _window: &impl HasWindowHandle,
    _x: f64,
    _y: f64,
    _items: &[MenuItem],
) -> Option<u32> {
    // Stub: fall back to None (no selection) until implemented
    tracing::warn!("Native context menu not yet implemented on Linux");
    None
}
