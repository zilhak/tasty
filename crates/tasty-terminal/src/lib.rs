mod accessors;
mod color;
mod events;
mod handle;
mod io;
mod modes;
mod mouse_report;
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
use std::sync::{Arc, Condvar, Mutex, MutexGuard, mpsc};
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
pub use io::WriteAck;
pub use mouse_report::encode_mouse_report;
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
    /// `Some` for a normally-owned PTY (Surface 터미널). `None` only after
    /// [`Terminal::take_child`] hands the waitable child off to an external owner
    /// (headless `pty_registry` exit-watcher, TODO 18) — Surface 터미널은 절대
    /// take_child 를 호출하지 않으므로 항상 `Some` 이고 kill/reap 경로가 그대로 산다.
    child: Option<Box<dyn portable_pty::Child + Send + Sync>>,
    /// PTY reader + VTE parser thread. Reads raw chunks and ingests them into the
    /// shared `TerminalState` off the input thread.
    _parser_thread: thread::JoinHandle<()>,
}

impl Drop for PtyBackend {
    /// 정상 종료 경로(surface 닫기 / 앱 quit)에서 자식 셸을 best-effort 로 종료한다.
    /// PTY master close 로 인한 HUP 에만 의존하지 않고 명시적으로 kill 한다. 이미
    /// 종료된 경우의 오류는 무시(로그만). 비정상 종료(크래시 등 Drop 미실행) 경로는
    /// [`tasty_reaper`] 의 Job Object 결박이 커버한다.
    fn drop(&mut self) {
        // take_child 로 자식이 이관된 경우(headless pty_registry) kill/reap 소유권도
        // 그쪽으로 넘어갔으므로 여기서는 아무것도 하지 않는다. Surface 터미널은 항상
        // `Some` 이라 아래 경로가 그대로 실행된다.
        let Some(child) = self.child.as_mut() else {
            return;
        };
        if let Err(e) = child.kill() {
            tracing::trace!("pty child kill on drop failed (already exited?): {e}");
        }
        // unix: portable-pty 의 kill() 은 SIGHUP 송신뿐 reap 이 없고, 살아있는 동안의
        // try_wait 폴링(process() 의 exit 감지)도 close 시점엔 더 이상 돌지 않는다 —
        // 회수하지 않으면 zombie 가 PID 테이블에 남는다 (macOS soak 실측: s9 30분
        // churn 중 16개 누적). 메인 스레드를 막지 않도록 detached thread 에서
        // 유예 poll → SIGKILL escalation 으로 reap 한다. 셸은 SIGHUP 후 보통 수 ms
        // 안에 죽으므로 정상 경로 비용은 poll 1~2회다.
        #[cfg(unix)]
        if let Some(pid) = child.process_id() {
            std::thread::spawn(move || {
                let pid = pid as i32;
                let mut status = 0i32;
                for _ in 0..40 {
                    // SAFETY: waitpid 는 본 프로세스의 자식 pid 에만 매칭된다. 이미
                    // 다른 곳에서 회수됐으면 -1(ECHILD) 로 즉시 반환된다.
                    match unsafe { libc::waitpid(pid, &raw mut status, libc::WNOHANG) } {
                        0 => std::thread::sleep(std::time::Duration::from_millis(5)),
                        _ => return, // >0 회수 완료, -1 이미 회수됨/자식 아님
                    }
                }
                // 200ms 유예 내 미종료 — SIGHUP 을 무시하는 자식. SIGKILL 은 무시
                // 불가능하므로 blocking waitpid 가 곧바로 끝난다.
                // SAFETY: kill syscall. pid 는 아직 미회수 자식(위 waitpid 가 0 반환)
                // 이므로 재사용될 수 없다.
                unsafe {
                    libc::kill(pid, libc::SIGKILL);
                }
                // SAFETY: waitpid syscall — 본 프로세스의 자식 pid 에만 매칭.
                unsafe {
                    libc::waitpid(pid, &raw mut status, 0);
                }
            });
        }
    }
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
/// Bounded capacity for each resize tap channel. Resizes are rare and coalescing
/// is acceptable (the client only needs the latest size), so a small buffer is
/// ample; a persistently full channel is treated as a live-but-behind client and
/// the message is simply dropped for that tick.
const RESIZE_TAP_CAP: usize = 8;

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
    /// PTY writer 스레드가 지금까지 flush 완료한 write 개수 + 대기자 깨우기용
    /// condvar. `enqueued_count` 와 비교해 "이 write 가 실제로 PTY 에 flush 됐는지"
    /// 를 폴링 없이 확인하는 데 쓴다([`io::WriteAck`], tell/spawn 제출 순서 보장—
    /// 본문 write 완료 전에 제출 `\r` 이 먼저 도달해 paste 휴리스틱에 먹히는 걸
    /// 막는다). detached(mirror) 터미널은 writer 스레드가 없어 절대 증가하지
    /// 않는다 — `WriteAck::wait` 는 타임아웃으로 자연 종료.
    write_progress: WriteProgress,
    /// `input_tx` 로 실제 enqueue 성공한 총 횟수. `write_progress` 와 비교해 "내
    /// write" 가 몇 번째인지 판별하는 용도 전용 — [`io::WriteAck`] 가 없으면
    /// 아무도 읽지 않는다.
    enqueued_count: u64,
    /// Server-side raw output subscribers. Each tap receives the exact raw PTY
    /// chunks (in apply order) so a remote mirror can replay them. Empty on a
    /// detached terminal and in the common no-subscriber case (zero overhead).
    output_taps: Vec<OutputTap>,
    /// Server-side resize subscribers. Each tap receives `(cols, rows)` whenever
    /// the grid actually changes so an attached client can keep its mirror grid
    /// in lockstep with the authoritative remote size. Empty in the common
    /// no-subscriber case (zero overhead).
    resize_taps: Vec<mpsc::SyncSender<(usize, usize)>>,
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
    /// 트래킹 `None → ON` 엣지에서 무장되는 "첫 마우스 캡처 안내 toast" 플래그. 호스트가
    /// `take_mouse_capture_hint()` 로 1회 소비(읽고 disarm)한다. 좌·우 클릭 중 먼저 발생한
    /// 캡처 상호작용이 소비해 세션당 1회만 안내된다 (ADR-0022 ②).
    pub(crate) mouse_capture_hint_armed: bool,
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
    /// The parser thread's wake callback, held behind a mutex so it can be
    /// re-targeted after construction. A headless PTY's Terminal is created with
    /// a waker targeting its pty id; when it is promoted to a real Surface
    /// (`pty.attach_surface`, TODO 18-c) its store key changes to the new
    /// surface_id, so the waker must be [rewired](Terminal::rewire_waker) to that
    /// id — otherwise targeted PTY polling would keep draining the stale key and
    /// the promoted terminal would appear frozen. `None` for a detached mirror.
    waker: Arc<Mutex<Waker>>,
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

/// PTY writer 스레드가 지금까지 flush 완료한 write 개수(`Mutex<u64>`) + 대기자
/// 깨우기용 condvar. [`io::WriteAck`] 가 폴링 없이 "이 write 가 실제로 flush
/// 됐는지" 를 확인하는 데 쓴다.
pub(crate) type WriteProgress = Arc<(Mutex<u64>, Condvar)>;

/// PTY writer 스레드 본체 — 큐에 들어오는 write 를 순서대로 PTY 에
/// write_all+flush 하고, 각 성공마다 `progress` 카운터를 올려 `\r` 를 별도로
/// write 로 보내는 `IpcHandler` 가 `WriteAck` 로 실제 flush 완료를 확인할 수
/// 있게 한다.
fn run_writer_loop(
    mut pty_writer: Box<dyn Write + Send>,
    write_rx: mpsc::Receiver<Vec<u8>>,
    progress: WriteProgress,
) {
    while let Ok(data) = write_rx.recv() {
        if pty_writer.write_all(&data).is_err() {
            break;
        }
        if pty_writer.flush().is_err() {
            break;
        }
        let (count, cvar) = &*progress;
        if let Ok(mut n) = count.lock() {
            *n += 1;
            cvar.notify_all();
        }
    }
}

/// PTY 자식 프로세스에 넘길 [`CommandBuilder`] 를 조립한다(shell arg/env/cwd).
/// `Terminal::new` 의 cognitive complexity 상한 때문에 뺐다 — 인라인이었을 때와
/// 동작은 동일.
fn build_shell_command(
    shell: &str,
    args: &[&str],
    surface_id: u32,
    working_dir: Option<&std::path::Path>,
) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(shell);
    // Launch as interactive login shell so .zshrc/.bashrc and themes are loaded.
    #[cfg(not(windows))]
    cmd.arg("-li");
    for arg in args {
        if !arg.is_empty() {
            cmd.arg(arg);
        }
    }
    cmd.env("TERM", "xterm-256color");
    cmd.env("TASTY_SURFACE_ID", surface_id.to_string());
    // host 가 확정한 데이터 루트를 모든 터미널 env 에 주입한다(정보성 broadcast).
    // conductor 자신이 떠 있는 이 PTY 셸이 completion-log 경로
    // (`<tasty_home>/notify/...`)를 판별하려면 부모 루트를 알아야 한다(머신에
    // ~/.tasty 와 ~/.tasty-debug 가 공존하면 어느 쪽인지 모름).
    //
    // 이 값은 `TASTY_PARENT_HOME` 으로 주입한다 — **`TASTY_HOME` 이 아니다.**
    // `TASTY_HOME` 은 tasty_home()(self-determination, override 전용)의 1순위라,
    // release 터미널 안에서 debug 빌드를 실행하면 그 debug 프로세스가 부모의
    // release 루트를 override 로 오인해 ~/.tasty-debug 격리가 깨지고 release 의
    // 포트파일을 덮어쓰는 사고가 난다. 정보성 값은 별도 이름으로 분리한다.
    if let Some(home) = tasty_utils::path::tasty_home() {
        cmd.env("TASTY_PARENT_HOME", &home);
    }

    // Remove CMUX_* environment variables so cmux CLI doesn't work inside tasty terminals.
    for (key, _) in std::env::vars() {
        if key.starts_with("CMUX_") {
            cmd.env_remove(&key);
        }
    }

    // Add tasty's own binary directory to PATH so `tasty` CLI works inside the
    // terminal. hook_handler::trigger::spawn_shell 와 동일한 보강을 공유
    // 헬퍼로 적용해 두 경로의 동작을 일치시킨다(패키징된 macOS `.app` 의
    // 최소 PATH 에서 `tasty` self 호출 해결).
    if let Some(new_path) = tasty_utils::process::path_prepending_self_dir(std::env::var_os("PATH"))
    {
        cmd.env("PATH", new_path);
    }

    if let Some(dir) = working_dir {
        cmd.cwd(dir);
    }
    cmd
}

/// PTY writer 스레드를 띄우고 `(input 채널 sender, join handle, flush 진행률
/// 카운터)` 를 반환한다. `Terminal::new` 의 cognitive complexity 상한 때문에
/// 채널/Arc 준비까지 통째로 뺐다.
fn spawn_pty_writer(
    pty_writer: Box<dyn Write + Send>,
) -> (mpsc::Sender<Vec<u8>>, thread::JoinHandle<()>, WriteProgress) {
    let write_progress: WriteProgress = Arc::new((Mutex::new(0), Condvar::new()));
    let write_progress_for_writer = Arc::clone(&write_progress);
    let (write_tx, write_rx) = mpsc::channel::<Vec<u8>>();
    let writer_thread =
        thread::spawn(move || run_writer_loop(pty_writer, write_rx, write_progress_for_writer));
    (write_tx, writer_thread, write_progress)
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
            write_progress: Arc::new((Mutex::new(0), Condvar::new())),
            enqueued_count: 0,
            output_taps: Vec::new(),
            resize_taps: Vec::new(),
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
            mouse_capture_hint_armed: false,
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

        if changed {
            self.flush_surface_change_logs();
        }
        changed
    }

    /// termwiz `Surface` 는 모든 변경을 내부 change log(`Vec<Change>`)에
    /// append-only 로 누적하고, 소비자가 `flush_changes_older_than` 으로 비워
    /// 주기를 기대하는 설계다. tasty 는 diff 스트림을 쓰지 않고 grid 셀을 직접
    /// 읽으므로(process-then-render) 이 로그의 소비자가 없다 — 비우지 않으면
    /// 출력 줄당 ~380B 가 영구 누적된다 (soak s4 실측: 5000줄 명령당 호스트
    /// RSS +1.9MB, ED3 로도 해제 불가). ingest 말미에서 전량 비운다.
    fn flush_surface_change_logs(&mut self) {
        let seq = self.primary_surface.current_seqno();
        self.primary_surface.flush_changes_older_than(seq);
        if let Some(alt) = &mut self.alternate_surface {
            let seq = alt.current_seqno();
            alt.flush_changes_older_than(seq);
        }
    }

    /// Register a server-side subscriber to this terminal's raw PTY output.
    pub(crate) fn add_output_tap(&mut self) -> mpsc::Receiver<Vec<u8>> {
        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(OUTPUT_TAP_CAP);
        self.output_taps.push(OutputTap { tx, lag: 0 });
        rx
    }

    /// Register a server-side subscriber to this terminal's grid resizes. The
    /// receiver yields `(cols, rows)` on every actual dimension change.
    pub(crate) fn add_resize_tap(&mut self) -> mpsc::Receiver<(usize, usize)> {
        let (tx, rx) = mpsc::sync_channel::<(usize, usize)>(RESIZE_TAP_CAP);
        self.resize_taps.push(tx);
        rx
    }

    /// Fan a `(cols, rows)` change out to all resize subscribers. Drops any
    /// disconnected or persistently-full subscriber (a resize is a rare, tiny
    /// message — a full channel means the client is gone).
    fn fan_out_resize(&mut self, cols: usize, rows: usize) {
        if self.resize_taps.is_empty() {
            return;
        }
        self.resize_taps.retain(|tx| {
            !matches!(
                tx.try_send((cols, rows)),
                Err(mpsc::TrySendError::Disconnected(_))
            )
        });
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
        let cmd = build_shell_command(&shell, config.args, surface_id, working_dir);

        let child = pair.slave.spawn_command(cmd)?;
        drop(pair.slave);

        // 자식 셸을 호스트 프로세스 수명에 결박한다(Windows Job Object). tasty 가
        // 크래시·taskkill /f·디버거 stop 등으로 죽어도 셸 트리가 고아로 남지 않는다.
        // 미초기화(테스트/CLI)·비-Windows 는 no-op.
        tasty_reaper::adopt_pid(child.process_id());

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
        // `write_progress` 는 [`io::WriteAck`] 가 "이 write 가 실제로 flush 됐는지"
        // 를 폴링 없이 확인하는 데 쓴다.
        let (write_tx, writer_thread, write_progress) = spawn_pty_writer(pty_writer);

        // Shared VTE state + signalling flags (ADR-0002). The writer-thread sender
        // is wired into the state so VTE responses (DSR/DA), emitted from the
        // parser thread during ingest, reach the PTY.
        let mut initial_state = TerminalState::new(cols, rows);
        initial_state.input_tx = Some(write_tx);
        initial_state.write_progress = write_progress;
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
        // Shared, rewireable waker. The parser thread reads the *current* callback
        // each wake (cloning the inner Arc out from under a brief lock, then
        // releasing before invoking), so `rewire_waker` can re-target it at
        // runtime without racing the wake path.
        let waker_holder = Arc::new(Mutex::new(waker));
        let waker_t = Arc::clone(&waker_holder);
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
                        let w = { waker_t.lock().unwrap_or_else(|p| p.into_inner()).clone() };
                        w();
                    }
                    Err(_) => break,
                }
            }
            // PTY EOF/error: signal so the next process() does an immediate alive
            // check (bypassing the throttle), and wake once more to drive it.
            eof_t.store(true, Ordering::Release);
            dirty_t.store(true, Ordering::Release);
            let w = { waker_t.lock().unwrap_or_else(|p| p.into_inner()).clone() };
            w();
        });

        let pty = PtyBackend {
            _writer_thread: writer_thread,
            pty_master: pair.master,
            child: Some(child),
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
            waker: waker_holder,
        })
    }

    /// Re-target the parser thread's wake callback. Used when a headless PTY's
    /// Terminal is re-keyed to a new `surface_id` during promotion to a real
    /// Surface (`pty.attach_surface`, TODO 18-c): the host installs a waker for
    /// the new id so targeted PTY polling drains the terminal at its new store
    /// key. Detached mirrors have no parser thread, so this is inert for them.
    pub fn rewire_waker(&self, waker: Waker) {
        *self.waker.lock().unwrap_or_else(|p| p.into_inner()) = waker;
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
            // No parser thread — a no-op waker keeps the field total.
            waker: Arc::new(Mutex::new(Arc::new(|| {}))),
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

    /// Register a server-side subscriber to this terminal's grid resizes. Yields
    /// `(cols, rows)` on each actual change — used to keep a remote mirror grid
    /// in lockstep with this authoritative terminal.
    pub fn add_resize_tap(&mut self) -> mpsc::Receiver<(usize, usize)> {
        self.lock_state().add_resize_tap()
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
