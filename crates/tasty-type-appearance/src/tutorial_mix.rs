//! 튜토리얼 마커 발광과 주제 목록 선택 테두리의 수기 접근자. design `--tasty-tutorial-marker-glow-*`·
//! `--tasty-tutorial-topic-selected-border` 는 accent-primary 와 `transparent` 를 섞는 식이라 생성기가
//! 건너뛴다. 색은 그대로 두고 알파만 accent 알파 × 비율로 낮춘다.

use crate::color::HexColor;
use crate::state_ink::CSS_TRANSPARENT;
use crate::theme::{Theme, mix_srgb_premultiplied};

impl Theme {
    /// 마커 링 바로 밖의 발광. design `--tasty-tutorial-marker-glow-inner` = accent-primary 22% + transparent.
    #[inline]
    pub fn tutorial_marker_glow_inner(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_primary(), 22, CSS_TRANSPARENT)
    }

    /// 마커의 번지는 발광. design `--tasty-tutorial-marker-glow-outer` = accent-primary 45% + transparent.
    #[inline]
    pub fn tutorial_marker_glow_outer(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_primary(), 45, CSS_TRANSPARENT)
    }

    /// 주제 목록에서 선택한 행의 테두리. design `--tasty-tutorial-topic-selected-border` =
    /// accent-primary 40% + transparent.
    #[inline]
    pub fn tutorial_topic_selected_border(&self) -> HexColor {
        mix_srgb_premultiplied(self.accent_primary(), 40, CSS_TRANSPARENT)
    }
}
