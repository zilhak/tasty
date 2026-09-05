//! `InMemoryThemeStore` — disk 우회. test 시 mocha fallback 만.

use std::sync::atomic::AtomicBool;
use std::sync::{Arc, RwLock};

use tasty_utils::poison::{recover_read, recover_write};

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::fallback::mocha_fallback;
use crate::port::ThemeStorage;
use crate::scan::ThemeEntry;
use crate::state::resolve;
use crate::store::ThemeStoreError;

/// 이 test double 의 락은 자료구조 임계구역이라 poison 을 복구한다. 조용한 복구는
/// 조용한 유실과 구분되지 않으므로 헬퍼로 첫-1 회 보고를 태운다(다른 크레이트의
/// 테스트가 이 double 을 공유하므로 poison 이 실제로 다른 스레드의 패닉일 수 있다).
const STORE_WHAT: &str = "the in-memory theme store";
static STORE_POISON_REPORTED: AtomicBool = AtomicBool::new(false);

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
        Arc::clone(&recover_read(
            self.current.read(),
            STORE_WHAT,
            &STORE_POISON_REPORTED,
        ))
    }

    fn install(&self, ctx: &dyn ThemeApplyContext) {
        let theme = resolve(ctx);
        let mut guard = recover_write(self.current.write(), STORE_WHAT, &STORE_POISON_REPORTED);
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
