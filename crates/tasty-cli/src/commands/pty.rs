//! `tasty pty` subcommand — headless PTY primitive (ADR-0050).
//!
//! 에이전트가 **Surface 없이** 백그라운드에서 굴리는 1 회성 PTY 를 다룬다. 자식
//! 터미널 *surface* 를 만드는 `tasty terminal`(ADR-0040) 과는 **별개 네임스페이스** 다 —
//! 이쪽은 GUI 에 아무것도 노출하지 않고, pty id 로만 조작한다(포커스 독립).

use clap::Subcommand;

#[derive(Subcommand)]
pub enum PtyCommands {
    /// Spawn a headless PTY (no Surface/tab). Returns a pty id.
    ///
    /// With a command, it runs immediately in the PTY's shell; without one, a
    /// bare shell is started and you drive it with `pty write`.
    Spawn {
        /// Working directory for the PTY.
        #[arg(long)]
        cwd: Option<String>,
        /// Command tokens to run on spawn (e.g. `pty spawn -- echo hi`).
        #[arg(trailing_var_arg = true)]
        command: Vec<String>,
    },
    /// Write input to a running headless PTY (sent as-is; include a newline to submit).
    Write {
        /// Target pty id.
        #[arg(long)]
        id: u32,
        /// Text to send to the PTY's stdin.
        text: String,
    },
    /// Read the current screen text of a headless PTY.
    Read {
        /// Target pty id.
        #[arg(long)]
        id: u32,
        /// Last N lines of content (default: full visible screen). Trailing blank
        /// rows are skipped and the shortfall is filled from scrollback, so N means
        /// "the last N lines actually printed", not "the bottom N grid rows".
        /// With no scrollback to draw on there is nothing to fill from, and the
        /// visible screen is the ceiling however large N is.
        #[arg(long)]
        lines: Option<usize>,
        /// Include dim (ghost-suggestion, e.g. Claude Code autocomplete overlay) cells.
        /// Default excludes them so unsubmitted UI suggestions aren't mistaken for
        /// real buffer content.
        #[arg(long)]
        show_dim: bool,
    },
    /// Poll a headless PTY's exit status (returns immediately, non-blocking).
    Wait {
        /// Target pty id.
        #[arg(long)]
        id: u32,
    },
    /// Kill a headless PTY's process and reclaim its slot.
    Kill {
        /// Target pty id.
        #[arg(long)]
        id: u32,
    },
    /// List all live headless PTYs (focus-independent — always the full set).
    List,
    /// Promote a headless PTY into a real Surface/tab under a pane. The same
    /// process/scrollback is moved (state preserved) and the pty id leaves the
    /// headless list.
    AttachSurface {
        /// Target pty id to promote.
        #[arg(long = "pty-id")]
        pty_id: u32,
        /// Pane to attach the promoted terminal to (new background tab).
        #[arg(long = "pane-id")]
        pane_id: u32,
    },
}
