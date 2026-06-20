//! Button / IconButton 공유 size 축.
//!
//! 디자인 `ControlSize`(sm/md/lg)를 tasty Theme 토큰으로 해소한다. 색·폰트는
//! `&Theme` 메서드에서, lg(32)·icon md(16) 등 Theme 토큰에 없는 값은
//! [`crate::tokens`] const 에서 가져온다.

use tasty_type_appearance::theme::Theme;

use crate::tokens;

/// 디자인 control-height 축. md = 28(기본), sm = 24, lg = 32.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ControlSize {
    Sm,
    Md,
    Lg,
}

impl ControlSize {
    /// 컨트롤 높이(정사각 IconButton 의 한 변이기도 하다).
    pub fn height(self, theme: &Theme) -> f32 {
        match self {
            ControlSize::Sm => theme.item_height_tab.value(),
            ControlSize::Md => theme.item_height_interactive.value(),
            ControlSize::Lg => tokens::CONTROL_HEIGHT_LG,
        }
    }

    /// 좌우 inner padding. 디자인 Button: sm=space-sm, md=space-md, lg=space-lg.
    pub fn pad_x(self, theme: &Theme) -> f32 {
        match self {
            ControlSize::Sm => theme.spacing_sm.value(),
            ControlSize::Md => theme.spacing_md.value(),
            ControlSize::Lg => theme.spacing_lg.value(),
        }
    }

    /// 라벨 폰트 크기. 디자인 Button: sm=caption(11), md/lg=body(13).
    pub fn font_size(self, theme: &Theme) -> f32 {
        match self {
            ControlSize::Sm => theme.font_size_caption.value(),
            _ => theme.font_size_body.value(),
        }
    }

    /// IconButton 글리프 크기. 디자인 icon scale: sm=14, md/lg=16(기본).
    pub fn icon_glyph(self, theme: &Theme) -> f32 {
        match self {
            ControlSize::Sm => theme.icon_glyph_size_sm.value(),
            _ => tokens::ICON_GLYPH_MD,
        }
    }
}
