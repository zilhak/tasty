//! 전역 `Theme` 인스턴스 — UI 코드가 `theme()` 으로 읽는다.
//!
//! `tasty-themes` 의 `apply_theme()` / `install_global()` 가 `resolve()` 결과를
//! `set_theme()` 으로 박아 넣고, 그 외 코드(특히 UI/렌더러) 는 `theme()` 으로
//! 읽기만 한다.

use crate::fallback::MOCHA_FALLBACK;
use tasty_type_appearance::theme::Theme;

/// Global theme instance. Mutable at runtime via [`set_theme`].
static THEME: std::sync::RwLock<Theme> = std::sync::RwLock::new(MOCHA_FALLBACK);

/// Get the current theme (read lock). Poisoned 락도 그냥 풀어서 반환한다 —
/// 쓰기 측이 panic 했더라도 읽기는 안전하다.
pub fn theme() -> std::sync::RwLockReadGuard<'static, Theme> {
    THEME
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Replace the current theme at runtime. `install_global()` 가 호출.
pub fn set_theme(new_theme: Theme) {
    let mut guard = THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *guard = new_theme;
}

/// 전역 인스턴스를 in-place 로 mutate 한다. `apply_colors` 후 `set_is_light`
/// 같은 두 단계 변경을 락 한 번에 묶을 때 사용.
pub fn mutate_theme(f: impl FnOnce(&mut Theme)) {
    let mut guard = THEME
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    f(&mut guard);
}
