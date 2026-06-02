//! Plugin 매니페스트 정의 + 파싱 + 검증.
//!
//! `~/.tasty/plugins/<plugin-id>/tasty-plugin.toml` 형식.
//!
//! 일부 필드(authors/homepage/contributes/icon 등)는 deserialize surface로 정의돼
//! 있지만 호스트 본문이 아직 모두 활용하지는 않는다 — 매니페스트 schema를 한 곳에서
//! 정확히 표현하기 위해 의도적으로 남겨둔다.
//!
//! 본 모듈은 sub-module 로만 분할되어 있고, 외부에서는 기존과 동일한
//! `crate::plugin::manifest::<Type>` 경로로 사용 가능하도록 `pub use` 로 재노출한다.
#![allow(dead_code)]

mod package;
#[cfg(test)]
mod tests;
mod types;
mod validate;
mod validators;

pub use package::PluginPackage;
pub use types::{
    BindingMode, CliArg, CliArgGroup, CliArgType, CliCommandDecl, CliSubcommandDecl, CommandDecl,
    CommandScope, Contributes, Entry, EventEmittedDecl, EventHookDecl, EventStability, ExtendsDecl,
    HOOK_TIMEOUT_MS_MAX, HOST_API_VERSION, HookMode, IpcHookDecl, IpcNamespaceDecl,
    MANIFEST_VERSION, Manifest, MenuItemDecl, Permission, PopupAnchor, PopupContribute,
    PopupSizeHint, PopupTrigger, SurfaceKindDecl, SurfaceKindRendering, ToolAction, ToolContribute,
    WindowContribute, WindowSizeHint,
};
