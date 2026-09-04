//! 전역 `Theme` 인스턴스 — UI 코드가 `theme()` 으로 읽는다.
//!
//! `tasty-themes` 의 `apply_theme()` / `install_global()` 가 `resolve()` 결과를
//! `set_theme()` 으로 박아 넣고, 그 외 코드(특히 UI/렌더러) 는 `theme()` 으로
//! 읽기만 한다.
//!
//! `Theme` 이 `BTreeMap` 을 들고 있어서 const 생성자가 사라졌다. `RwLock` 초기값을
//! `LazyLock` 으로 감싸 첫 접근 시 `mocha_fallback()` 으로 초기화한다.

use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, RwLock, RwLockReadGuard};

use tasty_type_appearance::theme::Theme;

use crate::fallback::mocha_fallback;

/// Global theme instance. Mutable at runtime via [`set_theme`].
static THEME: LazyLock<RwLock<Theme>> = LazyLock::new(|| RwLock::new(mocha_fallback()));

/// 이 락은 **복구가 맞다** — `theme()` 이 렌더 경로에서 매 프레임 불려서 여기서 패닉하면
/// UI 가 통째로 죽는다. 다만 [`mutate_theme`] 은 호출자 클로저를 락 안에서 돌리므로
/// (두 단계 변경을 한 락에 묶는 것이 그 함수의 존재 이유다) 그 클로저가 중간에 패닉하면
/// **절반만 적용된 테마**가 남는다. 복구는 그 절반을 그대로 들고 계속 그린다 — 사용자
/// 눈에는 "색이 이상하다" 로만 보이고 다른 흔적이 없다. 그래서 관측을 붙인다.
pub(crate) const THEME_WHAT: &str = "the global theme";
pub(crate) static THEME_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

/// Get the current theme (read lock). Poisoned 락도 풀어서 반환한다 — 쓰기 측이 panic 했더라도
/// 값 자체는 읽을 수 있고, 여기서 패닉하면 렌더가 죽는다.
pub fn theme() -> RwLockReadGuard<'static, Theme> {
    tasty_utils::poison::recover_read(THEME.read(), THEME_WHAT, &THEME_POISON_REPORTED)
}

/// Replace the current theme at runtime. `install_global()` 가 호출.
pub fn set_theme(new_theme: Theme) {
    let mut guard =
        tasty_utils::poison::recover_write(THEME.write(), THEME_WHAT, &THEME_POISON_REPORTED);
    *guard = new_theme;
}

/// 전역 인스턴스를 in-place 로 mutate 한다. `apply_colors` 후 `set_is_light`
/// 같은 두 단계 변경을 락 한 번에 묶을 때 사용.
pub fn mutate_theme(f: impl FnOnce(&mut Theme)) {
    let mut guard =
        tasty_utils::poison::recover_write(THEME.write(), THEME_WHAT, &THEME_POISON_REPORTED);
    f(&mut guard);
}

#[cfg(test)]
mod poison_tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// 복구는 이 자리에 **이미** 있었다 — 이번에 더한 것은 관측뿐이라, "poison 이어도 값이
    /// 나온다" 만 보는 테스트는 헬퍼를 되돌려도 그대로 통과한다(변이가 안 죽는다). 그래서
    /// 보고 플래그가 실제로 뒤집혔는지를 함께 본다. 그 플래그가 곧 `tracing::error!` 가
    /// 나갔다는 증거이고, `unwrap_or_else(into_inner)` 로 되돌리면 `false` 로 남는다.
    ///
    /// THEME 은 process-global 이고 poison 은 sticky 라 이 테스트 뒤로 같은 바이너리의
    /// 모든 테마 읽기가 복구 경로를 지난다. 값은 보존되므로 그래도 다른 테스트가 깨지지
    /// 않는다는 것이 곧 복구가 자리잡았다는 증거다.
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
