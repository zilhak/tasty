//! Toast 카드 그리기 — **정본은 `tasty-ui-widgets` 다.** 여기는 재수출뿐이다.
//!
//! 예전에는 이 파일이 `ToastKind` 미러와 `accent_color` 매핑 사본, 카드 chrome 사본을
//! 들고 있었다. 사유는 "정본 크레이트(`tasty-model`)가 termwiz/터미널 모델을 끌고 온다"
//! 였는데, `ToastKind` 의 정본이 `tasty-type-appearance` 로 옮겨 가면서 그 사유가
//! 사라졌다 — 그 크레이트는 갤러리가 이미 의존하고 있고 터미널 모델을 안 끌고 온다.
//! 그래서 사본을 없애고 **본체와 같은 함수·같은 열거**를 부른다.
//!
//! 카드 한 장을 보이는 specimen(Toast · Toast stack · kb import/export 의 export 토스트)은
//! 전부 `draw_single_card` 를 부른다 — 치수와 색까지 본체 스택과 같은 함수에서 온다.
//! 위젯 크레이트 루트에서는 같은 이름이 너무 넓어 `toast_` 접두가 붙는다.

pub use tasty_type_appearance::toast_kind::ToastKind;
pub use tasty_ui_widgets::draw_toast_single_card as draw_single_card;
