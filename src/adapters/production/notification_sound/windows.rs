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
        // `MessageBeep` 은 MSDN 명시상 thread-safe. 반환은
        // `windows_core::Result<()>` — 사운드 실패가 notification 발화를
        // 막아서는 안 되므로 자체 로그 후 무시.
        if let Err(e) = unsafe { MessageBeep(MB_OK) } {
            tracing::warn!("notification sound playback failed: {e}");
        }
    }
}
