#![forbid(unsafe_code)]

//! 플러그인 매니페스트의 형식과 기본 검증.
//! 파일 감지 규칙 등 호스트 타입이 필요한 추가 검증은 plugin_bridge::manifest_validate에서 한다.

pub mod gates;
pub mod host_actions;
pub mod package;
pub mod types;
pub mod validate;
pub mod validators;

#[cfg(test)]
mod tests;

pub use gates::{ContributesGate, GateToken};
pub use host_actions::{INHERITABLE_HOST_ACTIONS, is_inheritable};
pub use package::PluginPackage;
pub use types::{
    AutoWaitDecl, BannerContribute, BannerRendering, BannerScopeDecl, BannerSizeHint,
    BannerTrigger, BindingMode, CliArg, CliArgGroup, CliArgType, CliCommandDecl, CliSubcommandDecl,
    CommandDecl, CommandScope, CompletionStrategyDecl, Contributes, Entry, EventEmittedDecl,
    EventHookDecl, EventStability, ExtendsDecl, HOOK_TIMEOUT_MS_MAX, HOST_API_VERSION,
    HookEventDecl, HookMode, IpcHookDecl, IpcNamespaceDecl, MANIFEST_VERSION, Manifest,
    MenuItemDecl, Permission, PollingDecl, PopupAnchor, PopupContribute, PopupRendering,
    PopupScopeDecl, PopupSizeHint, PopupTrigger, PresetFieldDecl, PresetFieldInputType,
    SelectOptionDecl, SettingsCategory, SettingsItemDecl, SettingsPageContribute, SurfaceKindDecl,
    SurfaceKindRendering, ToolAction, ToolContribute, WindowContribute, WindowSizeHint,
};
