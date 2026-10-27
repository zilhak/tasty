//! 경고 알림 배지는 밝은 테마에서 yellow 50% + white 채움과 neutral-1100 글자를 쓴다.
//! 상세 목록의 경고 글리프와 다른 강조색의 전경은 이 배지 전용 역할을 사용하지 않는다.
//! 밝은 배경 위 경고 글리프는 yellow 50% + neutral-1100 의 별도 역할을 쓴다.
//! raised 바탕 위 위험 글자와 글리프도 같은 방식으로 red 60% + neutral-1100 을 쓴다.

use crate::color::HexColor;
use crate::theme::{TEXT_ON_ACCENT_LIGHT, Theme, mix_srgb};

/// DTCG `semantic.notify-warning-fill`의 Latte color-mix 비율.
const WARNING_FILL_RATIO: f32 = 0.5;

/// DTCG `semantic.accent-warning-glyph`의 Latte color-mix 비율.
const WARNING_GLYPH_RATIO: f32 = 0.5;

/// DTCG `semantic.accent-danger-raised`의 Latte color-mix 비율.
const DANGER_RAISED_RATIO: f32 = 0.6;

/// DTCG semantic.accent-success-ink: Latte green 60% toward primitive.color-black.
const SUCCESS_INK_RATIO: f32 = 0.6;
const SUCCESS_INK_BLACK: HexColor = crate::hex!("#000000");

impl Theme {
    /// Trusted text and shield on the unchanged success tint. Other green consumers keep accent_success.
    #[inline]
    pub fn accent_success_ink(&self) -> HexColor {
        if self.is_light {
            mix_srgb(self.accent_success(), SUCCESS_INK_RATIO, SUCCESS_INK_BLACK)
        } else {
            self.accent_success()
        }
    }

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

    /// 밝은 배경 위 경고 글리프. Latte의 노랑은 3:1에 못 미쳐 neutral-1100(text) 쪽으로 섞는다.
    #[inline]
    pub fn accent_warning_glyph(&self) -> HexColor {
        if self.is_light {
            mix_srgb(
                self.accent_warning(),
                WARNING_GLYPH_RATIO,
                self.text_primary(),
            )
        } else {
            self.accent_warning()
        }
    }

    /// raised 바탕(surface-raised 카드) 위 위험 글자와 글리프. Latte의 빨강은 그 위에서
    /// 4.5:1에 못 미쳐 neutral-1100(text) 쪽으로 섞는다. 채움과 invalid 테두리는 accent-danger를 쓴다.
    #[inline]
    pub fn accent_danger_raised(&self) -> HexColor {
        if self.is_light {
            mix_srgb(
                self.accent_danger(),
                DANGER_RAISED_RATIO,
                self.text_primary(),
            )
        } else {
            self.accent_danger()
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
