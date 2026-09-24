//! 플러그인 관리자가 사용하는 호스트 기능의 trait.
//! 호스트가 구현하고 관리자는 Arc<dyn TraitName>으로 전달받는다.

pub mod file;
pub mod i18n;
pub mod surface;

pub use file::{
    CompletionStrategyRegistryPort, FileFormatRegistryPort, FileHandlerRegistryPort,
    HookHandlerRegistryPort,
};
pub use i18n::I18nNamespaceRegistrar;
pub use surface::SurfaceRegistry;
