#![cfg(windows)]

//! Windows `MessageBeep` 기반 NotificationSoundPlayer impl.
//!
//! windows-rs 0.61 기준 `MessageBeep` 은 `System::Diagnostics::Debug`
//! 모듈에, `MB_OK` (실제 타입 `MESSAGEBOX_STYLE`) 는
//! `UI::WindowsAndMessaging` 모듈에 위치. 두 모듈 모두 import.

use windows::Win32::System::Diagnostics::Debug::MessageBeep;
use windows::Win32::UI::WindowsAndMessaging::MB_OK;

use crate::ports::notification_sound::NotificationSoundPlayer;

pub struct WinBeepPlayer;

impl NotificationSoundPlayer for WinBeepPlayer {
    fn play(&self) {
        // SAFETY: `MessageBeep` 은 인자로 `MESSAGEBOX_STYLE` 상수만 받고 호출자
        // 메모리를 건드리지 않는 thread-safe Win32 호출(MSDN) — 지켜야 할 선행
        // 조건이 없다. 반환 `windows_core::Result<()>` 의 실패는 사운드 재생
        // 실패일 뿐 notification 발화를 막아선 안 되므로 로그 후 무시.
        if let Err(e) = unsafe { MessageBeep(MB_OK) } {
            tracing::warn!("notification sound playback failed: {e}");
        }
    }
}
