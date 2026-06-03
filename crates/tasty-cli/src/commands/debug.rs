//! `tasty debug ...` subcommand 정의 — Debug + Tool + Popup + Extension + EventBus.

#![cfg(debug_assertions)]

use clap::Subcommand;

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Show debug info from the running tasty instance
    Info,
    /// Enable IME composition mode
    ImeEnable,
    /// Disable IME composition mode and clear preedit
    ImeDisable,
    /// Send IME preedit (composition) text
    ImePreedit {
        /// Composition text (e.g. "ㅎ", "하", "한")
        #[arg()]
        text: String,
        /// Cursor position within composition
        #[arg(long)]
        cursor: Option<u64>,
    },
    /// Commit IME composition text (finalize and send to terminal)
    ImeCommit {
        /// Finalized text to commit (e.g. "한")
        #[arg()]
        text: String,
    },
    /// Show current IME status
    ImeStatus,
    /// Get cell info at a specific position
    CellInfo {
        /// Row (0-indexed)
        #[arg(long)]
        row: u64,
        /// Column (0-indexed)
        #[arg(long)]
        col: u64,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Get all cell attributes for a specific row
    ScreenAttrs {
        /// Row (0-indexed)
        #[arg(long)]
        row: u64,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Get the resolved RGBA bg/fg the renderer would push to the GPU for a cell
    GlyphColor {
        /// Row (0-indexed)
        #[arg(long)]
        row: u64,
        /// Column (0-indexed)
        #[arg(long)]
        col: u64,
        /// Surface ID (default: focused)
        #[arg(long)]
        surface: Option<u32>,
        /// Background mode: "focused" or "unfocused" (default: focused)
        #[arg(long, default_value = "focused")]
        bg_mode: String,
    },
    /// Switch macOS input source (e.g. Korean IME)
    SwitchInputSource {
        /// Input source ID (e.g. "com.apple.inputmethod.Korean.2SetKorean")
        #[arg()]
        source_id: String,
    },
    /// Send a raw physical key code via CGEvent (goes through IME pipeline)
    RawKey {
        /// macOS virtual key code (e.g. 7=KeyX, 35=KeyP, 49=Space)
        #[arg()]
        keycode: u16,
    },
    /// Event Bus inspection and injection (debug builds only)
    #[command(subcommand)]
    EventBus(EventBusCommands),
    /// Extension hook inspection and manual invocation (debug builds only)
    #[command(subcommand)]
    Extension(ExtensionDebugCommands),
    /// Tool menu inspection and invocation (debug builds only)
    #[command(subcommand)]
    Tool(ToolDebugCommands),
    /// Plugin popup inspection and open/close (debug builds only)
    #[command(subcommand)]
    Popup(PopupDebugCommands),
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum ToolDebugCommands {
    /// List all tool menu items in display order
    List,
    /// Invoke a tool menu item by key
    Invoke {
        /// Tool item key (`<plugin_id>/<tool_id>`, e.g. `com.tasty.clipboard-history/open-viewer`)
        #[arg(long)]
        key: String,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum PopupDebugCommands {
    /// List all popup contributes + currently open instances
    List,
    /// Open a plugin popup instance
    Open {
        /// Plugin id (e.g. "com.example.popper")
        #[arg(long)]
        plugin_id: String,
        /// Popup id within the plugin
        #[arg(long)]
        popup_id: String,
        /// Optional context JSON to send as the popup.open payload
        #[arg(long)]
        context: Option<String>,
    },
    /// Close a popup instance by id
    Close {
        /// Popup instance id returned by `popup open`
        #[arg(long)]
        instance_id: u64,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum ExtensionDebugCommands {
    /// Fire an extension hook manually (sends extension.invoke_hook to the
    /// specified extension and returns the response).
    InvokeHook {
        /// Extension plugin id (must be installed and running).
        #[arg(long)]
        extension_id: String,
        /// Hook kind: "event" or "ipc".
        #[arg(long)]
        kind: String,
        /// Hook phase: "pre" or "post".
        #[arg(long)]
        phase: String,
        /// Hook mode: "transform", "filter", or "observe".
        #[arg(long)]
        mode: String,
        /// Target: event key (e.g. "foo.bar") or IPC method (e.g. "codex.spawn").
        #[arg(long)]
        target: String,
        /// JSON payload to pass as the hook input (default: `{}`).
        #[arg(long, default_value = "{}")]
        payload: String,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum EventBusCommands {
    /// List plugins subscribing to the given event key
    ListSubscribers {
        /// Event key (e.g. "surface.closed")
        #[arg()]
        key: String,
    },
    /// Publish an arbitrary event from the host side
    Publish {
        /// Event key
        #[arg()]
        key: String,
        /// JSON payload (default: `{}`)
        #[arg(long, default_value = "{}")]
        payload: String,
        /// Event scope: "system" (default) or "surface"
        #[arg(long, default_value = "system")]
        scope: String,
    },
    /// Print recent envelopes with the given trace_id
    Trace {
        /// trace_id (e.g. "h2a")
        #[arg()]
        trace_id: String,
    },
}
