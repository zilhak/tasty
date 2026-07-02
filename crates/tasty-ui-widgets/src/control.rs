//! Button / IconButton 공유 size 축.
//!
//! 디자인 `ControlSize`(sm/md/lg)를 tasty Theme 토큰으로 해소한다. 색·폰트는
//! `&Theme` 메서드에서 가져온다. lg 높이(32)는 component 접근자
//! `theme.button_height_lg()` (primitive `size-32` 종착, `ui_zoom` 반영), icon
//! md 글리프(16)는 semantic `theme.icon_glyph_size_md` 로 해소한다.

use tasty_type_appearance::theme::Theme;

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
            // Sm/Md 는 control-height semantic(=button/icon-button 두 component
            // 토큰이 공히 aliasing) 을 공유 축으로 직접 읽는다. Lg(32)는 button
            // 전용 component 토큰 `button-height-lg` 로 해소한다.
            ControlSize::Sm => theme.item_height_tab.value(),
            ControlSize::Md => theme.item_height_interactive.value(),
            ControlSize::Lg => theme.button_height_lg().value(),
        }
    }

    /// 좌우 inner padding. 디자인 Button: sm=space-sm, md=space-md, lg=space-lg.
    /// (Button 전용 축 — IconButton 은 정사각이라 pad_x 를 쓰지 않는다.)
    pub fn pad_x(self, theme: &Theme) -> f32 {
        match self {
            // md 만 `button-padding-x` component 토큰 대응. sm/lg 패딩은 대응
            // component 토큰이 없어 semantic 직접(계약상 정상 경로).
            ControlSize::Sm => theme.spacing_sm.value(),
            ControlSize::Md => theme.button_padding_x().value(),
            ControlSize::Lg => theme.spacing_lg.value(),
        }
    }

    /// 라벨 폰트 크기. 디자인 Button: sm=caption(11), md/lg=body(13).
    /// (Button 전용 축.)
    pub fn font_size(self, theme: &Theme) -> f32 {
        match self {
            // sm caption 은 대응 component 토큰 없음 → semantic. md/lg body 는
            // `button-font-size` component 토큰 대응.
            ControlSize::Sm => theme.font_size_caption.value(),
            _ => theme.button_font_size().value(),
        }
    }

    /// IconButton 글리프 크기. 디자인 icon scale: sm=14, md/lg=16(기본).
    /// 글리프 크기는 semantic `icon-size-*` 로, 대응 component 토큰이 없다.
    pub fn icon_glyph(self, theme: &Theme) -> f32 {
        match self {
            ControlSize::Sm => theme.icon_glyph_size_sm.value(),
            _ => theme.icon_glyph_size_md.value(),
        }
    }
}
