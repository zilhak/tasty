//! Windows native context menu using Win32 TrackPopupMenu.
//! TODO: Implement using CreatePopupMenu + TrackPopupMenu(TPM_RETURNCMD)

use winit::raw_window_handle::HasWindowHandle;
use super::MenuItem;

pub fn show_context_menu(
    _window: &impl HasWindowHandle,
    _x: f64,
    _y: f64,
    _items: &[MenuItem],
) -> Option<u32> {
    // Stub: fall back to None (no selection) until implemented
    tracing::warn!("Native context menu not yet implemented on Windows");
    None
}
