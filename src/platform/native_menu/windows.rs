//! Windows native context menu using Win32 CreatePopupMenu + TrackPopupMenu.

use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, MF_ENABLED, MF_GRAYED, MF_SEPARATOR, MF_STRING,
    TPM_RETURNCMD, TPM_RIGHTBUTTON, TrackPopupMenu,
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
        RawWindowHandle::Win32(w) => HWND(w.hwnd.get() as *mut std::ffi::c_void),
        _ => return None,
    };

    // SAFETY: 네이티브 컨텍스트 메뉴는 winit 윈도우의 main thread (winit event loop)
    // 에서만 호출된다 (PendingNativeMenu 패턴, docs/dev-guide/context-menu.md 참조).
    // hwnd는 위에서 RawWindowHandle::Win32에서 가져온 활성 윈도우 핸들이고, AppendMenuW에
    // 넘기는 PCWSTR은 호출 직전에 만든 local Vec<u16>의 포인터로 TrackPopupMenu 종료까지
    // 살아있다. DestroyMenu는 마지막에 호출 (아래 정리부).
    unsafe {
        let hmenu = CreatePopupMenu().ok()?;

        for item in items {
            if item.is_separator() {
                if let Err(e) = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null()) {
                    tracing::warn!("AppendMenuW separator failed: {e}");
                }
            } else {
                let flags = MF_STRING | if item.enabled { MF_ENABLED } else { MF_GRAYED };
                // Encode label as null-terminated UTF-16.
                let wide: Vec<u16> = item
                    .label
                    .encode_utf16()
                    .chain(std::iter::once(0))
                    .collect();
                if let Err(e) = AppendMenuW(hmenu, flags, item.id as usize, PCWSTR(wide.as_ptr())) {
                    tracing::warn!("AppendMenuW item '{}' failed: {e}", item.label);
                }
            }
        }

        // Convert client coordinates to screen coordinates.
        let mut pt = POINT {
            x: x as i32,
            y: y as i32,
        };
        if !windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut pt).as_bool() {
            tracing::warn!("ClientToScreen failed: {}", std::io::Error::last_os_error());
        }

        let selected = TrackPopupMenu(
            hmenu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON,
            pt.x,
            pt.y,
            Some(0),
            hwnd,
            None,
        );

        if let Err(e) = DestroyMenu(hmenu) {
            tracing::warn!("DestroyMenu failed: {e}");
        }

        let id = selected.0 as u32;
        if id == 0 { None } else { Some(id) }
    }
}
