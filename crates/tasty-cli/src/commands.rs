//! Subcommand enum 도메인 분할.

pub mod agent;
pub mod approval;
pub mod attach;
pub mod clipboard;
pub mod debug;
pub mod file_handler;
pub mod list_set;
pub mod memory;
pub mod move_send;
pub mod new_close;
pub mod output;
pub mod plugin_cmd;
pub mod port;
pub mod preset;
pub mod read_unset;
pub mod surface_meta;
pub mod telemetry;
pub mod update;

pub use agent::AgentCommands;
pub use approval::{ApprovalCommands, ApprovalSummaryCommands};
pub use clipboard::ClipboardCommands;
#[cfg(debug_assertions)]
pub use debug::{
    DebugCommands, EventBusCommands, ExtensionDebugCommands, PopupDebugCommands, ToolDebugCommands,
};
pub use file_handler::{FileHandlerCommands, ScriptCommands};
pub use list_set::{ListCommands, SetCommands};
pub use memory::{
    MemoryBbCommands, MemoryCacheCommands, MemoryCommands, MemoryPlanCommands, MemorySecretCommands,
};
pub use move_send::{MoveCommands, SendCommands};
pub use new_close::{CloseCommands, NewCommands};
pub use output::{OutputCommands, OutputObserveCommands};
pub use plugin_cmd::{ExtensionCommands, PluginCommands, ToolCommands};
pub use preset::PresetCommands;
pub use read_unset::{ReadCommands, UnsetCommands};
pub use surface_meta::SurfaceMetaCommands;
pub use telemetry::{TelemetryAnomalyCommands, TelemetryCapCommands, TelemetryCommands};
pub use update::UpdateOpts;
