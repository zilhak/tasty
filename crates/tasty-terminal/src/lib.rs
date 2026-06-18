mod accessors;
mod color;
mod events;
mod handle;
mod io;
mod modes;
mod output_buffer;
mod port;
mod port_impl;
mod resize;
mod screen;
mod scrollback;
mod snapshot;
mod vte_handler;

pub mod cwd;
pub mod disk_scrollback;
pub mod foreground_process;
pub mod search;
pub mod testing;
pub mod waker_factory;

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::thread;

use anyhow::Result;
use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};
use termwiz::cell::CellAttributes;
use termwiz::escape::Action;
use termwiz::escape::csi::CSI;
use termwiz::escape::parser::Parser;
use termwiz::surface::Surface;

pub use color::{ColorPalette, TerminalRgb};
pub use events::*;
pub use port::TerminalProcess;
pub use scrollback::ScrollbackLine;

/// Configuration for creating a new Terminal.
pub struct TerminalConfig<'a> {
    pub cols: usize,
    pub rows: usize,
    pub shell: Option<&'a str>,
    pub args: &'a [&'a str],
    pub surface_id: u32,
    pub working_dir: Option<&'a std::path::Path>,
    /// PTY master fd 에 동기적으로 미리 써넣을 바이트. writer thread 가 spawn
    /// 되기 전에 직접 write_all + flush 되므로, child shell 이 stdin 을 처음 read
    /// 하는 순간 무조건 이 바이트가 첫 입력으로 들어온다. TUI 세션 복원
    /// (`claude -r <uuid>\r`) 처럼 spawn 과 동시에 실행되어야 할 명령을 넘기는 용도.
    /// 호출자는 줄바꿈(`\r`) 등 submit 문자를 직접 포함해야 한다.
    pub initial_input: Option<&'a str>,
}

/// Information about a single cell for debug inspection.
#[derive(Debug, Clone)]
pub struct CellInfo {
    pub text: String,
    pub fg: String,
    pub bg: String,
    /// Legacy bool kept for backward compat. True iff `intensity == "bold"`.
    pub bold: bool,
    pub italic: bool,
    /// Legacy bool kept for backward compat. True iff `underline_style != "none"`.
    pub underline: bool,
    pub strikethrough: bool,
    pub inverse: bool,
    pub width: usize,
    /// "normal" | "bold" | "half" (faint/dim, SGR 2)
    pub intensity: &'static str,
    /// "none" | "single" | "double" | "curly" | "dotted" | "dashed"
    pub underline_style: &'static str,
    /// "default" | "palette:N" | "#rrggbb"
    pub underline_color: String,
    /// "none" | "slow" | "rapid"
    pub blink: &'static str,
    pub invisible: bool,
    pub overline: bool,
    /// "baseline" | "super" | "sub"
    pub vertical_align: &'static str,
}

/// PTY-bearing fields, grouped so a terminal is either fully PTY-backed
/// (`Some`) or fully detached (`None`). Detached mirror terminals own no PTY,
/// no child process, and no reader/writer threads.
///
/// 파싱은 `_parser_thread` 가 수행한다(ADR-0002). reader 스레드와 파서를 합쳐,
/// PTY raw 바이트를 읽는 즉시 그 스레드에서 `TerminalState::ingest` 로 grid 를
/// 갱신한다 — 메인(winit) 스레드는 파싱을 하지 않는다.
struct PtyBackend {
    _writer_thread: thread::JoinHandle<()>,
    pty_master: Box<dyn portable_pty::MasterPty + Send>,
    child: Box<dyn portable_pty::Child + Send + Sync>,
    /// PTY reader + VTE parser thread. Reads raw chunks and ingests them into the
    /// shared `TerminalState` off the input thread.
    _parser_thread: thread::JoinHandle<()>,
}

/// A server-side subscriber to a terminal's raw PTY output.
struct OutputTap {
    tx: mpsc::SyncSender<Vec<u8>>,
    /// Consecutive `Full` count; the tap is dropped once it exceeds the limit
    /// (a persistently slow subscriber must not pin memory or stall the pump).
    lag: u32,
}

/// Bounded capacity for each output tap channel.
const OUTPUT_TAP_CAP: usize = 1024;
/// Consecutive `Full` sends after which a slow tap is unsubscribed.
const OUTPUT_TAP_LAG_LIMIT: u32 = 64;

/// VTE 상태 머신 — surface grid · parser · modes · scrollback · output buffer ·
/// events. **파서 스레드와 메인 스레드가 `Arc<Mutex<TerminalState>>` 로 공유** 한다
/// (ADR-0002). 파서 스레드는 raw 청크마다 락을 잡아 [`TerminalState::ingest`] 를
/// 수행하고 즉시 해제하므로, 메인 스레드의 렌더/IPC/이벤트 수집은 최대 1 청크
/// 파싱 시간만 대기한다.
///
/// 필드 가시성은 기존 `Terminal` 과 동일 — crate root 에 정의되어 submodule
/// (`accessors`/`resize`/`scrollback`/`modes`/…) 의 `impl TerminalState` 에서
/// root-private 필드에 접근한다.
pub(crate) struct TerminalState {
    /// Primary screen buffer.
    pub(crate) primary_surface: Surface,
    /// Alternate screen buffer (lazily created on DECSET 1049/47).
    pub(crate) alternate_surface: Option<Surface>,
    /// Whether the alternate screen is active.
    pub(crate) use_alternate: bool,
    parser: Parser,
    /// Channel that delivers input/response bytes to the PTY writer thread (PTY
    /// terminals) or to the attach stream (detached mirror via `set_input_sink`).
    /// `None` until wired. Lives in the shared state because VTE responses
    /// (DSR/DA) are emitted from the parser thread during ingest.
    input_tx: Option<mpsc::Sender<Vec<u8>>>,
    /// Server-side raw output subscribers. Each tap receives the exact raw PTY
    /// chunks (in apply order) so a remote mirror can replay them. Empty on a
    /// detached terminal and in the common no-subscriber case (zero overhead).
    output_taps: Vec<OutputTap>,
    pub(crate) cols: usize,
    pub(crate) rows: usize,
    /// Saved cursor position for ESC 7 / ESC 8
    pub(crate) saved_cursor: Option<(usize, usize)>,
    /// Saved cursor position specifically for alternate screen enter/exit.
    pub(crate) alt_saved_cursor: Option<(usize, usize)>,
    /// Events accumulated during ingest(), consumed via take_events().
    pub(crate) events: Vec<TerminalEvent>,
    /// Raw PTY output buffer for read-mark API and ClaudeError scanner.
    output: output_buffer::OutputBuffer,
    /// DECCKM: application cursor keys mode.
    pub(crate) application_cursor_keys: bool,
    /// DECTCEM: cursor visibility.
    pub(crate) cursor_visible: bool,
    /// DECSCUSR: cursor shape (block/underline/bar + blink). The renderer reads
    /// this via `cursor_shape()`; storing it does not itself drive a redraw.
    pub(crate) cursor_shape: CursorShape,
    /// Bracketed paste mode (mode 2004).
    pub(crate) bracketed_paste: bool,
    /// Mouse tracking mode.
    pub(crate) mouse_tracking: MouseTrackingMode,
    /// SGR mouse encoding (mode 1006).
    pub(crate) sgr_mouse: bool,
    /// Focus event tracking (mode 1004).
    pub(crate) focus_tracking: bool,
    /// IRM: insert/replace mode (standard mode 4). When true, printed glyphs
    /// shift existing cells right instead of overwriting them.
    pub(crate) insert_mode: bool,
    /// Scroll region top/bottom (1-based inclusive, None = full screen).
    pub(crate) scroll_region: Option<(usize, usize)>,
    /// Whether synchronized output mode (DECSET 2026) is active.
    /// Note: changes are always applied immediately regardless of this flag.
    /// See apply_or_stage_change() for rationale.
    pub(crate) synchronized_output: bool,
    /// Scrollback buffer (memory + optional disk).
    scrollback: scrollback::Scrollback,
    /// CWD cached from OSC 7 (CurrentWorkingDirectory) sequences emitted by the shell.
    /// Used by get_cwd() to avoid spawning external processes.
    pub(crate) cached_cwd: Option<std::path::PathBuf>,
    /// Saved right-side cells for each line, preserved when cols shrink.
    saved_line_tails: Vec<Vec<(String, CellAttributes)>>,
    /// Number of scrollback lines pushed by `handle_rows_shrink` awaiting a
    /// symmetric `handle_rows_grow` to restore them.
    restorable_scrollback_count: usize,
    /// Timestamp of the most recent non-empty PTY output processed.
    last_output_at: std::time::Instant,
    /// Timestamp of the most recent user input sent to the PTY.
    last_input_at: std::time::Instant,
    /// Whether `OutputAppended` events are pushed during ingest.
    emit_output_events: bool,
    /// Mirror of the active Surface's current pen (text attributes). termwiz's
    /// `Surface` mutates its pen internally but exposes no accessor, so we track
    /// it here to support SGRs that have no `AttributeChange` variant
    /// (Overline/UnderlineColor/VerticalAlign): those are applied by cloning the
    /// pen, setting the one field, and emitting `Change::AllAttributes`. Kept in
    /// sync by `mirror_pen()` on every change. See `vte_handler/control.rs`.
    current_pen: CellAttributes,
    /// Pen mirror of the *inactive* surface while the active one is tracked by
    /// `current_pen`. termwiz keeps an independent pen per surface, so on every
    /// primary↔alternate transition we swap this with `current_pen` to keep the
    /// mirror aligned with the surface that actually receives changes — without
    /// this, an SGR applied on the alt screen would clone the primary's stale pen
    /// (e.g. leftover bold) into `cell_info`. See `swap_pen_for_surface_switch`.
    saved_pen: CellAttributes,
    /// Resolved theme palette plumbed in by the host so OSC 10/11/12/4 color
    /// *queries* report the colors the renderer actually draws. `None` until the
    /// host sets it (a query before plumbing is left unanswered). Refreshed on
    /// terminal creation and on every theme change. See `vte_handler/osc.rs`.
    pub(crate) color_palette: Option<crate::color::ColorPalette>,
    /// Last character printed to the grid, used by REP (CSI b) to repeat it.
    /// `None` until the first print; reset by RIS (full reset).
    pub(crate) last_print: Option<String>,
    /// Horizontal tab stops, indexed by column (`true` = stop at that column).
    /// Initialised to every 8th column; mutated by HTS/TBC, rebuilt to the
    /// default on resize and RIS. HT/CHT/CBT navigate between stops.
    pub(crate) tab_stops: Vec<bool>,
    /// DECSCNM (DEC private mode 5): reverse screen. When set, the renderer
    /// swaps the default foreground/background so the whole screen is inverted.
    /// Reset by RIS. Read by the host via `screen_reverse()`.
    pub(crate) reverse_screen: bool,
    /// DECOM (DEC private mode 6): origin mode. When set, absolute cursor
    /// positioning (CUP/VPA/HVP) is relative to the scroll-region top and the
    /// cursor is confined to the region. Reset by RIS and DECSTR.
    pub(crate) origin_mode: bool,
    /// Current window title (last value emitted via OSC 0/2). Tracked so the
    /// XTWINOPS title stack (CSI 22/23 t) can save and restore it.
    pub(crate) current_title: Option<String>,
    /// Saved window titles for the XTWINOPS title stack (push/pop).
    pub(crate) title_stack: Vec<Option<String>>,
    /// G0 designated as the DEC special line-drawing charset (`ESC ( 0`).
    pub(crate) charset_g0_line_drawing: bool,
    /// G1 designated as the DEC special line-drawing charset (`ESC ) 0`).
    pub(crate) charset_g1_line_drawing: bool,
    /// Whether G1 is currently invoked into GL (SO/`ESC N` selects G1, SI
    /// selects G0). When the active set is line-drawing, printed ASCII in
    /// `0x60..=0x7e` is mapped to box-drawing glyphs.
    pub(crate) charset_active_g1: bool,
}

/// PTY-backed (or detached mirror) terminal **handle**. Owns PTY I/O and the
/// parser thread, and shares the VTE state machine ([`TerminalState`]) with that
/// thread via `Arc<Mutex<_>>`. All grid/mode/scrollback accessors lock the shared
/// state; the input (winit) thread never parses (ADR-0002).
pub struct Terminal {
    /// Shared VTE state. The parser thread locks this per raw chunk to ingest;
    /// the main thread locks it for render/IPC/resize/event-drain.
    state: Arc<Mutex<TerminalState>>,
    /// PTY backend (master/child/writer-parser threads/channels). `None` for a
    /// detached mirror terminal created via [`Terminal::new_detached`], which
    /// reconstructs its grid from externally supplied bytes (`feed_bytes`).
    pty: Option<PtyBackend>,
    /// Set by the parser thread whenever it ingests a chunk; `process()` swaps it
    /// to false and reports whether anything changed since the last poll.
    dirty: Arc<AtomicBool>,
    /// Set by the parser thread on PTY EOF — forces a prompt alive check.
    parser_eof: Arc<AtomicBool>,
    /// Last known grid dimensions `(cols, rows)`, mirrored on the handle so
    /// `cols()`/`rows()` and the no-op `resize()` fast path avoid locking the
    /// shared state. The per-frame `resize_all` sweep would otherwise lock every
    /// terminal (including busy background ones) on each redraw (ADR-0002).
    cached_dims: (usize, usize),
    /// Handle-side mirror of `TerminalState::emit_output_events`, so the host's
    /// per-wake `set_output_events_enabled` (called on every targeted poll) is a
    /// lock-free no-op when the gate is unchanged — otherwise it would wait on a
    /// busy background terminal's parser lock every wake, re-serializing the input
    /// thread against parsing (ADR-0002).
    cached_emit_events: bool,
    /// Pending PTY resize: surface is updated immediately, but PTY notification
    /// is throttled to avoid SIGWINCH storms during continuous window drag.
    pending_pty_resize: Option<(usize, usize)>,
    /// Timestamp of the last actual PTY resize flush. Used for throttling.
    last_pty_flush: std::time::Instant,
    /// Timestamp of the last `check_process_alive()` syscall from `process()`.
    last_alive_check: std::time::Instant,
    /// Whether we've already emitted a ProcessExited event.
    process_exit_emitted: bool,
}

/// How long after the last PTY output a terminal still counts as busy.
pub(crate) const BUSY_OUTPUT_WINDOW: std::time::Duration = std::time::Duration::from_secs(2);

/// Maximum delay between user input and its PTY echo (echo is not "busy").
pub(crate) const INPUT_ECHO_WINDOW: std::time::Duration = std::time::Duration::from_millis(200);

/// Minimum interval between child-alive `try_wait` syscalls in `process()`.
pub(crate) const ALIVE_CHECK_INTERVAL: std::time::Duration = std::time::Duration::from_millis(500);

/// Default horizontal tab stops: a stop at column 0 and every 8th column.
pub(crate) fn default_tab_stops(cols: usize) -> Vec<bool> {
    (0..cols).map(|c| c % 8 == 0).collect()
}

impl TerminalState {
    /// Build the PTY-independent VTE state for a fresh terminal.
    fn new(cols: usize, rows: usize) -> Self {
        Self {
            primary_surface: Surface::new(cols, rows),
            alternate_surface: None,
            use_alternate: false,
            parser: Parser::new(),
            input_tx: None,
            output_taps: Vec::new(),
            cols,
            rows,
            saved_cursor: None,
            alt_saved_cursor: None,
            events: Vec::new(),
            output: output_buffer::OutputBuffer::new(),
            application_cursor_keys: false,
            cursor_visible: true,
            cursor_shape: CursorShape::default(),
            bracketed_paste: false,
            mouse_tracking: MouseTrackingMode::None,
            sgr_mouse: false,
            focus_tracking: false,
            insert_mode: false,
            scroll_region: None,
            synchronized_output: false,
            scrollback: scrollback::Scrollback::new(),
            cached_cwd: None,
            saved_line_tails: Vec::new(),
            restorable_scrollback_count: 0,
            last_output_at: std::time::Instant::now(),
            // Start in the past so the first PTY output is never mistaken for echo.
            last_input_at: std::time::Instant::now() - INPUT_ECHO_WINDOW,
            emit_output_events: false,
            current_pen: CellAttributes::default(),
            saved_pen: CellAttributes::default(),
            color_palette: None,
            last_print: None,
            tab_stops: default_tab_stops(cols),
            reverse_screen: false,
            origin_mode: false,
            current_title: None,
            title_stack: Vec::new(),
            charset_g0_line_drawing: false,
            charset_g1_line_drawing: false,
            charset_active_g1: false,
        }
    }

    /// Parse a chunk of raw VT bytes and apply it to the surface. Shared by the
    /// parser thread (PTY drain), [`Terminal::feed_bytes`] (mirror), and
    /// `process_bytes` (test injection) so all three take an identical path.
    /// Returns true if the surface changed.
    pub(crate) fn ingest(&mut self, data: &[u8]) -> bool {
        if data.is_empty() {
            return false;
        }
        self.output.append(data);
        self.last_output_at = std::time::Instant::now();
        self.fan_out_to_taps(data);

        let mut changed = false;
        let actions = self.parser.parse_as_vec(data);
        for action in actions {
            // Intercept Mode actions (DECSET/DECRST) -- they affect Terminal
            // state rather than Surface content.
            if let Action::CSI(CSI::Mode(ref mode)) = action {
                self.handle_mode(mode);
                changed = true;
                continue;
            }
            let changes = self.action_to_changes(action);
            if !changes.is_empty() {
                for change in changes {
                    self.apply_or_stage_change(change);
                }
                changed = true;
            }
        }

        changed
    }

    /// Register a server-side subscriber to this terminal's raw PTY output.
    pub(crate) fn add_output_tap(&mut self) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(OUTPUT_TAP_CAP);
        self.output_taps.push(OutputTap { tx, lag: 0 });
        rx
    }

    /// Fan a raw chunk out to all output subscribers without blocking the pump.
    fn fan_out_to_taps(&mut self, data: &[u8]) {
        if self.output_taps.is_empty() {
            return;
        }
        self.output_taps
            .retain_mut(|tap| match tap.tx.try_send(data.to_vec()) {
                Ok(()) => {
                    tap.lag = 0;
                    true
                }
                Err(mpsc::TrySendError::Full(_)) => {
                    tap.lag += 1;
                    tap.lag < OUTPUT_TAP_LAG_LIMIT
                }
                Err(mpsc::TrySendError::Disconnected(_)) => false,
            });
    }
}

impl Terminal {
    /// Create a new terminal.
    ///
    /// If `config.shell` is `None` or empty, the platform default shell is used.
    /// The `waker` callback is invoked from the parser thread whenever new data
    /// has been ingested, allowing the main event loop to wake up and render.
    pub fn new(config: TerminalConfig<'_>, waker: Waker) -> Result<Self> {
        let cols = config.cols;
        let rows = config.rows;
        let surface_id = config.surface_id;
        let working_dir = config.working_dir;
        let pty_system = NativePtySystem::default();

        let pair = pty_system.openpty(PtySize {
            rows: rows as u16,
            cols: cols as u16,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        let shell = match config.shell {
            Some(s) if !s.is_empty() => s.to_string(),
            _ => Self::default_shell(),
        };
        let mut cmd = CommandBuilder::new(&shell);
        // Launch as interactive login shell so .zshrc/.bashrc and themes are loaded.
        #[cfg(not(windows))]
        cmd.arg("-li");
        for arg in config.args {
            if !arg.is_empty() {
                cmd.arg(arg);
            }
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("TASTY_SURFACE_ID", surface_id.to_string());

        // Remove CMUX_* environment variables so cmux CLI doesn't work inside tasty terminals.
        for (key, _) in std::env::vars() {
            if key.starts_with("CMUX_") {
                cmd.env_remove(&key);
            }
        }

        // Add tasty's own binary directory to PATH so `tasty` CLI works inside the terminal
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let exe_dir_str = exe_dir.to_string_lossy();
            let sep = if cfg!(windows) { ";" } else { ":" };
            let new_path = if let Ok(existing) = std::env::var("PATH") {
                format!("{}{}{}", exe_dir_str, sep, existing)
            } else {
                exe_dir_str.to_string()
            };
            cmd.env("PATH", new_path);
        }

        if let Some(dir) = working_dir {
            cmd.cwd(dir);
        }

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        let mut pty_writer = pair.master.take_writer()?;
        let mut pty_reader = pair.master.try_clone_reader()?;

        // PTY master 의 첫 바이트로 initial_input 을 동기 write — child 가 stdin 을
        // 처음 read 하는 순간 이 바이트가 무조건 첫 입력으로 들어간다.
        if let Some(input) = config.initial_input
            && !input.is_empty()
        {
            if let Err(e) = pty_writer.write_all(input.as_bytes()) {
                tracing::warn!("initial_input write_all failed: {e}");
            } else if let Err(e) = pty_writer.flush() {
                tracing::warn!("initial_input flush failed: {e}");
            }
        }

        // Writer thread: drains queued writes to PTY without blocking the main thread.
        let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
        let writer_thread = thread::spawn(move || {
            while let Ok(data) = write_rx.recv() {
                if pty_writer.write_all(&data).is_err() {
                    break;
                }
                if pty_writer.flush().is_err() {
                    break;
                }
            }
        });

        // Shared VTE state + signalling flags (ADR-0002). The writer-thread sender
        // is wired into the state so VTE responses (DSR/DA), emitted from the
        // parser thread during ingest, reach the PTY.
        let mut initial_state = TerminalState::new(cols, rows);
        initial_state.input_tx = Some(write_tx);
        let state = Arc::new(Mutex::new(initial_state));
        let dirty = Arc::new(AtomicBool::new(false));
        let parser_eof = Arc::new(AtomicBool::new(false));

        // Parser thread: read raw PTY bytes and ingest them into the shared state
        // OFF the input thread. The lock is taken per 8KB chunk and released
        // immediately, so the main thread waits at most one chunk's parse time.
        //
        // The thread holds a *weak* ref to the state: once the `Terminal` handle
        // is dropped (surface closed), `upgrade()` fails and the thread exits
        // instead of parsing the orphaned child's output forever. This mirrors
        // the old reader-thread behaviour, where a dropped `action_rx` made the
        // next `send` fail and broke the loop — without it, a dropped terminal's
        // parser thread would burn CPU and grow memory until the child exits.
        let state_weak = Arc::downgrade(&state);
        let dirty_t = Arc::clone(&dirty);
        let eof_t = Arc::clone(&parser_eof);
        let waker_t = waker.clone();
        let parser_thread = thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match pty_reader.read(&mut buf) {
                    Ok(0) => break,
                    Ok(n) => {
                        let Some(state) = state_weak.upgrade() else {
                            // Terminal handle dropped — stop ingesting.
                            return;
                        };
                        {
                            let mut st = state.lock().unwrap_or_else(|p| p.into_inner());
                            st.ingest(&buf[..n]);
                        }
                        dirty_t.store(true, Ordering::Release);
                        waker_t();
                    }
                    Err(_) => break,
                }
            }
            // PTY EOF/error: signal so the next process() does an immediate alive
            // check (bypassing the throttle), and wake once more to drive it.
            eof_t.store(true, Ordering::Release);
            dirty_t.store(true, Ordering::Release);
            waker_t();
        });

        let pty = PtyBackend {
            _writer_thread: writer_thread,
            pty_master: pair.master,
            child,
            _parser_thread: parser_thread,
        };

        Ok(Self {
            state,
            pty: Some(pty),
            dirty,
            parser_eof,
            cached_dims: (cols, rows),
            cached_emit_events: false,
            pending_pty_resize: None,
            last_pty_flush: std::time::Instant::now(),
            // Start in the past so the first process() always checks immediately.
            last_alive_check: std::time::Instant::now() - ALIVE_CHECK_INTERVAL,
            process_exit_emitted: false,
        })
    }

    /// Create a detached mirror terminal with no PTY, child, or threads. Its grid
    /// is reconstructed purely from bytes pushed via [`Terminal::feed_bytes`].
    pub fn new_detached(cols: usize, rows: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(TerminalState::new(cols, rows))),
            pty: None,
            dirty: Arc::new(AtomicBool::new(false)),
            parser_eof: Arc::new(AtomicBool::new(false)),
            cached_dims: (cols, rows),
            cached_emit_events: false,
            pending_pty_resize: None,
            last_pty_flush: std::time::Instant::now(),
            last_alive_check: std::time::Instant::now() - ALIVE_CHECK_INTERVAL,
            process_exit_emitted: false,
        }
    }

    /// Lock the shared VTE state. Recovers from poisoning (a panicked ingest must
    /// not wedge the whole terminal).
    pub(crate) fn lock_state(&self) -> MutexGuard<'_, TerminalState> {
        self.state.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Run a closure with shared read access to the active surface. The state lock
    /// is held for the closure's duration — keep it short on the render path.
    pub fn with_surface<R>(&self, f: impl FnOnce(&Surface) -> R) -> R {
        let st = self.lock_state();
        f(st.surface())
    }

    /// Run a closure with a read-only [`RenderView`] over the shared state. Locks
    /// once for an entire terminal render (surface + scrollback + cursor/modes),
    /// keeping the parser thread's per-chunk lock window the only contention.
    pub fn with_render_view<R>(&self, f: impl FnOnce(RenderView<'_>) -> R) -> R {
        let st = self.lock_state();
        f(RenderView { state: &st })
    }

    /// Process pending terminal state. Parsing now happens on the parser thread
    /// (ADR-0002), so this only: (1) flushes a deferred PTY resize, (2) reports
    /// whether the parser ingested anything since the last call, (3) detects child
    /// exit (emitting `ProcessExited` once). Returns true if the surface changed.
    pub fn process(&mut self) -> bool {
        // Flush deferred PTY resize before reporting.
        self.force_flush_pty_resize();

        let changed = self.dirty.swap(false, Ordering::AcqRel);

        // Child exit detection (emit event once), throttled to one `try_wait`
        // syscall per ALIVE_CHECK_INTERVAL. PTY EOF forces an immediate check.
        if self.pty.is_some() && !self.process_exit_emitted {
            let reader_gone = self.parser_eof.load(Ordering::Acquire);
            if reader_gone || self.last_alive_check.elapsed() >= ALIVE_CHECK_INTERVAL {
                self.last_alive_check = std::time::Instant::now();
                if !self.check_process_alive() {
                    self.process_exit_emitted = true;
                    self.lock_state().events.push(TerminalEvent {
                        surface_id: 0,
                        kind: TerminalEventKind::ProcessExited,
                    });
                }
            }
        }

        changed
    }

    /// Feed externally supplied raw VT bytes into the parser, updating the surface
    /// as if they had arrived from a PTY. Returns true if the surface changed.
    /// Mirror ingestion path: a detached terminal has the caller supply bytes.
    pub fn feed_bytes(&mut self, data: &[u8]) -> bool {
        let changed = self.lock_state().ingest(data);
        if changed {
            self.dirty.store(true, Ordering::Release);
        }
        changed
    }

    /// Register a server-side subscriber to this terminal's raw PTY output.
    pub fn add_output_tap(&mut self) -> mpsc::Receiver<Vec<u8>> {
        self.lock_state().add_output_tap()
    }

    fn default_shell() -> String {
        #[cfg(windows)]
        {
            std::env::var("COMSPEC").unwrap_or_else(|_| "cmd.exe".to_string())
        }
        #[cfg(not(windows))]
        {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string())
        }
    }
}

/// Read-only render view over a terminal's shared [`TerminalState`], exposing
/// exactly what the GPU renderer needs while the state lock is held (see
/// [`Terminal::with_render_view`]).
pub struct RenderView<'a> {
    state: &'a TerminalState,
}

impl RenderView<'_> {
    /// Active surface (primary or alternate).
    pub fn surface(&self) -> &Surface {
        self.state.surface()
    }

    /// Active surface dimensions `(cols, rows)`.
    pub fn dimensions(&self) -> (usize, usize) {
        self.state.surface().dimensions()
    }

    /// Whether the cursor is visible (DECTCEM).
    pub fn cursor_visible(&self) -> bool {
        self.state.cursor_visible()
    }

    /// Current cursor shape (DECSCUSR). Defaults to [`CursorShape::Default`].
    pub fn cursor_shape(&self) -> CursorShape {
        self.state.cursor_shape()
    }

    /// Whether reverse-screen mode (DECSCNM) is active. The renderer swaps the
    /// default foreground/background when this is set.
    pub fn screen_reverse(&self) -> bool {
        self.state.screen_reverse()
    }

    /// Current scrollback scroll offset (0 = live bottom).
    pub fn scroll_offset(&self) -> usize {
        self.state.scroll_offset()
    }

    /// Number of scrollback lines.
    pub fn scrollback_len(&self) -> usize {
        self.state.scrollback_len()
    }

    /// Borrow a scrollback line by absolute index.
    pub fn scrollback_line(&self, index: usize) -> Option<&ScrollbackLine> {
        self.state.scrollback_line(index)
    }
}

#[cfg(test)]
mod tests;
