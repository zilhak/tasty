//! Tasty plugin manifest schema, parse and schema-agnostic validation.
//!
//! 본 crate 는 `tasty-plugin.toml` 의 schema + 파서 + 기본 검증 (id 형식, 중복,
//! permission 매칭 등) 만 제공한다. concrete file::format / file::handler 결합이
//! 필요한 추가 검증 (detector rule schema 등) 은 호스트 본 바이너리의
//! `plugin_bridge::manifest_validate` 가 담당.

pub mod package;
pub mod types;
pub mod validate;
pub mod validators;

#[cfg(test)]
mod tests;

pub use package::PluginPackage;
pub use types::{
    AutoWaitDecl, BindingMode, CliArg, CliArgGroup, CliArgType, CliCommandDecl, CliSubcommandDecl,
    CommandDecl, CommandScope, Contributes, Entry, EventEmittedDecl, EventHookDecl, EventStability,
    ExtendsDecl, HOOK_TIMEOUT_MS_MAX, HOST_API_VERSION, HookMode, IpcHookDecl, IpcNamespaceDecl,
    MANIFEST_VERSION, Manifest, MenuItemDecl, Permission, PollingDecl, PopupAnchor,
    PopupContribute, PopupSizeHint, PopupTrigger, SettingsCategory, SettingsItemDecl,
    SettingsPageContribute, SurfaceKindDecl, SurfaceKindRendering, ToolAction, ToolContribute,
    WindowContribute, WindowSizeHint,
};
