//! macOS native context menu using NSMenu.

use objc2::MainThreadMarker;
use objc2_app_kit::{NSMenu, NSMenuItem, NSView};
use objc2_foundation::NSString;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::MenuItem;

/// Show a native context menu at the given position (logical coordinates, top-left origin).
/// Returns the `id` of the selected item, or `None` if dismissed.
/// This call is synchronous — it blocks until the user selects or dismisses.
pub fn show_context_menu(
    window: &impl HasWindowHandle,
    x: f64,
    y: f64,
    items: &[MenuItem],
) -> Option<u32> {
    let mtm = MainThreadMarker::new()?;

    let ns_view_ptr = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::AppKit(w) => w.ns_view.as_ptr(),
        _ => return None,
    };
    let ns_view: &NSView = unsafe { &*(ns_view_ptr as *const NSView) };

    let menu = NSMenu::new(mtm);

    for item in items {
        if item.is_separator() {
            let sep = NSMenuItem::separatorItem(mtm);
            menu.addItem(&sep);
        } else {
            let ns_item = NSMenuItem::new(mtm);
            ns_item.setTitle(&NSString::from_str(&item.label));
            ns_item.setTag(item.id as isize);
            ns_item.setEnabled(item.enabled);
            menu.addItem(&ns_item);
        }
    }

    // Convert from top-left logical coordinates to NSView coordinates.
    let is_flipped = ns_view.isFlipped();
    let view_h = ns_view.frame().size.height;
    let ns_y = if is_flipped { y } else { view_h - y };
    let location = objc2_foundation::NSPoint::new(x, ns_y);

    // popUpMenuPositioningItem:atLocation:inView: is synchronous.
    let selected = menu.popUpMenuPositioningItem_atLocation_inView(None, location, Some(ns_view));

    if selected {
        let highlighted = menu.highlightedItem();
        highlighted.map(|item| item.tag() as u32)
    } else {
        None
    }
}
