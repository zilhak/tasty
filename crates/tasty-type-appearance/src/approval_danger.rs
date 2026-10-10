//! 에이전트 승인 위험 선택지 채움의 수기 접근자. design `--tasty-approval-danger-bg` 는
//! accent-danger 를 tint-fill 비율로 bg-panel 에 섞는 불투명 식이라 생성기가 건너뛴다.
//! 테두리·글자색(`approval_danger_border`·`approval_danger_fg`)은 생성 접근자다.

use crate::color::HexColor;
use crate::theme::{TINT_FILL_ALPHA, Theme, mix_srgb};

impl Theme {
    /// 위험 선택지 버튼 채움 = accent-danger 12% + bg-panel. 뒤에 무엇이 있든 같은 불투명 색이다.
    #[inline]
    pub fn approval_danger_bg(&self) -> HexColor {
        mix_srgb(self.accent_danger(), TINT_FILL_ALPHA, self.bg_panel())
    }
}
