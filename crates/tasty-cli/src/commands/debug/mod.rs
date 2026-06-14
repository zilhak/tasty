//! `tasty debug ...` subcommand 정의 — Debug + Tool + Popup + Extension + EventBus.

#![cfg(debug_assertions)]

pub mod attach;

use clap::Subcommand;

#[derive(Subcommand)]
pub enum DebugCommands {
    /// Local loopback attach to a surface/workspace and mirror it (debug builds
    /// only — local self-attach simulates user-driven mirroring, so it is
    /// isolated from the release surface per the agent/user action separation
    /// policy). Remote attach is `tasty remote attach` (release).
    Attach {
        /// 대상 surface_id (포커스 비의존 — ID 직접 지정). `--workspace` 와 상호배타.
        surface: Option<u32>,
        /// 대상 workspace_id — 그 안 모든 터미널을 트리째 mirror.
        /// `surface` positional 과 상호배타. 비-터미널은 placeholder 로 숨김.
        #[arg(long)]
        workspace: Option<u32>,
        /// mirror-dump: attach 후 N ms 동안 출력 수집 → mirror 화면을 stdout 출력 후 종료.
        #[arg(long)]
        dump_after: Option<u64>,
        /// attach 직후 1 회 전송할 입력 (escape 디코딩: \n \r \t \xNN). 비대화형 검증용.
        #[arg(long)]
        send: Option<String>,
        /// workspace 모드에서 `--send` 입력을 보낼 대상 remote surface_id.
        #[arg(long)]
        send_to: Option<u32>,
        /// raw 브리지 모드: stdin/stdout passthrough (detach = Ctrl+\).
        #[arg(long)]
        raw: bool,
        /// 점유된 surface/workspace 를 강제로 끊는다 (서버 권한, attach 하지 않음).
        #[arg(long)]
        force_detach: bool,
    },
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

/// Run the `debug stream-echo` verification: connect, upgrade to a streaming
/// channel, send `count` data frames, and confirm each is echoed back by the
/// host's main loop. Returns an error on connect/handshake failure or mismatch.
///
/// This exercises the *transport infrastructure* (server→client push), not user
/// input simulation, so it lives in the debug-isolated CLI surface per the
/// agent/user action separation policy.
pub fn run_stream_echo(payload: &str, count: u32, port_file: Option<&str>) -> anyhow::Result<()> {
    use std::net::TcpStream;

    use tasty_ipc::port_file as pf;
    use tasty_ipc::stream::{STREAM_PROTO, StreamTag};

    use crate::stream::StreamConnection;

    let port = pf::read_port_file_from(port_file)?;
    let sock = TcpStream::connect(format!("127.0.0.1:{}", port)).map_err(|e| {
        anyhow::anyhow!(
            "Could not connect to tasty instance on port {}: {}. Is tasty running?",
            port,
            e
        )
    })?;

    let (mut conn, client_id) = StreamConnection::open(sock, STREAM_PROTO)?;
    println!("stream opened (client_id={client_id}, proto={STREAM_PROTO})");

    for i in 0..count {
        let msg = format!("{payload}#{i}");
        conn.send(StreamTag::Data, msg.as_bytes())?;
        let frame = conn.recv()?;
        if frame.tag != StreamTag::Data {
            anyhow::bail!("frame {i}: expected Data tag, got {:?}", frame.tag);
        }
        if frame.payload != msg.as_bytes() {
            anyhow::bail!(
                "frame {i}: echo mismatch — sent {:?}, got {:?}",
                msg,
                String::from_utf8_lossy(&frame.payload)
            );
        }
        println!(
            "echo {}/{} ok: {}",
            i + 1,
            count,
            String::from_utf8_lossy(&frame.payload)
        );
    }

    conn.detach()?;
    println!("all {count} frame(s) echoed back; detached");
    Ok(())
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
