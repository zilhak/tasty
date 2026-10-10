//! 탐색기 영역 선택 사각형의 수기 접근자. design `--tasty-explorer-marquee-bg`·`-border` 는
//! accent-primary 와 `transparent` 를 섞는 식이라 생성기가 건너뛴다. explorer drop target 과 같이
//! 색은 그대로 두고 알파만 낮춘다.

use crate::color::HexColor;
use crate::theme::Theme;

/// accent-primary × tint-fill-alpha(12%) 와 `transparent` 의 합성(12%×255≈31).
pub const EXPLORER_MARQUEE_BG_ALPHA: u8 = 31;

/// accent-primary × tint-border-alpha(36%) 와 `transparent` 의 합성(36%×255≈92).
pub const EXPLORER_MARQUEE_BORDER_ALPHA: u8 = 92;

impl Theme {
    /// 영역 선택 사각형의 채움. design `--tasty-explorer-marquee-bg`.
    #[inline]
    pub fn explorer_marquee_bg(&self) -> HexColor {
        self.accent_primary().with_alpha(EXPLORER_MARQUEE_BG_ALPHA)
    }

    /// 영역 선택 사각형의 1px 테두리. design `--tasty-explorer-marquee-border`.
    #[inline]
    pub fn explorer_marquee_border(&self) -> HexColor {
        self.accent_primary()
            .with_alpha(EXPLORER_MARQUEE_BORDER_ALPHA)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::theme::{TINT_BORDER_ALPHA, TINT_FILL_ALPHA};

    /// 두 알파는 tint-fill·tint-border 계수를 바이트로 반올림한 값이다.
    #[test]
    fn marquee_alphas_follow_the_tint_fractions() {
        assert_eq!(
            EXPLORER_MARQUEE_BG_ALPHA,
            (TINT_FILL_ALPHA * 255.0).round() as u8
        );
        assert_eq!(
            EXPLORER_MARQUEE_BORDER_ALPHA,
            (TINT_BORDER_ALPHA * 255.0).round() as u8
        );
    }
}
