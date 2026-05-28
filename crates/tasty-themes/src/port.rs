//! `ThemeStorage` trait — Hexagonal architecture 의 *internal port*.
//!
//! `ThemeStore` (instance) 가 자체 impl. Core 가 `Arc<ThemeStore>` 또는 `Arc<dyn
//! ThemeStorage>` 보유. test 시 `testing::InMemoryThemeStore` 로 swap.
//!
//! 위치 결정: `tasty-themes` 가 internal crate 라 *trait 정의도 crate 안*.

use std::sync::Arc;

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::scan::ThemeEntry;
use crate::store::ThemeStoreError;

/// 테마 instance 의 동작 인터페이스. `&self` 만 — internal RwLock / Mutex 로 mutate.
pub trait ThemeStorage: Send + Sync {
    /// 현재 적용된 theme 의 snapshot. Arc::clone — cheap.
    fn current(&self) -> Arc<Theme>;

    /// `resolve()` 결과를 instance 의 current 로 설치.
    fn install(&self, ctx: &dyn ThemeApplyContext);

    /// id 로 테마 적용. ctx 의 base/overrides 갱신 후 install.
    fn apply(&self, ctx: &mut dyn ThemeApplyContext, id: &str);

    /// 디스크 themes 디렉토리 rescan.
    fn rescan(&self) -> Result<Vec<ThemeEntry>, ThemeStoreError>;

    /// 처음 부팅 시 사용자 디렉토리 초기화 (`~/.tasty/themes/`).
    fn first_run_init(&self) -> Result<(), ThemeStoreError>;

    /// mocha 테마 파일이 디스크에 존재하도록 보장.
    fn ensure_mocha_exists(&self) -> Result<(), ThemeStoreError>;
}
