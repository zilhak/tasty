//! Subcommand enum 도메인 분할.

pub mod agent;
pub mod debug;
pub mod memory;
pub mod plugin_cmd;
pub mod telemetry;

pub use agent::AgentCommands;
pub use debug::{
    DebugCommands, EventBusCommands, ExtensionDebugCommands, PopupDebugCommands,
    ToolDebugCommands,
};
pub use memory::{
    MemoryBbCommands, MemoryCacheCommands, MemoryCommands, MemoryPlanCommands,
    MemorySecretCommands,
};
pub use plugin_cmd::{ExtensionCommands, PluginCommands, ToolCommands};
pub use telemetry::{TelemetryAnomalyCommands, TelemetryCapCommands, TelemetryCommands};
