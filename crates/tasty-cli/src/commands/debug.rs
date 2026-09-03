//! `tasty debug ...` subcommand 정의 — Debug + Tool + Popup + Extension + EventBus.

#![cfg(debug_assertions)]

use clap::Subcommand;

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Local loopback attach to a surface/workspace and mirror it (debug builds
    /// only — local self-attach simulates user-driven mirroring, so it is
    /// isolated from the release surface per the agent/user action separation
    /// policy). Remote attach is `tasty remote attach` (release).
    Attach {
        /// Target surface id (given explicitly, never inferred from focus).
        /// Mutually exclusive with `--workspace`.
        surface: Option<u32>,
        /// Target workspace id; mirrors every terminal in it as a tree.
        /// Mutually exclusive with the `surface` positional. Non-terminal
        /// surfaces are shown as placeholders.
        #[arg(long)]
        workspace: Option<u32>,
        /// Mirror-dump mode: collect output for N ms after attaching, print the
        /// mirrored screen to stdout, then exit.
        #[arg(long)]
        dump_after: Option<u64>,
        /// Input to send once right after attaching (escapes decoded: \n \r \t
        /// \xNN). For non-interactive verification.
        #[arg(long)]
        send: Option<String>,
        /// Remote surface id that receives the `--send` input in workspace mode.
        #[arg(long)]
        send_to: Option<u32>,
        /// Raw bridge mode: stdin/stdout passthrough (detach with Ctrl+\).
        #[arg(long)]
        raw: bool,
        /// Forcibly detach an occupied surface/workspace (server-side; does not
        /// attach).
        #[arg(long)]
        force_detach: bool,
    },
    /// Show debug info from the running tasty instance
    Info,
    /// Artificially block the next frame's GPU present to reproduce an event
    /// loop stall (for verifying the stall watchdog; debug builds only).
    GpuStall {
        /// How long to block, in milliseconds
        #[arg(long, default_value_t = 8000)]
        ms: u64,
    },
    /// Enable IME composition mode
    ImeEnable,
    /// Disable IME composition mode and clear preedit
    ImeDisable,
    /// Send IME preedit (composition) text
    ImePreedit {
        /// Composition text (e.g. an in-progress Hangul syllable such as U+D558)
        #[arg()]
        text: String,
        /// Cursor position within composition
        #[arg(long)]
        cursor: Option<u64>,
    },
    /// Commit IME composition text (finalize and send to terminal)
    ImeCommit {
        /// Finalized text to commit (e.g. the completed Hangul syllable U+D55C)
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
    /// Host built-in popup inspection and direct open/close (debug builds only).
    /// Forces a host popup (tools_menu / port_scanner / command_palette /
    /// remote_tool ...) open without the user-click path, for visual verification.
    #[command(subcommand)]
    HostPopup(HostPopupDebugCommands),
    /// Fullscreen stage inspection and direct enter/exit (debug builds only).
    /// Puts a stage on a window without the popup-titlebar click path, and dumps
    /// the window's fullscreen state, for visual verification of the stage.
    #[command(subcommand)]
    Fullscreen(FullscreenDebugCommands),
    /// Banner inspection and direct fire/close (debug builds only).
    /// Fires a banner without the user-action path (banners only fire from user
    /// actions in release), for visual verification of the overlay.
    #[command(subcommand)]
    Banner(BannerDebugCommands),
    /// Modifier-hint overlay hold injection + render-state dump (debug builds only).
    /// Sets the overlay's held modifier combo without a real key hold and dumps
    /// the recomputed render state (narrowed sections / delay / visibility).
    #[command(subcommand)]
    ModifierHint(ModifierHintDebugCommands),
    /// Settings modal force-open (debug builds only).
    /// Opens the settings modal without the user-action path (shortcut / button
    /// click), for visual verification of the settings UI against the design.
    #[command(subcommand)]
    Settings(SettingsDebugCommands),
    /// Arbitrary Lua injection into the host worker (debug builds only).
    /// Runs source in the isolated Lua worker (deadline-guarded, ADR-0031).
    /// Release builds have no such path — the user-input-only rule applies there.
    #[command(subcommand)]
    Lua(LuaDebugCommands),
    /// VTE sequence simulator — identical to the standalone `tasty-tui-sim`
    /// binary, run from inside the current surface (emits raw VTE to stdout, no
    /// IPC). No subcommand = interactive REPL. Use `sim flood` for a heavy
    /// full-screen redraw stress load. Debug builds only.
    Sim {
        #[command(subcommand)]
        cmd: Option<tasty_tui_simulator::Commands>,
    },
    /// Open a streaming channel and verify the server→client push path: send N
    /// data frames and expect each one echoed back (debug builds only).
    StreamEcho {
        /// Payload sent in each frame (an index suffix `#N` is appended).
        #[arg(long, default_value = "hello")]
        payload: String,
        /// Number of frames to send and expect echoed back.
        #[arg(long, default_value_t = 1)]
        count: u32,
    },
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
pub enum HostPopupDebugCommands {
    /// List all host built-in popups (id + title key)
    List,
    /// Force-open a host popup by id, centered on the focused window
    Open {
        /// Host popup id (e.g. "tools_menu", "port_scanner", "command_palette")
        #[arg(long)]
        popup_id: String,
        /// Open it scoped to the active workspace instead of the window, so the
        /// scope visibility gate can be exercised (e.g. "dag_list").
        #[arg(long)]
        workspace_scope: bool,
    },
    /// Close a host popup by id
    Close {
        /// Host popup id
        #[arg(long)]
        popup_id: String,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum FullscreenDebugCommands {
    /// List all registered fullscreen stages (id + title key)
    List,
    /// Put a stage on a window. A different stage already up is replaced; the
    /// same stage is a no-op.
    Open {
        /// Stage id (e.g. "blank", "notifications")
        #[arg(long)]
        stage_id: String,
        /// Target window. Omit only when exactly one window is open — with
        /// several, the call errors instead of guessing the focused one.
        #[arg(long)]
        window_id: Option<u64>,
    },
    /// Take the active stage off a window
    Close {
        /// Target window (see `open --window-id`)
        #[arg(long)]
        window_id: Option<u64>,
    },
    /// Dump the active stage id plus the window's fullscreen state (OS
    /// fullscreen / maximized / inner size / covered monitor)
    State {
        /// Target window (see `open --window-id`)
        #[arg(long)]
        window_id: Option<u64>,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum ModifierHintDebugCommands {
    /// Inject a modifier hold (no flags = release). `--elapsed-ms` backdates the
    /// hold timer to instantly pass the reveal-delay gate.
    Hold {
        #[arg(long)]
        ctrl: bool,
        #[arg(long)]
        alt: bool,
        #[arg(long)]
        option: bool,
        #[arg(long)]
        shift: bool,
        /// Backdate the hold timer by this many ms (skip the reveal delay).
        #[arg(long)]
        elapsed_ms: Option<u64>,
    },
    /// Dump the overlay render state (held / delay / alpha / visible / sections / header).
    State,
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum BannerDebugCommands {
    /// List built-in banner definitions + current shown/queued state
    List,
    /// Fire a banner by id into a scope
    Show {
        /// Banner id (e.g. "mouse-capture")
        #[arg(long)]
        banner_id: String,
        /// Target scope token: view | workspace:<i> | pane:<id> | tab:<pane>:<i> | surface:<id>
        #[arg(long)]
        scope: String,
    },
    /// Close a banner by id (promotes the queue head if it was shown)
    Close {
        /// Banner id
        #[arg(long)]
        banner_id: String,
    },
    /// Force the remaining countdown of a shown TTL banner in a scope
    SetCountdown {
        /// Target scope token (same format as `show --scope`)
        #[arg(long)]
        scope: String,
        /// New remaining seconds
        #[arg(long)]
        seconds: u32,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum LuaDebugCommands {
    /// Inject arbitrary Lua source and run it in the host worker (fire-and-forget).
    /// Effects are observable via logs (e.g. `tasty.log`); deadline-exceeding
    /// sources are aborted by the worker (ADR-0031).
    Eval {
        /// Lua source to execute. e.g. 'tasty.log("hi from debug")'
        #[arg()]
        source: String,
    },
    /// Read Lua source from a file and inject it (same worker path as `eval`).
    EvalFile {
        /// Path to a `.lua` file to read and execute.
        #[arg()]
        path: String,
    },
}

#[cfg(debug_assertions)]
#[derive(Subcommand)]
pub enum SettingsDebugCommands {
    /// Force-open the settings modal, optionally on a specific tab/subtab.
    Open {
        /// Initial L1 tab: general | terminal | appearance | keybindings |
        /// file_handler | misc | plugins (default: general).
        #[arg(long)]
        tab: Option<String>,
        /// Initial L2 section within the chosen tab (depends on --tab). e.g.
        /// appearance: theme|colors|general|display|tasty|terminal;
        /// keybindings: general|workspace|pane|tab|surface|clipboard|zoom|
        /// image|preset|plugins; general: general|notifications|accessibility;
        /// terminal: general|mouse_capture|tui|performance; file_handler: extension_mapping|
        /// detectors|handlers; misc: tastyrc. Unknown keys keep the tab default.
        #[arg(long)]
        subtab: Option<String>,
    },
    /// Apply a (partial) settings patch at runtime: the JSON object is
    /// deep-merged onto the live settings, then dispatched as UpdateSettings
    /// (same path as saving the settings modal — persists to config.toml).
    Apply {
        /// Settings patch as a JSON object (partial allowed). e.g.
        /// '{"general":{"workspace_categories_enabled":false}}'
        #[arg(long, conflicts_with = "file")]
        json: Option<String>,
        /// Read the JSON patch from a file instead of --json.
        #[arg(long)]
        file: Option<String>,
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
