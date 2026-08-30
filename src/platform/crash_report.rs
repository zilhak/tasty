#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

use std::backtrace::Backtrace;
use std::fs;
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::time::SystemTime;

use tracing_subscriber::EnvFilter;

use crate::paths::tasty_home;

/// Return the crash report directory: `~/.tasty/crash-reports/`
fn crash_report_dir() -> Option<PathBuf> {
    tasty_home().map(|dir| dir.join("crash-reports"))
}

/// Format a `SystemTime` as `YYYY-MM-DDTHH-MM-SS` without external crates.
fn format_timestamp(time: SystemTime) -> String {
    let duration = time
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();

    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    let (year, month, day) = days_to_date(days);

    format!(
        "{:04}-{:02}-{:02}T{:02}-{:02}-{:02}",
        year, month, day, hours, minutes, seconds
    )
}

fn days_to_date(days: u64) -> (u64, u64, u64) {
    // Algorithm from http://howardhinnant.github.io/date_algorithms.html
    let days = days + 719_468;
    let era = days / 146_097;
    let doe = days % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}

/// Write a crash report file and return the path on success.
fn write_crash_report(info: &panic::PanicHookInfo<'_>, backtrace: &Backtrace) -> Option<PathBuf> {
    let dir = crash_report_dir()?;
    fs::create_dir_all(&dir).ok()?;

    let timestamp = format_timestamp(SystemTime::now());
    let path = dir.join(format!("crash-{timestamp}.log"));

    let mut file = fs::File::create(&path).ok()?;

    writeln!(file, "=== Tasty Crash Report ===").ok();
    writeln!(
        file,
        "Timestamp: {}",
        timestamp.replace('T', " ").replace('-', ":")
    )
    .ok();
    writeln!(file, "Version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(
        file,
        "OS: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .ok();
    writeln!(file).ok();

    writeln!(file, "=== Panic ===").ok();
    if let Some(location) = info.location() {
        writeln!(file, "Location: {}:{}", location.file(), location.line()).ok();
    }
    if let Some(msg) = info.payload().downcast_ref::<&str>() {
        writeln!(file, "Message: {msg}").ok();
    } else if let Some(msg) = info.payload().downcast_ref::<String>() {
        writeln!(file, "Message: {msg}").ok();
    } else {
        writeln!(file, "Message: <unknown>").ok();
    }
    writeln!(file, "Display: {info}").ok();
    writeln!(file).ok();

    writeln!(file, "=== Backtrace ===").ok();
    writeln!(file, "{backtrace}").ok();

    Some(path)
}

/// 이벤트 루프 stall 리포트를 `~/.tasty/crash-reports/hang-<ts>.log` 로 남기고 경로를 돌려준다.
///
/// panic 리포트와 같은 디렉토리를 쓰는 이유: 사용자가 "앱이 멎었다" 를 겪은 뒤 실제로
/// 들여다보는 곳이 거기다. 행(hang)은 panic 이 아니라 hook 이 발동하지 않으므로, 그
/// 디렉토리가 비어 있으면 "아무 일도 없었다" 로 오독된다.
///
/// 공유 로그(`debug.log`)가 아니라 별도 파일인 이유: 그 로그는 프로세스마다 시작 시
/// truncate 되므로, 행 상태에서 `tasty` CLI 가 한 번이라도 실행되면 증거가 지워진다.
pub fn write_hang_report(site: &str, phase: &str, stuck_ms: u64) -> Option<PathBuf> {
    let dir = crash_report_dir()?;
    fs::create_dir_all(&dir).ok()?;

    let timestamp = format_timestamp(SystemTime::now());
    let path = dir.join(format!("hang-{timestamp}.log"));
    let mut file = fs::File::create(&path).ok()?;

    writeln!(file, "=== Tasty Hang Report ===").ok();
    writeln!(
        file,
        "Timestamp: {}",
        timestamp.replace('T', " ").replace('-', ":")
    )
    .ok();
    writeln!(file, "Version: {}", env!("CARGO_PKG_VERSION")).ok();
    writeln!(
        file,
        "OS: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .ok();
    writeln!(file).ok();

    writeln!(file, "=== Stall ===").ok();
    writeln!(file, "Callback: {site}").ok();
    writeln!(file, "Render phase: {phase}").ok();
    writeln!(file, "Stuck for: {stuck_ms} ms").ok();
    writeln!(file).ok();
    writeln!(
        file,
        "The winit event-loop callback above did not return within the watchdog threshold.\n\
         While it is blocked, keyboard, mouse and IPC are all unprocessed — the window looks\n\
         frozen even though the process is alive and no panic occurred.\n\
         A render phase of `present`/`submit`/`acquire` points at the GPU driver, not at tasty\n\
         logic: those calls have no application-level timeout and cannot be cancelled."
    )
    .ok();

    Some(path)
}

/// Initialize crash reporting and tracing.
///
/// - **All builds**: Installs a panic hook that writes crash reports to `~/.tasty/crash-reports/`.
///   Initializes tracing with stderr output, plus a file layer under `~/.tasty/` (independent of
///   the stderr `TASTY_LOG` filter — see `init_tracing`).
pub fn init() {
    // Install panic hook (always, no runtime cost until panic)
    panic::set_hook(Box::new(|info| {
        let backtrace = Backtrace::force_capture();

        if let Some(path) = write_crash_report(info, &backtrace) {
            eprintln!("Tasty crashed! Report saved to: {}", path.display());
        }

        eprintln!("panic: {info}");
        eprintln!("{backtrace}");
    }));

    // Initialize tracing
    init_tracing();
}

fn make_env_filter() -> EnvFilter {
    EnvFilter::try_from_env("TASTY_LOG").unwrap_or_else(|_| {
        EnvFilter::new("warn,wgpu_hal=error,wgpu_core=error,naga=error,egui_winit::clipboard=off")
    })
}

/// stderr + file tracing, all build modes. stderr 필터는 `make_env_filter()`
/// (`TASTY_LOG`, 기본 warn) 를 따르지만, 파일 필터는 그와 독립적으로 고정된다 —
/// dev 는 `debug-dev.log` 에 `debug` 레벨(기존 동작 유지), release/dist 는
/// `debug.log` 에 `warn` 이상만 남긴다(디스크 사용량 제한 — attach disconnect 같은
/// 진단 가치가 있는 로그만 release 사용자 환경에 보존하는 게 목적이라 전체 debug
/// 상시 로깅까진 필요 없다). 두 모드 모두 매 실행마다 파일을 truncate 한다(기존
/// dev 동작과 동일 — rotation 은 이번 스코프 밖).
fn init_tracing() {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let stderr_layer = tracing_subscriber::fmt::layer().with_filter(make_env_filter());

    let registry = tracing_subscriber::registry().with(stderr_layer);

    // Try to set up file logging; fall back to stderr-only if it fails
    if let Some(dir) = tasty_home() {
        // 로거 초기화 이전이라 tracing 사용 불가. 디렉토리 생성 실패는 아래
        // fs::File::create가 실패하면서 자연스럽게 fall back 경로로 진입한다.
        if let Err(e) = fs::create_dir_all(&dir) {
            eprintln!("tasty: failed to create log dir {}: {e}", dir.display());
        }
        let (log_filename, file_filter) = if cfg!(debug_assertions) {
            (
                "debug-dev.log",
                EnvFilter::new("debug,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
            )
        } else {
            (
                "debug.log",
                EnvFilter::new("warn,wgpu_hal=warn,wgpu_core=warn,naga=warn"),
            )
        };
        let log_path = dir.join(log_filename);
        if let Ok(file) = fs::File::create(&log_path) {
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_filter(file_filter);

            registry.with(file_layer).init();
            return;
        }
    }

    registry.init();
}

// =============================================================================
// Debug-only: error loop detection
// =============================================================================

#[cfg(debug_assertions)]
pub mod error_loop {
    use std::sync::{LazyLock, Mutex};
    use std::time::Instant;

    const WINDOW_SECS: u64 = 1;
    const THRESHOLD: usize = 100;

    struct Inner {
        count: usize,
        window_start: Instant,
        last_msg: String,
    }

    pub struct ErrorLoopDetector {
        inner: Mutex<Inner>,
    }

    impl ErrorLoopDetector {
        pub fn new() -> Self {
            Self {
                inner: Mutex::new(Inner {
                    count: 0,
                    window_start: Instant::now(),
                    last_msg: String::new(),
                }),
            }
        }

        /// Record an error occurrence. Panics (triggering crash report) if the
        /// same error repeats more than `THRESHOLD` times within `WINDOW_SECS`.
        pub fn record(&self, msg: &str) {
            let Ok(mut inner) = self.inner.lock() else {
                return;
            };

            let now = Instant::now();
            let elapsed = now.duration_since(inner.window_start).as_secs();

            if elapsed >= WINDOW_SECS || inner.last_msg != msg {
                inner.count = 1;
                inner.window_start = now;
                inner.last_msg = msg.to_string();
                return;
            }

            inner.count += 1;

            if inner.count >= THRESHOLD {
                let count = inner.count;
                let last_msg = inner.last_msg.clone();
                drop(inner);
                panic!(
                    "Error loop detected! The following error repeated {count} times in {WINDOW_SECS}s:\n{last_msg}"
                );
            }
        }
    }

    /// Global error loop detector instance.
    static DETECTOR: LazyLock<ErrorLoopDetector> = LazyLock::new(ErrorLoopDetector::new);

    /// Record an error for loop detection. Call this at recurring error sites
    /// (render loop, event loop). Panics if the same error repeats >100 times/sec.
    pub fn record_error(msg: &str) {
        DETECTOR.record(msg);
    }
}

#[cfg(debug_assertions)]
pub use error_loop::record_error;

/// Record an error for loop detection (debug builds only, no-op in release).
#[cfg(not(debug_assertions))]
#[inline(always)]
pub fn record_error(_msg: &str) {}
