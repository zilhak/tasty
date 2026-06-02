//! NotificationSoundPlayer port — OS level beep 재생.
//!
//! `settings.notification.sound == true` 시 cascade 가 호출. headless / 테스트
//! 빌드는 NoopPlayer 로 fallback — `feature = "gui"` 와 무관하게 port 자체는
//! 항상 존재.

#[allow(dead_code)]
pub trait NotificationSoundPlayer: Send + Sync {
    /// 시스템 기본 알림음을 1 회 재생. 사운드 재생 실패는 notification 발화
    /// 자체를 막아서는 안 되므로, 구현체는 에러를 자체 로그 후 무시한다.
    fn play(&self);
}

/// Headless / 테스트 / 기본 fallback. 호출은 받지만 아무것도 하지 않음.
pub struct NoopPlayer;

impl NotificationSoundPlayer for NoopPlayer {
    fn play(&self) {}
}
