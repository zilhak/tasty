//! `tasty terminal` subcommand — 호스트 내재화된 child-terminal 관리 (ADR-0040 /
//! occupancy-04). 에이전트가 자식 터미널을 spawn/tell/kill 하는 범용 명령.
//!
//! codex/claude 플러그인 CLI 와 달리 특정 에이전트 바이너리에 묶이지 않는다 —
//! `--command` 로 임의 명령을 띄운다. spawn 성공 시 자식은 soft 점유로 표시된다.

use clap::Subcommand;

#[derive(Subcommand)]
pub enum TerminalCommands {
    /// Spawn a child terminal in a workspace and run a command in it.
    ///
    /// The child is registered under the parent surface and marked with a soft
    /// occupancy (green border). Returns immediately — completion (idle /
    /// needs_input / exited) is reported via an agent hook wired to
    /// `terminal set-state`, not by blocking here.
    Spawn {
        /// Parent surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
        /// Target workspace (ID or name)
        #[arg(long)]
        workspace: String,
        /// Target pane ID (defaults to the first pane in the workspace)
        #[arg(long)]
        pane: Option<u32>,
        /// Working directory for the child terminal
        #[arg(long)]
        cwd: Option<String>,
        /// Command to run in the child terminal (sent as-is, submitted with Enter)
        #[arg(long)]
        command: String,
        /// Role label attached to the child (used by broadcast --role)
        #[arg(long)]
        role: Option<String>,
        /// Display nickname shown on the tab
        #[arg(long)]
        nickname: Option<String>,
    },
    /// Send a message to a child terminal (preserves line breaks, submits at end).
    ///
    /// Returns immediately.
    Tell {
        /// Message text to send. Newlines are preserved; submitted automatically.
        text: String,
        /// Target surface ID (defaults to caller's TASTY_SURFACE_ID)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// List children of the specified parent surface.
    Children {
        /// Parent surface ID (defaults to the single known parent)
        #[arg(long)]
        surface: Option<u32>,
    },
    /// Return the parent surface id for a given child surface.
    Parent {
        /// Child surface ID to look up the parent of
        #[arg(long)]
        surface: u32,
    },
    /// Kill a child terminal by child index (closes the tab, releases occupancy).
    Kill {
        /// Parent surface ID (defaults to the single known parent)
        #[arg(long)]
        surface: Option<u32>,
        /// Child index to kill
        #[arg(long)]
        child: u32,
    },
    /// Respawn a child terminal (replace PTY and/or re-run a command).
    Respawn {
        /// Parent surface ID (defaults to the single known parent)
        #[arg(long)]
        surface: Option<u32>,
        /// Child index to respawn
        #[arg(long)]
        child: u32,
        /// Override working directory (replaces the PTY at a new cwd)
        #[arg(long)]
        cwd: Option<String>,
        /// Command to re-run after respawn
        #[arg(long)]
        command: Option<String>,
        /// Replace the role label of the child
        #[arg(long)]
        role: Option<String>,
        /// Replace the display nickname of the child
        #[arg(long)]
        nickname: Option<String>,
    },
    /// Broadcast text to all children of a parent surface.
    Broadcast {
        /// Text to broadcast (sent as-is)
        text: String,
        /// Parent surface ID (defaults to the single known parent)
        #[arg(long)]
        surface: Option<u32>,
        /// Only send to children with this role label
        #[arg(long)]
        role: Option<String>,
    },
    /// Set a child's idle/needs_input/active state (called by agent hooks).
    SetState {
        /// Child surface ID
        #[arg(long)]
        surface: u32,
        /// New state: idle, needs_input, or active
        #[arg(long)]
        state: String,
    },
}
