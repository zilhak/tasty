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
    // SAFETY: ns_view_ptr는 winit의 활성 윈도우 RawWindowHandle에서 얻은 NSView 포인터.
    // 본 함수는 mtm 검증 통과 후 main thread에서만 실행되고, 호출 동안 윈도우(따라서 ns_view)는
    // valid 상태로 유지된다 (winit의 event loop가 호출 끝까지 윈도우를 살려둠).
    let ns_view: &NSView = unsafe { &*(ns_view_ptr as *const NSView) };

    let menu = NSMenu::new(mtm);
    menu.setAutoenablesItems(false);

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
