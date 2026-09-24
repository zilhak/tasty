//! 파일 입출력 없이 Mocha에서 시작하는 테스트용 테마 저장소.

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

/// 다른 스레드의 패닉으로 poison이 생겨도 값을 복구하며 최초 한 번 로그를 남긴다.
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
        // 이 테스트 저장소는 디스크 조회나 테마 ID 적용을 실행하지 않는다.
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
