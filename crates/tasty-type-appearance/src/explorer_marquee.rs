//! 탐색기 영역 선택 사각형의 수기 접근자. design `--tasty-explorer-marquee-bg`·`-border` 는
//! accent-primary 와 `transparent` 를 섞는 식이라 생성기가 건너뛴다. explorer drop target 과 같이
//! 색은 그대로 두고 알파만 accent 알파 × tint 비율로 낮춘다.

use crate::color::HexColor;
use crate::state_ink::CSS_TRANSPARENT;
use crate::theme::{
    TINT_BORDER_ALPHA, TINT_FILL_ALPHA, Theme, mix_srgb_premultiplied, whole_percent,
};

impl Theme {
    /// 영역 선택 사각형의 채움. design `--tasty-explorer-marquee-bg` = accent-primary × tint-fill-alpha
    /// + transparent.
    #[inline]
    pub fn explorer_marquee_bg(&self) -> HexColor {
        mix_srgb_premultiplied(
            self.accent_primary(),
            whole_percent(TINT_FILL_ALPHA),
            CSS_TRANSPARENT,
        )
    }

    /// 영역 선택 사각형의 1px 테두리. design `--tasty-explorer-marquee-border` = accent-primary ×
    /// tint-border-alpha + transparent.
    #[inline]
    pub fn explorer_marquee_border(&self) -> HexColor {
        mix_srgb_premultiplied(
            self.accent_primary(),
            whole_percent(TINT_BORDER_ALPHA),
            CSS_TRANSPARENT,
        )
    }
}
