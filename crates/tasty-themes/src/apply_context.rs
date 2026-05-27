//! `ThemeApplyContext` — settings ↔ themes 어댑터 인터페이스.
//!
//! `tasty-settings::AppearanceSettings` 가 이 trait 을 구현하면 `apply_theme()` /
//! `resolve()` 가 두 레이어(`theme_base`, `theme_overrides`) + 메타데이터(theme id,
//! is_light) 에 추상적으로 접근 가능. 그 결과 `tasty-themes` 는 settings 의 구체
//! 타입을 모른 채 동작한다.

use tasty_type_appearance::theme::{PartialColors, ThemeColors};

/// 두 레이어 + 메타데이터에 접근하는 trait.
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
