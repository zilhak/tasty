// 이유: 크래시 리포터를 설치하는 것이 gui 부팅 경로뿐이라 headless 빌드엔 호출자가 없다. 모듈을
// `#[cfg]` 로 가리지 않는 것은 headless 에서도 타입체크를 받게 하려는 것이다.
#![cfg_attr(not(feature = "gui"), allow(dead_code, unused_imports))]

use std::backtrace::Backtrace;
use std::fs;
use std::io::{self, Write};
use std::panic;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
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
/// 공유 로그(`debug.log`)가 아니라 별도 파일인 이유: 그 로그는 host 프로세스가 뜰 때마다
/// truncate 되므로, 행을 겪고 강제 종료 후 다시 띄우는 순간 증거가 지워진다.
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
///
/// 파일 레이어는 여기서 *설치*만 되고 파일은 열지 않는다. 실제 파일을 여는 것은 host
/// 프로세스(GUI / headless)가 부르는 [`enable_host_file_log`] 뿐이다 — 근거는
/// [ADR-0092](../../docs/adr/0092-file-log-host-process-only.md).
///
/// **한계**: 그래서 이 함수와 [`enable_host_file_log`] 사이의 구간
/// (`boot::run()` 의 `attach_windows_console_if_needed()` + `cli_routing::parse_or_route()`)
/// 에서 발생한 로그는 host 프로세스에서도 **파일에 남지 않는다** — stderr 로만 나간다.
/// 현재 그 구간에는 tracing 호출이 없어 실제 유실은 없지만, 라우팅 이전에 로그를
/// 추가하면 파일 로그에서 조용히 빠진다. 파일에 반드시 남아야 하는 진단이라면 라우팅
/// 이후로 옮기거나 전용 파일(`crash-*.log` / `hang-*.log` / `hook-failures.log`)을 쓴다.
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

/// 파일 로그 싱크. host 프로세스가 [`enable_host_file_log`] 로 열어 넣기 전까지 비어
/// 있고, 그동안 파일 레이어가 만든 출력은 버려진다.
static LOG_FILE: OnceLock<Mutex<fs::File>> = OnceLock::new();

/// 파일 레이어의 writer 팩토리. `LOG_FILE` 이 채워진 프로세스에서만 실제로 쓴다.
struct HostLogWriter;

/// 이벤트 한 건을 쓰는 동안 파일 락을 잡는다 — `Mutex<File>` 의 기본 `MakeWriter` 구현과
/// 같은 원자성(한 줄이 다른 줄 사이에 끼어들지 않는다). 파일이 없으면 조용히 버린다.
enum HostLogSink {
    File(MutexGuard<'static, fs::File>),
    Discard,
}

impl io::Write for HostLogSink {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            Self::File(file) => file.write(buf),
            Self::Discard => Ok(buf.len()),
        }
    }

    fn flush(&mut self) -> io::Result<()> {
        match self {
            Self::File(file) => file.flush(),
            Self::Discard => Ok(()),
        }
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for HostLogWriter {
    type Writer = HostLogSink;

    fn make_writer(&'a self) -> Self::Writer {
        match LOG_FILE.get().map(Mutex::lock) {
            Some(Ok(guard)) => HostLogSink::File(guard),
            // 아직 안 열렸거나(= CLI 프로세스), 쓰는 도중 다른 스레드가 panic 해 poison
            // 된 경우. 로그 한 줄 때문에 여기서 다시 panic 하지 않는다.
            Some(Err(_)) | None => HostLogSink::Discard,
        }
    }
}

/// 파일 로그 파일명. dev 와 release 는 필터가 달라 파일도 나눈다.
fn log_file_name() -> &'static str {
    if cfg!(debug_assertions) {
        "debug-dev.log"
    } else {
        "debug.log"
    }
}

/// 파일 레이어 필터. stderr 의 `TASTY_LOG` 와 독립적으로 고정된다 — dev 는
/// `debug` 레벨(기존 동작 유지), release/dist 는 `warn` 이상만(디스크 사용량 제한 —
/// attach disconnect 같은 진단 가치가 있는 로그만 release 사용자 환경에 보존하는 게
/// 목적이라 전체 debug 상시 로깅까진 필요 없다).
fn file_env_filter() -> EnvFilter {
    if cfg!(debug_assertions) {
        EnvFilter::new("debug,wgpu_hal=warn,wgpu_core=warn,naga=warn")
    } else {
        EnvFilter::new("warn,wgpu_hal=warn,wgpu_core=warn,naga=warn")
    }
}

/// stderr + file tracing, all build modes. stderr 필터는 `make_env_filter()`
/// (`TASTY_LOG`, 기본 warn) 를, 파일 필터는 `file_env_filter()` 를 따른다.
///
/// 두 레이어 모두 **모든 프로세스**에 설치되지만, 파일 레이어의 출력은
/// [`enable_host_file_log`] 를 부른 프로세스에서만 파일에 닿는다. 그래서 이 함수는
/// CLI/GUI 판정 이전(= 프로세스 역할을 모르는 시점)에 불려도 안전하다 — stderr 로그는
/// 부팅 첫 순간부터 나가고, 공유 로그 파일은 건드리지 않는다.
fn init_tracing() {
    use tracing_subscriber::Layer as _;
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber::util::SubscriberInitExt;

    // `fmt::layer()` 의 기본 writer 는 **stdout** 이다 — 그대로 두면 진단 로그가
    // 명령 출력에 섞인다. 이 제품에서 stdout 은 에이전트가 파싱하는 채널이라
    // (`tasty list tree | jq .`), 경고 한 줄이 JSON 앞에 붙는 것만으로 깨진다.
    let stderr_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_filter(make_env_filter());
    let file_layer = tracing_subscriber::fmt::layer()
        .with_writer(HostLogWriter)
        .with_ansi(false)
        .with_filter(file_env_filter());

    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();
}

/// 공유 로그 파일(`$TASTY_HOME/debug{-dev}.log`)을 열어 파일 레이어를 활성화한다.
/// **host 프로세스(GUI / headless)만** 부른다 — CLI 클라이언트도 같은 바이너리라
/// 무조건 열면 실행할 때마다 host 가 쌓아둔 로그를 truncate 한다([ADR-0092]).
///
/// host 는 데이터 루트당 하나이므로 시작 시 truncate 를 유지한다(rotation 불필요).
/// 실패하면 stderr-only 로 자연스럽게 폴백한다.
///
/// **재호출은 무해하다(no-op).** 파일을 열기 *전에* 먼저 걸러낸다 — `File::create` 는
/// 그 자체로 truncate 라, 열고 나서 `OnceLock::set` 실패로 되돌리면 이미 늦는다. 먼저
/// 설치된 핸들이 원래 오프셋에 계속 쓰면서 파일 앞부분이 NUL 구멍이 되는, 본 ADR 이
/// 고친 바로 그 손상이 축소판으로 재현된다.
///
/// [ADR-0092]: ../../docs/adr/0092-file-log-host-process-only.md
pub fn enable_host_file_log() {
    if let Some(reason) = install_host_log_file() {
        tracing::warn!("{reason}");
    }
}

/// 파일을 열어 [`LOG_FILE`] 에 넣는다. 로그로 남길 사유가 있으면 문자열로 돌려준다 —
/// `open_host_log_file` 과 같은 방식으로, warn 호출을 호출자 한 곳에 모은다.
fn install_host_log_file() -> Option<String> {
    // 파일을 열기 전에 판정한다 — 순서를 뒤집으면 재호출이 파일을 잘라먹는다(위 참조).
    if LOG_FILE.get().is_some() {
        return Some("file logging already enabled; ignoring repeat call".to_string());
    }
    let file = match open_host_log_file() {
        Ok(file) => file,
        Err(reason) => return Some(format!("file logging disabled: {reason}")),
    };
    if LOG_FILE.set(Mutex::new(file)).is_err() {
        // 위 검사와 이 사이에 다른 스레드가 먼저 넣은 경우(정상 부팅 경로에는 없지만
        // `pub` 이라 배제할 수 없다). 우리가 연 핸들은 그대로 버려진다.
        return Some("file logging enabled concurrently; dropping this handle".to_string());
    }
    None
}

/// 로그 파일을 연다(truncate). 실패 사유는 문자열로 올려보내 로그를 호출자 한 곳에서만
/// 남긴다 — 실패해도 stderr-only 로 도는 것이 정상 폴백이라 에러 타입까진 필요 없다.
fn open_host_log_file() -> Result<fs::File, String> {
    let dir = tasty_home().ok_or_else(|| "could not resolve tasty home".to_string())?;
    fs::create_dir_all(&dir)
        .map_err(|e| format!("cannot create log dir {}: {e}", dir.display()))?;
    let path = dir.join(log_file_name());
    fs::File::create(&path).map_err(|e| format!("cannot open log file {}: {e}", path.display()))
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
