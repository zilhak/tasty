//! 실제 파일 저장소와 메모리 테스트 저장소가 구현하는 테마 관리 인터페이스.

use std::sync::Arc;

use tasty_type_appearance::theme::Theme;

use crate::apply_context::ThemeApplyContext;
use crate::scan::ThemeEntry;
use crate::store::ThemeStoreError;

/// 구현체는 내부 락을 사용해 공유 참조로 상태를 갱신한다.
pub trait ThemeStorage: Send + Sync {
    /// 현재 테마의 Arc를 복제해 반환한다.
    fn current(&self) -> Arc<Theme>;

    /// `resolve()` 결과를 instance 의 current 로 설치.
    fn install(&self, ctx: &dyn ThemeApplyContext);

    /// id 로 테마 적용. ctx 의 base/overrides 갱신 후 install.
    fn apply(&self, ctx: &mut dyn ThemeApplyContext, id: &str);

    /// 디스크 themes 디렉토리 rescan.
    fn rescan(&self) -> Result<Vec<ThemeEntry>, ThemeStoreError>;

    /// Tasty 홈 아래의 테마 디렉터리를 초기화한다.
    fn first_run_init(&self) -> Result<(), ThemeStoreError>;

    /// mocha 테마 파일이 디스크에 존재하도록 보장.
    fn ensure_mocha_exists(&self) -> Result<(), ThemeStoreError>;
}
