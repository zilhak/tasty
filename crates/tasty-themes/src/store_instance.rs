//! `ThemeStore` — `ThemeStorage` 의 production instance. *전역 static* 의 instance 형식.
//!
//! 기존 `crate::global::THEME` 가 process-wide 1 개였다면, `ThemeStore` 는 Core 가
//! 보유하는 instance. 같은 *resolve / install / apply / rescan / first_run_init /
//! ensure_mocha_exists* 동작.
//!
//! 호환: D.3.A.4 시점에는 *기존 free function (`global::theme()`, `state::install_global`,
//! `state::apply_theme`, etc.) 그대로 유지*. 호출처 변경은 Phase D.3.C 에서.

use std::sync::{Arc, RwLock};

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback;
use crate::port::ThemeStorage;
use crate::scan::ThemeEntry;
use crate::state::{apply_theme, install_global, resolve};
use crate::store::ThemeStoreError;

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
        Arc::clone(&self.current.read().unwrap_or_else(|p| p.into_inner()))
    }

    fn install(&self, ctx: &dyn ThemeApplyContext) {
        // resolve(ctx) — generic 함수 — &dyn 받으면 자동 동작.
        let theme = resolve(ctx);
        let mut guard = self.current.write().unwrap_or_else(|p| p.into_inner());
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
        let mut guard = self.current.write().unwrap_or_else(|p| p.into_inner());
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
