//! 브랜드 정체성 색·워드마크 락업의 본체 진입점.
//!
//! 실체는 위젯 크레이트 [`tasty_ui_widgets::brand`] 에 있다 — 부팅/종료 로딩
//! 화면과 그 갤러리 specimen, 사이드바 헤더가 같은 단일 출처를 쓰도록 승격했다
//! (근거·정책은 그 모듈 doc). 본체 소비처(`sidebar`, `gfx/gpu/loading`)가 기존
//! `crate::adapters::ui::brand::…` 경로를 그대로 쓰도록 여기서 재노출한다.

// 본체 소비처가 실제 쓰는 것만 재노출한다(MELON_FLESH 는 워드마크 렌더 안에서만
// 쓰여 본체 직접 소비가 없다 — 필요하면 `tasty_ui_widgets::brand::MELON_FLESH`).
// 락업 치수는 여기 없다 — `Theme` 의 `loading_screen_*` / `sidebar_*` 접근자에서 온다.
pub use tasty_ui_widgets::brand::{LOGO_PNG, LOGO_URI, draw_wordmark};
