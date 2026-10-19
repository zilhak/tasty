//! 경고 알림 배지는 밝은 테마에서 yellow 50% + white 채움과 neutral-1100 글자를 쓴다.
//! 상세 목록의 경고 글리프와 다른 강조색의 전경은 이 배지 전용 역할을 사용하지 않는다.

use crate::color::HexColor;
use crate::theme::{TEXT_ON_ACCENT_LIGHT, Theme, mix_srgb};

/// DTCG `semantic.notify-warning-fill`의 Latte color-mix 비율.
const WARNING_FILL_RATIO: f32 = 0.5;

impl Theme {
    /// 경고 배지 채움. Mocha의 원래 경고색은 유지한다.
    #[inline]
    pub fn notify_warning_fill(&self) -> HexColor {
        if self.is_light {
            mix_srgb(
                self.accent_warning(),
                WARNING_FILL_RATIO,
                TEXT_ON_ACCENT_LIGHT,
            )
        } else {
            self.accent_warning()
        }
    }

    /// 경고 배지 안의 숫자와 글리프에 함께 쓰는 전경색.
    #[inline]
    pub fn notify_warning_fg(&self) -> HexColor {
        if self.is_light {
            self.text_primary()
        } else {
            self.text_on_accent()
        }
    }
}
