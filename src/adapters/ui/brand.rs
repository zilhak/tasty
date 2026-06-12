//! 브랜드 정체성 색 — 테마(Mocha/Latte)와 무관한 고정값.
//!
//! 디자인 시스템 `tokens/colors.css` 의 `--brand-melon-*` 미러. 테마 전환에도
//! 바뀌지 않는 *정체성* 색이라 `Theme` 색이 아니라 const 로 둔다 (테마 색
//! 하드코딩 금지 정책의 대상이 아니다 — 이건 테마 색이 아니다).

use tasty_type_appearance::color::HexColor;

/// 수박 과육 (워드마크 `tasty.` 의 `.`, 로고 flesh).
#[allow(clippy::disallowed_methods)] // 모듈 doc 참고 — 브랜드 정체성 색은 의도된 예외.
pub const MELON_FLESH: HexColor = HexColor::from_rgb(0xf2, 0x5d, 0x6b);
