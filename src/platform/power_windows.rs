//! Windows 절전(suspend/resume) 감지 — `WM_POWERBROADCAST` 후킹.
//!
//! winit 의 `ApplicationHandler::suspended()`/`resumed()` 는 데스크톱에서 OS 절전
//! (S3/S0/hibernate)과 매핑되지 않으므로, 메인 윈도우 HWND 에 `SetWindowSubclass`
//! 로 서브클래스를 붙여 `WM_POWERBROADCAST` 를 가로챈다. resume 신호를 받으면
//! `AppEvent::SystemResumed` 를 이벤트 루프에 주입하고, 원 메시지는
//! `DefSubclassProc` 로 winit 의 WndProc 에 그대로 넘긴다 (ADR-0017).
//!
//! macOS / Linux 는 Unix PTY 라 절전에 강건해 이 후킹 자체가 없다 (`platform.rs`
//! 의 `#[cfg(all(windows, feature = "gui"))]`).

use std::ffi::c_void;

use winit::event_loop::EventLoopProxy;
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::UI::Shell::{DefSubclassProc, SetWindowSubclass};
use windows::Win32::UI::WindowsAndMessaging::{
    PBT_APMRESUMEAUTOMATIC, PBT_APMRESUMESUSPEND, WM_POWERBROADCAST,
};

use crate::AppEvent;

/// 서브클래스 식별자 — 같은 HWND 의 다른 서브클래스와 충돌하지 않게 고유값을 쓴다.
const SUBCLASS_ID: usize = 0x7A57_7000; // "TASTY" + power

/// `WM_POWERBROADCAST` 만 가로채고 나머지는 `DefSubclassProc` 로 위임하는 서브클래스
/// 프로시저. `dwrefdata` 로 전달된 `EventLoopProxy` 포인터를 통해 resume 이벤트를
/// 이벤트 루프에 주입한다.
unsafe extern "system" fn subclass_proc(
    hwnd: HWND,
    umsg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _uidsubclass: usize,
    dwrefdata: usize,
) -> LRESULT {
    if umsg == WM_POWERBROADCAST {
        let event = wparam.0 as u32;
        // PBT_APMRESUMEAUTOMATIC: 항상 resume 시 발사. PBT_APMRESUMESUSPEND: 사용자
        // 조작에 의한 resume 일 때 추가로 발사. 둘 중 무엇이든 한 번 처리하면 충분하나
        // resume 헬스 패스는 idempotent 하므로 둘 다 통과시켜도 무해하다.
        if event == PBT_APMRESUMEAUTOMATIC || event == PBT_APMRESUMESUSPEND {
            // SAFETY: dwrefdata 는 install_resume_hook 에서 Box::into_raw 로 leak 한
            // EventLoopProxy 포인터다. 윈도우 수명 동안 유효하며(해제하지 않음), 이
            // 콜백은 메인 스레드의 메시지 펌프에서만 호출된다.
            let proxy = unsafe { &*(dwrefdata as *const EventLoopProxy<AppEvent>) };
            crate::shortcuts::send_app_event(proxy, AppEvent::SystemResumed);
        }
    }
    // SAFETY: 서브클래스 프로시저가 처리하지 않은 메시지를 다음 핸들러(winit 의
    // WndProc)로 위임하는 표준 호출. hwnd/메시지 인자는 OS 가 전달한 유효값이다.
    unsafe { DefSubclassProc(hwnd, umsg, wparam, lparam) }
}

/// 메인 윈도우에 power-broadcast 서브클래스를 설치한다. 윈도우 생성 직후 1 회 호출.
/// 실패해도 앱 동작에는 영향이 없으므로 경고만 남긴다.
pub fn install_resume_hook(window: &winit::window::Window, proxy: EventLoopProxy<AppEvent>) {
    let hwnd = match window.window_handle() {
        Ok(h) => match h.as_raw() {
            RawWindowHandle::Win32(w) => HWND(w.hwnd.get() as *mut c_void),
            _ => {
                tracing::warn!("power hook: not a Win32 window handle");
                return;
            }
        },
        Err(e) => {
            tracing::warn!("power hook: window_handle() failed: {e}");
            return;
        }
    };

    // proxy 를 heap 에 leak 해 서브클래스 콜백이 윈도우 수명 동안 참조할 수 있게 한다.
    let refdata = Box::into_raw(Box::new(proxy)) as usize;

    // SAFETY: hwnd 는 winit 메인 윈도우의 유효한 HWND. subclass_proc 시그니처는
    // SUBCLASSPROC 와 일치한다. 실패 시 leak 한 proxy 를 회수한다.
    let ok = unsafe { SetWindowSubclass(hwnd, Some(subclass_proc), SUBCLASS_ID, refdata) };
    if !ok.as_bool() {
        // 설치 실패 — leak 한 proxy 회수.
        // SAFETY: refdata 는 바로 위에서 Box::into_raw 로 만든 포인터이고, 설치
        // 실패로 서브클래스가 소유권을 넘겨받지 못했으므로 여기서 한 번만 회수한다.
        drop(unsafe { Box::from_raw(refdata as *mut EventLoopProxy<AppEvent>) });
        tracing::warn!("power hook: SetWindowSubclass failed");
    } else {
        tracing::debug!("power hook: WM_POWERBROADCAST subclass installed");
    }
}
