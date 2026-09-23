//! Panic reporting and host logs are shared by GUI, headless and CLI startup.
//! Only winit stall reports and the GUI error-loop detector require the GUI feature.

use std::backtrace::Backtrace;
use std::fs;
use std::io::{self, Write};
use std::panic;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::SystemTime;

use tracing_subscriber::EnvFilter;

use tasty_utils::path::tasty_home;

/// 보고서 머리의 `Version:` 에 찍을 **본체** 버전. [`init`] 이 받아 둔다.
///
/// 이 크레이트에서 `env!("CARGO_PKG_VERSION")` 를 쓰면 이 크레이트(`tasty-platform`)의
/// 버전으로 풀린다 — 보고서를 받는 사람이 가를 값은 tasty 바이너리의 버전이므로,
/// 그 값은 본체가 자기 크레이트에서 풀어 넘긴다(docs/architecture/index.md#크레이트를-나누는-기준: 의미는 부르는 쪽이 정한다).
static APP_VERSION: OnceLock<&'static str> = OnceLock::new();

/// 보고서에 찍을 버전. [`init`] 전이면 `unknown` 이다 — 틀린 값보다 모른다는 값이 낫다.
fn app_version() -> &'static str {
    APP_VERSION.get().copied().unwrap_or("unknown")
}

/// crash · hang 보고서가 공유하는 머리(제목 · 시각 · 버전 · OS)를 쓴다.
fn write_report_header(out: &mut impl Write, title: &str, timestamp: &str) {
    writeln!(out, "=== {title} ===").ok();
    writeln!(
        out,
        "Timestamp: {}",
        timestamp.replace('T', " ").replace('-', ":")
    )
    .ok();
    writeln!(out, "Version: {}", app_version()).ok();
    writeln!(
        out,
        "OS: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    )
    .ok();
    writeln!(out).ok();
}

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

    write_report_header(&mut file, "Tasty Crash Report", &timestamp);

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
#[cfg(feature = "gui")]
pub fn write_hang_report(site: &str, phase: &str, stuck_ms: u64) -> Option<PathBuf> {
    let dir = crash_report_dir()?;
    fs::create_dir_all(&dir).ok()?;

    let timestamp = format_timestamp(SystemTime::now());
    let path = dir.join(format!("hang-{timestamp}.log"));
    let mut file = fs::File::create(&path).ok()?;

    write_report_header(&mut file, "Tasty Hang Report", &timestamp);

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
/// `app_version` 은 보고서 머리의 `Version:` 에 찍힌다 — 본체가 자기 크레이트에서
/// `env!("CARGO_PKG_VERSION")` 으로 풀어 넘긴다([`APP_VERSION`]).
///
/// - **All builds**: Installs a panic hook that writes crash reports to `~/.tasty/crash-reports/`.
///   Initializes tracing with stderr output, plus a file layer under `~/.tasty/` (independent of
///   the stderr `TASTY_LOG` filter — see `init_tracing`).
///
/// 파일 레이어는 여기서 *설치*만 되고 파일은 열지 않는다. 실제 파일을 여는 것은 host
/// 프로세스(GUI / headless)가 부르는 [`enable_host_file_log`] 뿐이다 — 근거는
/// [호스트 로그와 CLI 진단](../../../docs/dev-guide/cli-structure.md#호스트-로그와-cli-진단).
///
/// **한계**: 그래서 이 함수와 [`enable_host_file_log`] 사이의 구간
/// (`boot::run()` 의 `attach_windows_console_if_needed()` + `cli_routing::parse_or_route()`)
/// 에서 발생한 로그는 host 프로세스에서도 **파일에 남지 않는다** — stderr 로만 나간다.
/// 현재 그 구간에는 tracing 호출이 없어 실제 유실은 없지만, 라우팅 이전에 로그를
/// 추가하면 파일 로그에서 조용히 빠진다. 파일에 반드시 남아야 하는 진단이라면 라우팅
/// 이후로 옮기거나 전용 파일(`crash-*.log` / `hang-*.log` / `hook-failures.log`)을 쓴다.
pub fn init(app_version: &'static str) {
    set_app_version(app_version);

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

/// [`APP_VERSION`] 을 채운다. 두 번째 호출은 무시한다 — 한 프로세스의 버전은 하나이고,
/// 부팅 경로의 호출자도 하나라 두 번째 값이 첫 값과 다를 이유가 없다. 먼저 들어간
/// 값을 지키는 편이 이미 쓰였을지 모를 보고서와도 맞는다.
fn set_app_version(version: &'static str) {
    if APP_VERSION.set(version).is_err() {
        tracing::debug!(
            ignored = version,
            kept = app_version(),
            "crash report version already set; keeping the first value"
        );
    }
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
/// 무조건 열면 실행할 때마다 host 가 쌓아둔 로그를 truncate 한다([docs/dev-guide/cli-structure.md#호스트-로그와-cli-진단]).
///
/// host 는 데이터 루트당 하나이므로 시작 시 truncate 를 유지한다(rotation 불필요).
/// 실패하면 stderr-only 로 자연스럽게 폴백한다.
///
/// **재호출은 무해하다(no-op).** 파일을 열기 *전에* 먼저 걸러낸다 — `File::create` 는
/// 그 자체로 truncate 라, 열고 나서 `OnceLock::set` 실패로 되돌리면 이미 늦는다. 먼저
/// 설치된 핸들이 원래 오프셋에 계속 쓰면서 파일 앞부분이 NUL 구멍이 되는, 본 ADR 이
/// 고친 바로 그 손상이 축소판으로 재현된다.
///
/// [docs/dev-guide/cli-structure.md#호스트-로그와-cli-진단]: ../../../docs/dev-guide/cli-structure.md#호스트-로그와-cli-진단
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

#[cfg(all(feature = "gui", debug_assertions))]
pub mod error_loop {
    use std::sync::atomic::AtomicBool;
    use std::sync::{LazyLock, Mutex};
    use std::time::Instant;

    const WINDOW_SECS: u64 = 1;
    const THRESHOLD: usize = 100;

    /// `record` 가 잡는 락의 poison 보고 플래그 — 첫 1 회만 남긴다.
    ///
    /// 임계구역은 카운터 · 창 시작 시각 · 마지막 메시지 문자열 갱신뿐이라, 락을 든 채
    /// 죽은 스레드가 불변식을 깨고 나갈 수 없다 — 복구가 맞다. 반대로 조용히 건너뛰면
    /// **에러 루프 감지기 자신이 꺼진다**: 폭주하는 에러가 계속 세어지지 않아 크래시
    /// 리포트가 영영 안 나오고, 그 사실도 어디에도 안 남는다.
    ///
    /// 매번 로그를 내지 않는 이유는 이 함수가 도는 자리다 — 렌더 · 이벤트 루프의 에러
    /// 경로라 초당 `THRESHOLD` 회까지 불린다. poison 은 sticky 라 그대로 두면 그 로그가
    /// 원인이 된 로그를 묻는다.
    ///
    /// 자기 패닉으로는 오염되지 않는다: 임계치 패닉은 `drop(inner)` 로 락을 놓은 뒤에
    /// 나고, 패닉 훅([`super::init`] 의 `set_hook`)은 리포트를 쓰고 stderr 로 찍을 뿐
    /// `record_error` 를 부르지 않는다. 그래서 재진입 경로가 없다.
    static DETECTOR_POISONED: AtomicBool = AtomicBool::new(false);
    const DETECTOR_WHAT: &str = "error-loop detector";

    struct Inner {
        count: usize,
        window_start: Instant,
        last_msg: String,
    }

    pub struct ErrorLoopDetector {
        inner: Mutex<Inner>,
    }

    /// `new` 와 같다. 이 타입이 **크레이트 밖으로 나가면서** clippy 의
    /// `new_without_default` 가 켜졌다 — 본체 안에 있던 동안은 crate 내부 타입이라
    /// 그 lint 의 대상이 아니었다. 인자 없는 `new` 를 그대로 두려면 이 impl 이 짝이다.
    impl Default for ErrorLoopDetector {
        fn default() -> Self {
            Self::new()
        }
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
            let mut inner = tasty_utils::poison::recover_mutex(
                self.inner.lock(),
                DETECTOR_WHAT,
                &DETECTOR_POISONED,
            );

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

#[cfg(all(feature = "gui", debug_assertions))]
pub use error_loop::record_error;

/// Record an error for loop detection (debug builds only, no-op in release).
#[cfg(all(feature = "gui", not(debug_assertions)))]
#[inline(always)]
pub fn record_error(_msg: &str) {}

#[cfg(test)]
mod tests {
    use super::*;

    /// 시험이 넣는 버전. `APP_VERSION` 은 프로세스 전역 `OnceLock` 이라 한 번만 들어가므로,
    /// 이 모듈의 모든 시험이 **같은 값**을 넣는다 — 어느 시험이 먼저 돌든 결과가 같다.
    /// 이 크레이트의 버전(`CARGO_PKG_VERSION`)과 일부러 다른 값이다.
    const TEST_VERSION: &str = "9.8.7-crash-report-test";

    fn with_test_version() {
        set_app_version(TEST_VERSION);
        assert_eq!(
            app_version(),
            TEST_VERSION,
            "이 모듈 밖에서 다른 값이 먼저 들어갔다 — 전역 OnceLock 을 쓰는 시험이 늘었다"
        );
    }

    /// 머리의 `Version:` 은 [`init`] 이 받은 본체 버전이다 — 이 크레이트 자신의 버전이
    /// 아니다. crash · hang 두 보고서가 이 함수를 공유하므로 headless 조합에서도 잰다.
    #[test]
    fn the_report_header_carries_the_version_given_to_init() {
        with_test_version();
        let mut out = Vec::new();
        write_report_header(&mut out, "Tasty Crash Report", "2026-09-21T00-00-00");
        let text = String::from_utf8(out).expect("utf-8");
        assert!(
            text.lines()
                .any(|l| l == format!("Version: {TEST_VERSION}")),
            "머리에 넘긴 버전이 없다:\n{text}"
        );
        assert_ne!(TEST_VERSION, env!("CARGO_PKG_VERSION"));
    }

    /// hang 보고서 파일을 격리 홈에 실제로 쓰고 머리를 읽는다.
    #[cfg(feature = "gui")]
    #[test]
    fn a_hang_report_written_under_tasty_home_carries_the_version_given_to_init() {
        with_test_version();
        let home = tasty_test_support::TastyHomeGuard::new();
        let path =
            write_hang_report("redraw", "present", 5000).expect("hang report 가 써져야 한다");
        assert!(path.starts_with(home.path().join("crash-reports")));
        let text = fs::read_to_string(&path).expect("read hang report");
        assert!(text.starts_with("=== Tasty Hang Report ==="));
        assert!(
            text.lines()
                .any(|l| l == format!("Version: {TEST_VERSION}")),
            "hang 보고서 머리에 넘긴 버전이 없다:\n{text}"
        );
    }
}
