#![cfg(all(target_os = "macos", feature = "gui"))]

//! macOS `NSBeep` 기반 NotificationSoundPlayer impl.

use objc2_app_kit::NSBeep;

use crate::ports::notification_sound::NotificationSoundPlayer;

pub struct MacBeepPlayer;

impl NotificationSoundPlayer for MacBeepPlayer {
    fn play(&self) {
        // NSBeep 는 AppKit 의 main thread 호출이 권장됨. cascade 는 winit
        // event_loop main thread 에서 실행되므로 조건 충족. worker thread 발화
        // 경로가 추가되면 dispatch::main_queue 로 marshalling 필요.
        // (`objc2_app_kit::NSBeep` 자체는 safe fn — main-thread 보장은 호출
        //  컨벤션 차원이지 unsafe 마커가 강제하지 않는다.)
        NSBeep();
    }
}
