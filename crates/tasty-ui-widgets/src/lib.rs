//! `tasty-ui-widgets` — 본체와 갤러리가 공유하는 egui layout / 위젯 primitive.
//!
//! `tasty-egui-theme` (색·폰트·spacing 토큰) 위에 *layout idiom* (frame + sub-tab 패널 등)
//! 을 얹는다. 본체 settings/plugins 와 갤러리가 동일 함수를 호출 → 시각 100% 동기화.
//!
//! 글로벌 `theme()` 호출 금지 — 모든 함수는 `&Theme` 을 명시적으로 받는다.
//! 본체 (`tasty`) 미의존 — 이 crate 는 본체 state 를 모른다.
//!
//! 위젯 함수는 후속 step 에서 점진적으로 추가된다.

mod horizontal_tab_bar;
mod tab_content_frame;
pub mod tokens;
mod two_depth;
pub use horizontal_tab_bar::horizontal_tab_bar_with_arrows;
pub use tab_content_frame::tab_content_frame;
pub use two_depth::two_depth_layout;
