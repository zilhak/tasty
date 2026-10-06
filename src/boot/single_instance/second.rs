//! release 두 번째 프로세스. 같은 데이터 홈의 Tasty가 이미 실행 중일 때 창·GPU·이벤트 루프 없이
//! 실행 중인 Tasty에 요청만 넘기고 종료한다.
//!
//! - 사용자 실행 증거가 있으면 OS 활성화 경로를 쓴다(Linux D-Bus `Activate`, Windows 등록 창 메시지).
//! - 증거가 없으면 `tasty new window`와 같은 `window.create`로 새 View 하나를 요청한다.
//! - 실행 중인 쪽이 아직 부팅 중이면 상한 안에서 기다리고, 그 사이 잠금이 풀리면 평소처럼 부팅한다.
//! - 요청을 넘기지 못하면 Tasty 창 없이 OS 메시지 상자로 알리고 종료한다.

use std::path::Path;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use super::evidence::{self, LaunchEvidence};
use super::instance_file::{self, InstanceRecord, InstanceState};
use super::launch_log::LaunchLog;

/// 실행 중인 Tasty가 인스턴스 파일에 포트를 쓸 때까지 기다리는 상한.
const BOOT_WAIT: Duration = Duration::from_secs(20);
/// `window.create` 요청 전체(연결·재시도 포함)의 상한.
const IPC_DEADLINE: Duration = Duration::from_secs(10);
/// D-Bus 이름 소유자가 나타나기를 기다리는 상한과 메서드 응답 상한.
#[cfg(target_os = "linux")]
const OWNER_WAIT: Duration = Duration::from_secs(5);
#[cfg(target_os = "linux")]
const DBUS_METHOD_TIMEOUT: Duration = Duration::from_secs(5);
/// 대체 동작의 종료 코드. 요청을 넘기지 못했다.
const FALLBACK_EXIT: u8 = 1;

pub(crate) enum HeldOutcome {
    Exit(ExitCode),
    /// 기다리는 동안 실행 중이던 Tasty가 끝나 잠금을 얻었다. 평소처럼 부팅한다.
    BootNormally(tasty_event_store::WriterLock),
}

/// 실행 중인 Tasty에 넘길 경로.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Route {
    /// 증거 없음: `window.create`로 새 View 하나.
    NewView,
    /// Linux 증거 있음: D-Bus `Activate`.
    DbusActivate,
    /// Windows 포그라운드 권한 있음: 등록 창 메시지(창이 없으면 `window.create` 후 포그라운드 요청).
    WindowsActivate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Platform {
    Linux,
    Windows,
    MacOs,
}

impl Platform {
    fn current() -> Self {
        if cfg!(windows) {
            Self::Windows
        } else if cfg!(target_os = "macos") {
            Self::MacOs
        } else {
            Self::Linux
        }
    }
}

/// 증거로 경로를 정한다. macOS의 Finder·Dock·`open` 실행은 LaunchServices가 두 번째 프로세스를
/// 만들지 않으므로, 두 번째 프로세스가 있으면 바이너리를 직접 실행한 것으로 보고 증거 없음이다.
pub(crate) fn route(
    os: Platform,
    evidence: &LaunchEvidence,
    windows_foreground_granted: bool,
) -> Route {
    match os {
        Platform::Linux if evidence.is_present() => Route::DbusActivate,
        Platform::Windows if windows_foreground_granted => Route::WindowsActivate,
        _ => Route::NewView,
    }
}

fn display_backend() -> &'static str {
    if cfg!(windows) {
        "windows"
    } else if cfg!(target_os = "macos") {
        "macos"
    } else if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        "wayland"
    } else {
        "x11"
    }
}

/// 같은 홈의 writer 잠금이 다른 프로세스에 있을 때 release가 부른다.
pub(crate) fn handle_held(home: &Path) -> HeldOutcome {
    let started = Instant::now();
    let log = LaunchLog::new(Some(home));
    let canonical = std::fs::canonicalize(home).unwrap_or_else(|_| home.to_path_buf());
    let evidence = evidence::take();
    log.line(&format!(
        "second launch: version={} home={} os={} backend={} evidence={:?}",
        env!("CARGO_PKG_VERSION"),
        canonical.display(),
        std::env::consts::OS,
        display_backend(),
        evidence.kinds()
    ));

    let record = match wait_for_instance(home, &log) {
        Waited::Live(record) => record,
        Waited::LockReleased(lock) => {
            log.line(&format!(
                "the running instance ended while waiting ({} ms); booting normally",
                started.elapsed().as_millis()
            ));
            return HeldOutcome::BootNormally(lock);
        }
        Waited::TimedOut(reason) => {
            return HeldOutcome::Exit(fallback(
                &log,
                &evidence,
                &format!("instance not ready: {reason}"),
            ));
        }
    };
    log.line(&format!(
        "instance live: pid={} start_time={} port={:?} after {} ms",
        record.pid,
        record.start_time,
        record.port,
        started.elapsed().as_millis()
    ));

    let granted = windows_grant(record.pid, &log);
    let chosen = route(Platform::current(), &evidence, granted);
    log.line(&format!("route: {chosen:?}"));
    let code = match chosen {
        Route::NewView => match request_new_view(&record, &log) {
            Ok(window_id) => {
                log.line(&format!("new view requested: window_id={window_id}"));
                ExitCode::SUCCESS
            }
            Err(reason) => fallback(&log, &evidence, &reason),
        },
        Route::DbusActivate => dbus_activate(home, &evidence, &log),
        Route::WindowsActivate => windows_activate(home, &record, &log),
    };
    log.line(&format!(
        "second launch done in {} ms",
        started.elapsed().as_millis()
    ));
    HeldOutcome::Exit(code)
}

enum Waited {
    Live(InstanceRecord),
    LockReleased(tasty_event_store::WriterLock),
    TimedOut(String),
}

/// 포트가 있는 살아 있는 기록이 나올 때까지 기다린다. 잠금 시도는 writer 잠금의 재시도 구간을 쓰므로
/// 그 자체가 대기 간격이 된다.
fn wait_for_instance(home: &Path, log: &LaunchLog) -> Waited {
    let deadline = Instant::now() + BOOT_WAIT;
    let database = crate::runtime::journal_product::journal_database_path(home);
    let mut last_reason = String::new();
    loop {
        match instance_file::read_state(home) {
            InstanceState::Live(record) => return Waited::Live(record),
            InstanceState::Booting(reason) => {
                let text = format!("{reason:?}");
                if text != last_reason {
                    log.line(&format!("waiting for the running instance: {text}"));
                    last_reason = text;
                }
            }
        }
        if Instant::now() >= deadline {
            return Waited::TimedOut(format!("{last_reason} after {} ms", BOOT_WAIT.as_millis()));
        }
        match tasty_event_store::preempt_writer_lock(&database) {
            Ok(tasty_event_store::WriterPreempt::Acquired(lock)) => {
                return Waited::LockReleased(lock);
            }
            Ok(tasty_event_store::WriterPreempt::Held) => {}
            Err(e) => {
                log.line(&format!("writer lock check failed: {e}"));
                std::thread::sleep(Duration::from_millis(200));
            }
        }
    }
}

/// `window.create`를 멱등 키와 함께 보낸다. 응답이 유실되면 같은 키로, 명시적으로 거절되면 새 키로 다시 보낸다.
fn request_new_view(record: &InstanceRecord, log: &LaunchLog) -> Result<u64, String> {
    request_new_view_within(record, log, IPC_DEADLINE)
}

fn request_new_view_within(
    record: &InstanceRecord,
    log: &LaunchLog,
    limit: Duration,
) -> Result<u64, String> {
    let Some(port) = record.port else {
        return Err("instance record has no port".to_string());
    };
    let deadline = Instant::now() + limit;
    let mut key = new_key();
    let mut attempt = 0u32;
    loop {
        attempt += 1;
        let left = deadline.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Err(format!(
                "window.create gave no result within {} ms ({attempt} attempts)",
                limit.as_millis()
            ));
        }
        match send_window_create(port, &key, left) {
            Ok(value) => {
                return value
                    .get("window_id")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| format!("window.create returned no window_id: {value}"));
            }
            Err(SendError::Refused { code, message }) => {
                log.line(&format!(
                    "window.create refused (attempt {attempt}): code={code} message={message}"
                ));
                key = new_key();
            }
            Err(SendError::Transport(e)) => {
                log.line(&format!(
                    "window.create transport error (attempt {attempt}): {e}"
                ));
            }
        }
        std::thread::sleep(Duration::from_millis(200).min(left));
    }
}

enum SendError {
    Refused { code: i32, message: String },
    Transport(String),
}

fn new_key() -> String {
    format!(
        "single-instance-{}-{:016x}",
        std::process::id(),
        rand::random::<u64>()
    )
}

fn send_window_create(
    port: u16,
    key: &str,
    left: Duration,
) -> Result<serde_json::Value, SendError> {
    let address = std::net::SocketAddr::from(([127, 0, 0, 1], port));
    let transport = |e: &dyn std::fmt::Display| SendError::Transport(e.to_string());
    let stream = std::net::TcpStream::connect_timeout(&address, left).map_err(|e| transport(&e))?;
    stream
        .set_read_timeout(Some(left))
        .map_err(|e| transport(&e))?;
    stream
        .set_write_timeout(Some(left))
        .map_err(|e| transport(&e))?;
    let mut connection =
        tasty_ipc::client::IpcConnection::new(stream).map_err(|e| transport(&e))?;
    let request = tasty_ipc::protocol::JsonRpcRequest {
        jsonrpc: "2.0".to_string(),
        method: "window.create".to_string(),
        params: serde_json::json!({}),
        id: Some(serde_json::json!(1)),
        session_token: None,
        response_timeout_ms: None,
        idempotency_key: None,
    };
    connection
        .send_idempotent(&request, key)
        .map_err(
            |e| match e.downcast_ref::<tasty_ipc::client::JsonRpcCallError>() {
                Some(call) => SendError::Refused {
                    code: call.code,
                    message: call.message.clone(),
                },
                None => transport(&e),
            },
        )
}

#[cfg(target_os = "linux")]
fn dbus_activate(home: &Path, evidence: &LaunchEvidence, log: &LaunchLog) -> ExitCode {
    let name = super::dbus::bus_name(home);
    let connection = match super::dbus::connect(DBUS_METHOD_TIMEOUT) {
        Ok(c) => c,
        Err(e) => return activation_failed(log, evidence, &format!("session bus: {e}")),
    };
    let deadline = Instant::now() + OWNER_WAIT;
    loop {
        match super::dbus::name_has_owner(&connection, &name) {
            Ok(true) => break,
            Ok(false) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(100))
            }
            Ok(false) => {
                return activation_failed(
                    log,
                    evidence,
                    &format!("D-Bus name {name} has no owner"),
                );
            }
            Err(e) => {
                return activation_failed(log, evidence, &format!("NameHasOwner({name}): {e}"));
            }
        }
    }
    match super::dbus::activate(&connection, &name, evidence) {
        Ok(()) => {
            log.line(&format!("D-Bus Activate sent to {name}"));
            ExitCode::SUCCESS
        }
        Err(e) => activation_failed(log, evidence, &format!("Activate on {name}: {e}")),
    }
}

#[cfg(not(target_os = "linux"))]
fn dbus_activate(_home: &Path, evidence: &LaunchEvidence, log: &LaunchLog) -> ExitCode {
    fallback(log, evidence, "D-Bus activation is only used on Linux")
}

#[cfg(target_os = "linux")]
/// 증거를 넘기지 못했다. X11이면 실행기의 대기 표시를 직접 끝낸 뒤 대체 동작으로 간다.
fn activation_failed(log: &LaunchLog, evidence: &LaunchEvidence, reason: &str) -> ExitCode {
    log.line(&format!("activation failed: {reason}"));
    if let Some(id) = &evidence.x11_startup_id {
        match crate::platform::window_activation::x11_startup_complete(id) {
            Ok(()) => log.line("sent X11 startup-notification remove"),
            Err(e) => log.line(&format!("X11 startup-notification remove failed: {e}")),
        }
    }
    fallback(log, evidence, reason)
}

#[cfg(windows)]
fn windows_grant(pid: u32, log: &LaunchLog) -> bool {
    match crate::platform::single_instance_windows::allow_set_foreground(pid) {
        Ok(()) => {
            log.line(&format!("AllowSetForegroundWindow({pid}) succeeded"));
            true
        }
        Err(e) => {
            log.line(&format!("AllowSetForegroundWindow({pid}) refused: {e}"));
            false
        }
    }
}

#[cfg(not(windows))]
fn windows_grant(_pid: u32, _log: &LaunchLog) -> bool {
    false
}

#[cfg(windows)]
fn windows_activate(home: &Path, record: &InstanceRecord, log: &LaunchLog) -> ExitCode {
    use crate::platform::single_instance_windows as win;
    let find = |log: &LaunchLog| match win::enumerate_top_level() {
        Ok(windows) => {
            let found = win::pick_main_view(&windows, record.pid);
            log.line(&format!(
                "enumerated {} top-level windows; main view of pid {}: {}",
                windows.len(),
                record.pid,
                found.is_some()
            ));
            found
        }
        Err(e) => {
            log.line(&format!("window enumeration failed: {e}"));
            None
        }
    };
    let post = |hwnd: isize, log: &LaunchLog| match win::post_activate(hwnd) {
        Ok(()) => {
            log.line("posted the activate message");
            ExitCode::SUCCESS
        }
        Err(e) => fallback(
            log,
            &LaunchEvidence::default(),
            &format!("post activate: {e}"),
        ),
    };
    if let Some(hwnd) = find(log) {
        return post(hwnd, log);
    }
    // 창이 없으면 연속 실행이 각자 창을 만들지 않도록 차례로 처리하고 다시 확인한다.
    let _turn = match activation_turn(home) {
        Ok(lock) => lock,
        Err(e) => {
            return fallback(
                log,
                &LaunchEvidence::default(),
                &format!("activation lock: {e}"),
            );
        }
    };
    if let Some(hwnd) = find(log) {
        return post(hwnd, log);
    }
    match request_new_view(record, log) {
        Ok(window_id) => {
            let granted = win::set_foreground(window_id);
            log.line(&format!(
                "new view {window_id} requested; SetForegroundWindow returned {granted}"
            ));
            ExitCode::SUCCESS
        }
        Err(reason) => fallback(log, &LaunchEvidence::default(), &reason),
    }
}

#[cfg(not(windows))]
fn windows_activate(_home: &Path, _record: &InstanceRecord, log: &LaunchLog) -> ExitCode {
    fallback(
        log,
        &LaunchEvidence::default(),
        "window messages are only used on Windows",
    )
}

/// 증거 있음 + 창 없음 처리를 차례로 하기 위한 홈의 활성화 잠금.
#[cfg(windows)]
fn activation_turn(home: &Path) -> std::io::Result<std::fs::File> {
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(home.join("activation.lock"))?;
    let deadline = Instant::now() + IPC_DEADLINE;
    loop {
        match file.try_lock() {
            Ok(()) => return Ok(file),
            Err(std::fs::TryLockError::WouldBlock) if Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(std::fs::TryLockError::WouldBlock) => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "activation lock is busy",
                ));
            }
            Err(std::fs::TryLockError::Error(e)) => return Err(e),
        }
    }
}

/// Tasty 창 없이 원인을 알리고 끝낸다. 문구는 하나이고 상세는 launch.log에 있다.
fn fallback(log: &LaunchLog, _evidence: &LaunchEvidence, reason: &str) -> ExitCode {
    log.line(&format!("fallback: {reason}"));
    crate::boot::locale::init();
    let title = crate::i18n::t("app.name").to_string();
    let body = crate::i18n::t("boot.already_running.body").to_string();
    let shown = show_notice(&title, &body);
    log.line(&format!("notice: {shown}; exit code {FALLBACK_EXIT}"));
    ExitCode::from(FALLBACK_EXIT)
}

/// 메시지 상자를 띄운다. Linux 메시지 상자는 외부 프로그램 zenity에 기대므로 없으면 데스크톱 알림으로 바꾼다.
fn show_notice(title: &str, body: &str) -> String {
    #[cfg(target_os = "linux")]
    {
        if !program_on_path("zenity") {
            return match super::dbus::notify(title, body) {
                Ok(()) => "desktop notification (zenity not found)".to_string(),
                Err(e) => format!("not shown: zenity not found and notification failed: {e}"),
            };
        }
    }
    let result = rfd::MessageDialog::new()
        .set_title(title)
        .set_description(body)
        .set_level(rfd::MessageLevel::Info)
        .set_buttons(rfd::MessageButtons::Ok)
        .show();
    format!("message box ({result:?})")
}

#[cfg(target_os = "linux")]
fn program_on_path(name: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|paths| {
        std::env::split_paths(&paths).any(|dir| {
            let candidate = dir.join(name);
            std::fs::metadata(&candidate).is_ok_and(|m| {
                use std::os::unix::fs::PermissionsExt;
                m.is_file() && m.permissions().mode() & 0o111 != 0
            })
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(wayland: Option<&str>, x11: Option<&str>) -> LaunchEvidence {
        LaunchEvidence::from_values(wayland.map(str::to_string), x11.map(str::to_string))
    }

    #[test]
    fn linux_routes_by_the_launcher_evidence() {
        assert_eq!(
            route(Platform::Linux, &with(None, None), false),
            Route::NewView
        );
        assert_eq!(
            route(Platform::Linux, &with(Some("t"), None), false),
            Route::DbusActivate
        );
        assert_eq!(
            route(Platform::Linux, &with(None, Some("id")), false),
            Route::DbusActivate
        );
    }

    #[test]
    fn windows_routes_by_the_foreground_grant_only() {
        assert_eq!(
            route(Platform::Windows, &with(None, None), true),
            Route::WindowsActivate
        );
        assert_eq!(
            route(Platform::Windows, &with(None, Some("id")), false),
            Route::NewView
        );
    }

    #[test]
    fn a_direct_second_process_on_macos_never_activates() {
        assert_eq!(
            route(Platform::MacOs, &with(Some("t"), Some("id")), true),
            Route::NewView
        );
    }

    #[test]
    fn a_released_lock_while_waiting_boots_normally() {
        let home = tempfile::tempdir().unwrap();
        // 인스턴스 파일도 잠금 소유자도 없다 → 첫 잠금 시도에서 얻는다.
        let log = LaunchLog::new(Some(home.path()));
        assert!(matches!(
            wait_for_instance(home.path(), &log),
            Waited::LockReleased(_)
        ));
        let text = std::fs::read_to_string(super::super::launch_log::path(home.path())).unwrap();
        assert!(
            text.contains("waiting for the running instance: NoFile"),
            "{text}"
        );
    }

    #[test]
    fn a_live_record_is_returned_without_touching_the_lock() {
        let home = tempfile::tempdir().unwrap();
        let pid = std::process::id();
        let record = InstanceRecord {
            pid,
            start_time: instance_file::process_start_time(pid).unwrap(),
            port: Some(9),
        };
        std::fs::write(
            instance_file::path(home.path()),
            serde_json::to_vec(&record).unwrap(),
        )
        .unwrap();
        let log = LaunchLog::new(Some(home.path()));
        assert!(matches!(wait_for_instance(home.path(), &log), Waited::Live(r) if r == record));
    }

    #[test]
    fn a_refused_or_unreachable_window_create_ends_within_the_deadline() {
        // 아무도 듣지 않는 포트: 연결 거절이 반복되고 기한 안에 오류로 끝난다.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        let home = tempfile::tempdir().unwrap();
        let log = LaunchLog::new(Some(home.path()));
        let record = InstanceRecord {
            pid: 1,
            start_time: 1,
            port: Some(port),
        };
        let started = Instant::now();
        let limit = Duration::from_millis(800);
        let error = request_new_view_within(&record, &log, limit).unwrap_err();
        assert!(error.contains("window.create gave no result"), "{error}");
        assert!(started.elapsed() < limit + Duration::from_secs(1));
        let text = std::fs::read_to_string(super::super::launch_log::path(home.path())).unwrap();
        assert!(text.contains("transport error"), "{text}");
    }
}
