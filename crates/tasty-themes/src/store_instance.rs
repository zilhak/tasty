//! `ThemeStore` — `ThemeStorage` 의 production instance. *전역 static* 의 instance 형식.
//!
//! 기존 `crate::global::THEME` 가 process-wide 1 개였다면, `ThemeStore` 는 Core 가
//! 보유하는 instance. 같은 *resolve / install / apply / rescan / first_run_init /
//! ensure_mocha_exists* 동작.
//!
//! 호환: *기존 free function (`global::theme()`, `state::install_global`,
//! `state::apply_theme`, etc.) 그대로 유지*. 호출처 변경은 Phase D.3.C 에서.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback;
use crate::port::ThemeStorage;
use crate::scan::ThemeEntry;
use crate::state::{apply_theme, install_global, resolve};
use crate::store::ThemeStoreError;

/// 인스턴스마다 락이 따로지만 보고 플래그는 하나로 둔다 — 첫 1 회만 남기는 것이 목적이라
/// 인스턴스별로 세어 봐야 로그가 늘 뿐이다. 임계구역은 `Arc` 교체·복제뿐이라 복구가 맞고,
/// 이 값을 읽는 쪽이 렌더 경로라 패닉은 금지다.
pub(crate) const STORE_WHAT: &str = "a theme store instance";
pub(crate) static STORE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

pub struct ThemeStore {
    current: RwLock<Arc<Theme>>,
}

impl ThemeStore {
    pub fn new() -> Self {
        Self {
            current: RwLock::new(Arc::new(mocha_fallback())),
        }
    }
}

impl Default for ThemeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeStorage for ThemeStore {
    fn current(&self) -> Arc<Theme> {
        Arc::clone(&tasty_utils::poison::recover_read(
            self.current.read(),
            STORE_WHAT,
            &STORE_POISON_REPORTED,
        ))
    }

    fn install(&self, ctx: &dyn ThemeApplyContext) {
        // resolve(ctx) — generic 함수 — &dyn 받으면 자동 동작.
        let theme = resolve(ctx);
        let mut guard = tasty_utils::poison::recover_write(
            self.current.write(),
            STORE_WHAT,
            &STORE_POISON_REPORTED,
        );
        *guard = Arc::new(theme);
        // 전역 static 호환 — Phase D.3.C 의 정리 전까지 같이 갱신.
        install_global(ctx);
    }

    fn apply(&self, ctx: &mut dyn ThemeApplyContext, id: &str) {
        apply_theme(ctx, id);
        // apply_theme 자체는 전역 static 안 만짐 — ctx 만 갱신.
        // 새 인스턴스도 install 해야 동기화. caller 가 install 한 번 더 호출하거나
        // 본 메서드 안에서 직접:
        let theme = resolve(ctx);
        let mut guard = tasty_utils::poison::recover_write(
            self.current.write(),
            STORE_WHAT,
            &STORE_POISON_REPORTED,
        );
        *guard = Arc::new(theme);
    }

    fn rescan(&self) -> Result<Vec<ThemeEntry>, ThemeStoreError> {
        crate::scan::rescan()
    }

    fn first_run_init(&self) -> Result<(), ThemeStoreError> {
        crate::store::first_run_init()
    }

    fn ensure_mocha_exists(&self) -> Result<(), ThemeStoreError> {
        crate::store::ensure_mocha_exists()
    }
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
    /// 여기 락은 인스턴스 필드라 전역을 오염시키지 않는다 — 테스트가 자기 `ThemeStore`
    /// 하나만 poison 시킨다.
    #[test]
    fn a_poisoned_store_instance_still_serves_and_says_so() {
        let store = ThemeStore::new();
        let before = store.current();

        let panicked = std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let _held = store.current.write().expect("not poisoned yet");
                    panic!("poison this store instance");
                })
                .join()
        });
        assert!(panicked.is_err());
        assert!(
            store.current.read().is_err(),
            "the instance lock must be poisoned now"
        );

        assert!(
            Arc::ptr_eq(&store.current(), &before),
            "the stored theme must survive"
        );
        assert!(
            STORE_POISON_REPORTED.load(Ordering::Relaxed),
            "the poison must have been reported once"
        );
    }
}
