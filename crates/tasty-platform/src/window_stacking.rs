//! 에이전트가 만든 창을 **사용자가 보던 창 뒤에**, 키 포커스 없이 보이게 한다.
//!
//! 원칙 1·3(에이전트 행동의 부수효과가 사용자 포커스에 닿지 않는다). 호출자는 창을
//! `with_visible(false)` · `with_active(false)` 로 만든 뒤 [`show_behind`] 로 보인다.
//! 사용자 발화 창은 이 모듈을 거치지 않는다.
//!
//! OS 마다 쓸 수 있는 가장 강한 수단이 다르다. winit 0.30.13 의 show 경로를 그대로 쓰면
//! 안 되는 자리가 있어서다.
//!
//! - **macOS**: winit `set_visible(true)` 는 `makeKeyAndOrderFront` 다 — 키 창이 되고 맨
//!   앞으로 온다. 그래서 winit 을 거치지 않고 `orderWindow:relativeTo:` 로 사용자 창
//!   바로 아래에 둔다. winit 은 macOS 에서 보임 상태를 따로 들고 있지 않다(`isVisible` 을
//!   매번 묻는다).
//! - **Windows**: winit 이 `WindowFlags::VISIBLE` 를 들고 있고, 그 플래그가 꺼진 채 다른
//!   플래그가 바뀌면 `ShowWindow(SW_HIDE)` 를 부른다. 그래서 보이는 것은 winit
//!   (`SW_SHOWNOACTIVATE` — 활성화 없음)으로 하고, z-order 만 `SetWindowPos` 로 사용자 창
//!   바로 아래에 건다. 보이기 전과 뒤에 한 번씩 건다. 그 사이에 창이 맨 위에 보이는 틈은
//!   생기지 않을 것으로 본다 — winit 의 `set_visible` 은 이벤트 루프 스레드에서 동기로 돌고,
//!   `apply_diff` 는 `SW_SHOWNOACTIVATE` 만 부르며 z-order 를 올리는 호출이 없다. 실기
//!   미측정이다.
//! - **X11**: winit 이 `with_active` 를 무시하고, 보일 때 `stack_mode=ABOVE` 를 건다.
//!   map 전에 EWMH `_NET_WM_USER_TIME = 0`(초기 포커스를 주지 말라)을 걸고, map 은 winit 으로
//!   하고(보임 상태를 winit 이 들고 있다), 그 뒤 `_NET_RESTACK_WINDOW` 로 사용자 창 아래를
//!   **요청한다.** 창 관리자가 무시할 수 있다. winit 의 Xlib 연결을 그대로 써서 winit 의
//!   map 요청 뒤에 순서대로 닿는다. `_NET_WM_USER_TIME = 0` 은 map 뒤에
//!   [`clear_initial_focus_hint`] 로 지운다 — EWMH 가 그 값을 map 시점의 초기 포커스로만
//!   정하므로 목적은 그대로이고, 남겨 두면 나중에 사용자가 부른 활성화(트레이 show 등)를
//!   막는 창 관리자가 있을 수 있다.
//! - **Wayland**: 클라이언트가 쌓임 순서나 포커스를 정할 프로토콜이 없다. xdg-activation 을
//!   요청하지 않는 것이 최선이고 winit 은 `with_active(false)` 로 요청하지 않는다. 그냥
//!   보인다.
//!
//! 실패는 `Err` 로 돌려준다. 호출자는 경고를 남기고 winit `set_visible(true)` 로 보인다
//! (창 생성을 실패시키지 않는다). `Err` 는 창을 보이기 **전**의 실패이거나, 이미 보인 뒤의
//! 실패라도 winit 의 `set_visible(true)` 가 멱등인 경로에서만 난다.
//!
//! 결정 근거와 OS 별 결과는 `docs/adr/0497-an-agent-created-window-does-not-take-the-users-focus.md`.

use winit::window::Window;

/// 숨겨 만든 `window` 를 `anchor`(사용자가 보던 창) 바로 아래에, 키 포커스 없이 보인다.
/// `anchor` 가 없으면 활성화 없이 보이기만 한다.
pub fn show_behind(window: &Window, anchor: Option<&Window>) -> Result<(), String> {
    imp::show_behind(window, anchor)
}

/// [`show_behind`] 가 map 전에 건 초기 포커스 힌트를 지운다. 창이 map 된 뒤 한 번 부른다.
/// X11 에서만 무언가를 하고(`_NET_WM_USER_TIME` 삭제), 그 밖에서는 아무것도 안 한다.
pub fn clear_initial_focus_hint(window: &Window) -> Result<(), String> {
    imp::clear_initial_focus_hint(window)
}

#[cfg(windows)]
mod imp {
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SetWindowPos,
    };
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    fn hwnd(window: &Window) -> Result<HWND, String> {
        match window
            .window_handle()
            .map_err(|e| format!("window handle: {e}"))?
            .as_raw()
        {
            RawWindowHandle::Win32(h) => Ok(HWND(h.hwnd.get() as *mut std::ffi::c_void)),
            other => Err(format!("not a Win32 window handle: {other:?}")),
        }
    }

    fn place_below(window: HWND, anchor: HWND) -> Result<(), String> {
        // SAFETY: 두 HWND 는 winit 이 살려 두고 있는 창의 핸들이다(호출 동안 `&Window` 를
        // 빌리고 있다). 위치·크기는 NOMOVE·NOSIZE 로 안 바꾸고 z-order 만 바꾼다.
        unsafe {
            SetWindowPos(
                window,
                Some(anchor),
                0,
                0,
                0,
                0,
                SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE,
            )
        }
        .map_err(|e| format!("SetWindowPos: {e}"))
    }

    pub(super) fn show_behind(window: &Window, anchor: Option<&Window>) -> Result<(), String> {
        let Some(anchor) = anchor else {
            window.set_visible(true);
            return Ok(());
        };
        let new = hwnd(window)?;
        let below = hwnd(anchor)?;
        place_below(new, below)?;
        // `with_active(false)` 로 만든 창이라 winit 은 `SW_SHOWNOACTIVATE` 로 보인다.
        window.set_visible(true);
        // 보이는 순간 z-order 가 바뀌지는 않을 것으로 보지만(모듈 문서) 실기로 재지 못해
        // 한 번 더 건다. 여기서 실패해도 창은 이미
        // 보이고, 호출자의 `set_visible(true)` 폴백은 winit 안에서 아무것도 안 한다.
        place_below(new, below)
    }

    pub(super) fn clear_initial_focus_hint(_window: &Window) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use objc2::rc::Retained;
    use objc2_app_kit::{NSView, NSWindow, NSWindowOrderingMode};
    use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
    use winit::window::Window;

    fn ns_window(window: &Window) -> Result<Retained<NSWindow>, String> {
        let ns_view_ptr = match window
            .window_handle()
            .map_err(|e| format!("window handle: {e}"))?
            .as_raw()
        {
            RawWindowHandle::AppKit(h) => h.ns_view.as_ptr(),
            other => return Err(format!("not an AppKit window handle: {other:?}")),
        };
        // SAFETY: ns_view_ptr 는 winit 이 살려 두고 있는 창의 NSView 다(호출 동안 `&Window` 를
        // 빌리고 있다). 이 함수는 winit 이벤트 루프(main thread)에서만 불린다.
        let ns_view: &NSView = unsafe { &*(ns_view_ptr as *const NSView) };
        ns_view
            .window()
            .ok_or_else(|| "NSView has no NSWindow".to_string())
    }

    pub(super) fn show_behind(window: &Window, anchor: Option<&Window>) -> Result<(), String> {
        // 모든 핸들을 창을 보이기 전에 얻는다 — 그래야 `Err` 가 늘 보이기 전의 실패다
        // (보인 뒤의 폴백 `set_visible(true)` 는 macOS 에서 키 창을 만든다).
        let new = ns_window(window)?;
        match anchor {
            Some(anchor) => {
                let number = ns_window(anchor)?.windowNumber();
                new.orderWindow_relativeTo(NSWindowOrderingMode::Below, number);
            }
            // 가리키던 창이 없다 — winit 이 `with_active(false)` 창을 생성할 때 쓰는
            // 것과 같은 호출(키 창으로 만들지 않고 보이기만).
            None => new.orderFront(None),
        }
        Ok(())
    }

    pub(super) fn clear_initial_focus_hint(_window: &Window) -> Result<(), String> {
        Ok(())
    }
}

#[cfg(target_os = "linux")]
mod imp {
    use std::ffi::c_ulong;

    use winit::raw_window_handle::{
        HasDisplayHandle, HasWindowHandle, RawDisplayHandle, RawWindowHandle,
    };
    use winit::window::Window;
    use x11_dl::xlib;

    /// EWMH `_NET_RESTACK_WINDOW` 의 detail 값 `Below`.
    const RESTACK_BELOW: std::os::raw::c_long = 1;
    /// EWMH source indication. 스펙상 응용의 값은 1 이지만 openbox 3.6.1 은 1 을
    /// "invalid source indication" 으로 버린다(실측). 다른 창 관리자는 미측정이다.
    /// 그래서 직접 사용자 조작을 뜻하는 2 를 쓴다(ADR-0497).
    const SOURCE_DIRECT: std::os::raw::c_long = 2;

    fn xlib_window(window: &Window) -> Result<Option<c_ulong>, String> {
        match window
            .window_handle()
            .map_err(|e| format!("window handle: {e}"))?
            .as_raw()
        {
            RawWindowHandle::Xlib(h) => Ok(Some(h.window)),
            _ => Ok(None),
        }
    }

    fn xlib_display(window: &Window) -> Result<*mut xlib::Display, String> {
        match window
            .display_handle()
            .map_err(|e| format!("display handle: {e}"))?
            .as_raw()
        {
            RawDisplayHandle::Xlib(h) => h
                .display
                .map(|p| p.as_ptr() as *mut xlib::Display)
                .ok_or_else(|| "Xlib display handle is empty".to_string()),
            other => Err(format!("not an Xlib display handle: {other:?}")),
        }
    }

    fn intern(x: &xlib::Xlib, dpy: *mut xlib::Display, name: &std::ffi::CStr) -> xlib::Atom {
        // SAFETY: dpy 는 winit 이 열어 두고 있는 Xlib 연결이고 name 은 NUL 종단 문자열이다.
        unsafe { (x.XInternAtom)(dpy, name.as_ptr(), xlib::False) }
    }

    /// map 전에 `_NET_WM_USER_TIME = 0` — "이 창에 초기 포커스를 주지 말라".
    fn set_zero_user_time(x: &xlib::Xlib, dpy: *mut xlib::Display, win: c_ulong) {
        let atom = intern(x, dpy, c"_NET_WM_USER_TIME");
        let zero: std::os::raw::c_long = 0;
        // SAFETY: format 32 의 원소 1 개 — Xlib 규약상 c_long 한 칸을 가리킨다. win 은
        // winit 이 만든 살아 있는 창이다.
        unsafe {
            (x.XChangeProperty)(
                dpy,
                win,
                atom,
                xlib::XA_CARDINAL,
                32,
                xlib::PropModeReplace,
                (&zero as *const std::os::raw::c_long).cast::<u8>(),
                1,
            )
        };
        // SAFETY: 같은 연결의 요청 버퍼를 비운다.
        unsafe { (x.XFlush)(dpy) };
    }

    /// map 뒤에 `_NET_RESTACK_WINDOW` — "win 을 sibling 바로 아래에 두라".
    fn request_restack_below(
        x: &xlib::Xlib,
        dpy: *mut xlib::Display,
        win: c_ulong,
        sibling: c_ulong,
    ) -> Result<(), String> {
        let mut data = xlib::ClientMessageData::new();
        data.set_long(0, SOURCE_DIRECT);
        data.set_long(1, sibling as std::os::raw::c_long);
        data.set_long(2, RESTACK_BELOW);
        let message = xlib::XClientMessageEvent {
            type_: xlib::ClientMessage,
            serial: 0,
            send_event: xlib::True,
            display: dpy,
            window: win,
            message_type: intern(x, dpy, c"_NET_RESTACK_WINDOW"),
            format: 32,
            data,
        };
        let mut event = xlib::XEvent {
            client_message: message,
        };
        // SAFETY: dpy 는 winit 의 살아 있는 연결이다.
        let root = unsafe { (x.XDefaultRootWindow)(dpy) };
        // SAFETY: event 는 위에서 완전히 채운 ClientMessage 이고 root 는 같은 연결의 루트 창.
        let status = unsafe {
            (x.XSendEvent)(
                dpy,
                root,
                xlib::False,
                xlib::SubstructureRedirectMask | xlib::SubstructureNotifyMask,
                &mut event,
            )
        };
        // SAFETY: 같은 연결의 요청 버퍼를 비운다.
        unsafe { (x.XFlush)(dpy) };
        if status == 0 {
            return Err("XSendEvent(_NET_RESTACK_WINDOW) failed".to_string());
        }
        Ok(())
    }

    pub(super) fn show_behind(window: &Window, anchor: Option<&Window>) -> Result<(), String> {
        // Wayland(또는 X11 이 아닌 무엇)이면 할 수 있는 것이 없다 — 보이기만 한다.
        let Some(win) = xlib_window(window)? else {
            window.set_visible(true);
            return Ok(());
        };
        let dpy = xlib_display(window)?;
        let sibling = match anchor {
            Some(a) => xlib_window(a)?,
            None => None,
        };
        let x = xlib::Xlib::open().map_err(|e| format!("Xlib::open: {e}"))?;
        set_zero_user_time(&x, dpy, win);
        // map 은 winit 으로 한다 — winit 이 보임 상태를 들고 있다.
        window.set_visible(true);
        match sibling {
            // 이미 보인 뒤라 실패해도 호출자의 `set_visible(true)` 폴백은 winit 안에서
            // 아무것도 안 한다.
            Some(sibling) => request_restack_below(&x, dpy, win, sibling),
            None => Ok(()),
        }
    }

    pub(super) fn clear_initial_focus_hint(window: &Window) -> Result<(), String> {
        let Some(win) = xlib_window(window)? else {
            return Ok(());
        };
        let dpy = xlib_display(window)?;
        let x = xlib::Xlib::open().map_err(|e| format!("Xlib::open: {e}"))?;
        let atom = intern(&x, dpy, c"_NET_WM_USER_TIME");
        // SAFETY: dpy 는 winit 의 살아 있는 연결이고 win 은 winit 이 만든 살아 있는 창이다.
        // 프로퍼티가 없어도 XDeleteProperty 는 아무것도 안 한다.
        unsafe { (x.XDeleteProperty)(dpy, win, atom) };
        // SAFETY: 같은 연결의 요청 버퍼를 비운다.
        unsafe { (x.XFlush)(dpy) };
        Ok(())
    }
}

#[cfg(not(any(windows, target_os = "macos", target_os = "linux")))]
mod imp {
    use winit::window::Window;

    pub(super) fn show_behind(window: &Window, _anchor: Option<&Window>) -> Result<(), String> {
        window.set_visible(true);
        Ok(())
    }

    pub(super) fn clear_initial_focus_hint(_window: &Window) -> Result<(), String> {
        Ok(())
    }
}
