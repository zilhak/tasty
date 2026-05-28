//! `InMemoryThemeStore` — disk 우회. test 시 mocha fallback 만.

use std::sync::{Arc, RwLock};

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback;
use crate::port::ThemeStorage;
use crate::scan::ThemeEntry;
use crate::state::resolve;
use crate::store::ThemeStoreError;

pub struct InMemoryThemeStore {
    current: RwLock<Arc<Theme>>,
}

impl InMemoryThemeStore {
    pub fn new() -> Self {
        Self {
            current: RwLock::new(Arc::new(mocha_fallback())),
        }
    }
}

impl Default for InMemoryThemeStore {
    fn default() -> Self {
        Self::new()
    }
}

impl ThemeStorage for InMemoryThemeStore {
    fn current(&self) -> Arc<Theme> {
        Arc::clone(&self.current.read().unwrap_or_else(|p| p.into_inner()))
    }

    fn install(&self, ctx: &dyn ThemeApplyContext) {
        let theme = resolve(ctx);
        let mut guard = self.current.write().unwrap_or_else(|p| p.into_inner());
        *guard = Arc::new(theme);
    }

    fn apply(&self, _ctx: &mut dyn ThemeApplyContext, _id: &str) {
        // test stub — disk scan 안 함. ctx 갱신만 필요하면 추가.
    }

    fn rescan(&self) -> Result<Vec<ThemeEntry>, ThemeStoreError> {
        Ok(Vec::new())
    }

    fn first_run_init(&self) -> Result<(), ThemeStoreError> {
        Ok(())
    }

    fn ensure_mocha_exists(&self) -> Result<(), ThemeStoreError> {
        Ok(())
    }
}
