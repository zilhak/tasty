//! 알림음 재생 인터페이스. 재생 여부는 호출자가 정한다.
//! 헤드리스에서는 NoopPlayer를 주입한다.

pub trait NotificationSoundPlayer: Send + Sync {
    /// 기본 알림음을 한 번 재생한다. 실패는 구현체가 기록하며 알림 저장을 막지 않는다.
    #[cfg_attr(
        not(feature = "gui"),
        expect(
            dead_code,
            reason = "헤드리스도 같은 trait으로 NoopPlayer를 주입하지만 play를 호출하지 않는다"
        )
    )]
    fn play(&self);
}

/// 알림음을 재생하지 않는 기본·시험용 구현.
// reason: 플랫폼과 빌드 조합에 따라 실제 사운드 어댑터로 대체된다.
#[allow(dead_code)]
pub struct NoopPlayer;

impl NotificationSoundPlayer for NoopPlayer {
    fn play(&self) {}
}
