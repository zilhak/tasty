#![cfg(all(target_os = "macos", feature = "gui"))]

//! macOS `NSBeep` 기반 NotificationSoundPlayer impl.

use objc2_app_kit::NSBeep;

use crate::ports::notification_sound::NotificationSoundPlayer;

pub struct MacBeepPlayer;

impl NotificationSoundPlayer for MacBeepPlayer {
    fn play(&self) {
        // AppKit 호출은 현재 이벤트 루프의 main thread에서 수행한다.
        // worker에서 호출하는 경로를 추가하면 main queue로 보내야 한다.
        NSBeep();
    }
}
