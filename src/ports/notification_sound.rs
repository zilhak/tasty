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
///
/// macOS gui 빌드에서는 `MacBeepPlayer` 가 주입되어 NoopPlayer 직접 사용
/// 경로 0. 그러나 BSD / Linux 미지원 OS / headless / 테스트 (e.g. ipc handler
/// image with_sound_player) 에서 fallback 으로 *cfg-분기* 사용 — 다른 환경
/// 기준 dead code 가 아님.
#[allow(dead_code)]
pub struct NoopPlayer;

impl NotificationSoundPlayer for NoopPlayer {
    fn play(&self) {}
}
