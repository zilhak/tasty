use std::backtrace::Backtrace;
use std::fs;
use std::io::Write;
use std::panic;
use std::path::PathBuf;
use std::time::SystemTime;

use tracing_subscriber::EnvFilter;

use crate::settings::tasty_home;

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
    writeln!(file).ok();

    writeln!(file, "=== Backtrace ===").ok();
    writeln!(file, "{backtrace}").ok();

    Some(path)
}

/// Initialize crash reporting and tracing.
///
/// - **All builds**: Installs a panic hook that writes crash reports to `~/.tasty/crash-reports/`.
///   Initializes tracing with stderr output.
/// - **Debug builds only**: Additionally writes all tracing output to `~/.tasty/debug.log`.
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
    EnvFilter::try_from_env("TASTY_LOG")
        .unwrap_or_else(|_| EnvFilter::new("warn,wgpu_hal=error,wgpu_core=error,naga=error"))
}

/// Release builds: stderr-only tracing.
#[cfg(not(debug_assertions))]
fn init_tracing() {
    tracing_subscriber::fmt()
        .with_env_filter(make_env_filter())
        .init();
}

/// Debug builds: stderr + file tracing.
#[cfg(debug_assertions)]
fn init_tracing() {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    let stderr_layer = tracing_subscriber::fmt::layer().with_filter(make_env_filter());

    let registry = tracing_subscriber::registry().with(stderr_layer);

    // Try to set up file logging; fall back to stderr-only if it fails
    if let Some(dir) = tasty_home() {
        let _ = fs::create_dir_all(&dir);
        let log_filename = if cfg!(debug_assertions) { "debug-dev.log" } else { "debug.log" };
        let log_path = dir.join(log_filename);
        if let Ok(file) = fs::File::create(&log_path) {
            let file_layer = tracing_subscriber::fmt::layer()
                .with_writer(std::sync::Mutex::new(file))
                .with_ansi(false)
                .with_filter(EnvFilter::new("debug,wgpu_hal=warn,wgpu_core=warn,naga=warn"));

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
