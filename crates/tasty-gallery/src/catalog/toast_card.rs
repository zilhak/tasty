//! Toast 카드 그리기 — **정본은 `tasty-ui-widgets` 다.** 여기는 재수출뿐이다.
//!
//! 예전에는 이 파일이 `ToastKind` 미러와 `accent_color` 매핑 사본, 카드 chrome 사본을
//! 들고 있었다. 사유는 "정본 크레이트(`tasty-model`)가 termwiz/터미널 모델을 끌고 온다"
//! 였는데, `ToastKind` 의 정본이 `tasty-type-appearance` 로 옮겨 가면서 그 사유가
//! 사라졌다 — 그 크레이트는 갤러리가 이미 의존하고 있고 터미널 모델을 안 끌고 온다.
//! 그래서 사본을 없애고 **본체와 같은 함수·같은 열거**를 부른다.
//!
//! 재수출 이름을 옛 이름 그대로 둔 것은 호출부(kb import/export specimen)가
//! `toast_card::draw_card` 로 부르고 있어서다 — 위젯 크레이트 루트에서는 같은 이름이 너무
//! 넓어 `toast_` 접두가 붙는다. 단일 카드 specimen 은 치수·색까지 공용인
//! `draw_single_card` 를 부른다.

pub use tasty_type_appearance::toast_kind::ToastKind;
pub use tasty_ui_widgets::tokens::{
    TOAST_ACCENT_BAR_WIDTH as ACCENT_BAR_WIDTH, TOAST_PADDING_X as PADDING_X,
    TOAST_PADDING_Y as PADDING_Y,
};
pub use tasty_ui_widgets::{
    ToastCardColors as CardColors, draw_toast_card as draw_card,
    draw_toast_single_card as draw_single_card, toast_accent_color as accent_color,
};
