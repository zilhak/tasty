//! 렌더러가 읽는 전역 Theme. 첫 접근 때 Mocha로 초기화한다.
//! 설정 적용과 명시적 변경은 set_theme 또는 mutate_theme를 사용한다.

use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

use tasty_type_appearance::theme::Theme;

use crate::fallback::mocha_fallback;

/// Global theme instance. Mutable at runtime via [`set_theme`].
static THEME: LazyLock<RwLock<Theme>> = LazyLock::new(|| RwLock::new(mocha_fallback()));

/// 렌더 중 poison으로 다시 패닉하지 않도록 값을 복구하고 최초 한 번 로그를 남긴다.
/// mutate_theme의 클로저가 중간에 패닉하면 일부 변경만 적용된 값이 남을 수 있다.
pub(crate) const THEME_WHAT: &str = "the global theme";
pub(crate) static THEME_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// 현재 테마의 읽기 락. poison이면 남아 있는 값을 복구해 반환한다.
pub fn theme() -> RwLockReadGuard<'static, Theme> {
    tasty_utils::poison::recover_read(THEME.read(), THEME_WHAT, &THEME_POISON_REPORTED)
}

/// Replace the current theme at runtime. `install_global()` 가 호출.
pub fn set_theme(new_theme: Theme) {
    let mut guard =
        tasty_utils::poison::recover_write(THEME.write(), THEME_WHAT, &THEME_POISON_REPORTED);
    *guard = new_theme;
}

/// 한 쓰기 락 안에서 여러 테마 변경을 처리한다.
pub fn mutate_theme(f: impl FnOnce(&mut Theme)) {
    let mut guard =
        tasty_utils::poison::recover_write(THEME.write(), THEME_WHAT, &THEME_POISON_REPORTED);
    f(&mut guard);
}

#[cfg(test)]
mod poison_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// poison 뒤에도 값을 읽고 쓸 수 있으며 복구 보고 플래그가 설정되는지 확인한다.
    /// 이 검사 뒤에는 전역 락의 poison 상태가 같은 프로세스에 계속 남는다.
    #[test]
    fn a_poisoned_global_theme_still_renders_and_says_so() {
        let before = theme().is_light;

        let panicked = std::thread::spawn(|| {
            let _held = THEME.write().expect("not poisoned yet");
            panic!("poison the global theme");
        })
        .join();
        assert!(panicked.is_err());
        assert!(THEME.read().is_err(), "the theme lock must be poisoned now");

        assert_eq!(theme().is_light, before, "the theme value must survive");
        assert!(
            THEME_POISON_REPORTED.load(Ordering::Relaxed),
            "the poison must have been reported once"
        );

        mutate_theme(|t| t.is_light = !before);
        assert_eq!(
            theme().is_light,
            !before,
            "writes must land on a poisoned lock"
        );
        mutate_theme(|t| t.is_light = before);
    }
}
