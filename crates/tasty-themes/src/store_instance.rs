//! Core가 소유할 수 있는 ThemeStorage 구현. 각 인스턴스가 현재 Theme를 보관한다.
//! install은 호환용 전역 테마도 갱신하지만 apply는 설정과 이 인스턴스만 갱신한다.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback;
use crate::port::ThemeStorage;
use crate::scan::ThemeEntry;
use crate::state::{apply_theme, install_global, resolve};
use crate::store::ThemeStoreError;

/// Arc 복제·교체 중 poison이 생기면 복구한다. 로그는 모든 인스턴스를 통틀어 최초 한 번 남긴다.
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
        let theme = resolve(ctx);
        let mut guard = tasty_utils::poison::recover_write(
            self.current.write(),
            STORE_WHAT,
            &STORE_POISON_REPORTED,
        );
        *guard = Arc::new(theme);
        // 기존 전역 테마를 읽는 호출자와도 맞춘다.
        install_global(ctx);
    }

    fn apply(&self, ctx: &mut dyn ThemeApplyContext, id: &str) {
        apply_theme(ctx, id);
        // 설정 변경 결과를 이 인스턴스에 반영한다.
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

    /// 이 인스턴스만 poison 상태로 만들어 값 보존과 최초 복구 보고를 확인한다.
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
