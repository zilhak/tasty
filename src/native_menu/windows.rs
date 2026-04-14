//! Windows native context menu using Win32 CreatePopupMenu + TrackPopupMenu.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, TrackPopupMenu,
    MF_ENABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    TPM_RETURNCMD, TPM_RIGHTBUTTON,
};
use windows::core::PCWSTR;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use super::MenuItem;

pub fn show_context_menu(
    window: &impl HasWindowHandle,
    x: f64,
    y: f64,
    items: &[MenuItem],
) -> Option<u32> {
    let hwnd = match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(w) => HWND(w.hwnd.get() as *mut core::ffi::c_void),
        _ => return None,
    };

    unsafe {
        let hmenu = CreatePopupMenu().ok()?;

        for item in items {
            if item.is_separator() {
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
            } else {
                let flags = MF_STRING
                    | if item.enabled { MF_ENABLED } else { MF_GRAYED };
                // Encode label as null-terminated UTF-16.
                let wide: Vec<u16> = item.label.encode_utf16().chain(std::iter::once(0)).collect();
                let _ = AppendMenuW(hmenu, flags, item.id as usize, PCWSTR(wide.as_ptr()));
            }
        }

        // Convert client coordinates to screen coordinates.
        let mut pt = POINT { x: x as i32, y: y as i32 };
        let _ = windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut pt);

        let selected = TrackPopupMenu(
            hmenu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );

        let _ = DestroyMenu(hmenu);

        let id = selected.0 as u32;
        if id == 0 { None } else { Some(id) }
    }
}
