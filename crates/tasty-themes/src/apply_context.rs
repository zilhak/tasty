//! 구체적인 설정 타입에 의존하지 않고 기본 색·사용자 변경분·테마 ID·밝기 모드에 접근한다.

use tasty_type_appearance::theme::{PartialColors, ThemeColors};

/// 테마 적용에 필요한 설정 접근 인터페이스.
pub trait ThemeApplyContext {
    fn theme_id(&self) -> &str;
    fn set_theme_id(&mut self, id: &str);

    fn theme_base(&self) -> &ThemeColors;
    fn theme_base_mut(&mut self) -> &mut ThemeColors;

    fn theme_overrides(&self) -> &PartialColors;
    fn theme_overrides_mut(&mut self) -> &mut PartialColors;

    fn theme_is_light(&self) -> bool;
    fn set_theme_is_light(&mut self, v: bool);
}
