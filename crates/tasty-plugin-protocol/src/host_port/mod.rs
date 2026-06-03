//! Host-side port traits — plugin manager 가 본 바이너리(엔진/파일/i18n 등)와
//! 직접 결합하지 않고 동작할 수 있도록 좁게 정의된 trait 모음.
//!
//! 각 trait 는 호스트가 구현하고, plugin 매니저는 `Arc<dyn TraitName>` 만 받는다.
//! 이를 통해 manager 와 의존 도메인을 모두 별도 crate 로 분리할 수 있다.

pub mod file;
pub mod i18n;
pub mod ipc_host;
pub mod surface;

pub use file::{FileFormatRegistryPort, FileHandlerRegistryPort};
pub use i18n::I18nNamespaceRegistrar;
pub use ipc_host::{AuditCallerMarker, AuditDecision, IpcHostFacade, SessionResolution};
pub use surface::SurfaceRegistry;
