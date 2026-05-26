//! GUI integration test harness for tasty.
//!
//! Launches a single shared tasty GUI instance for all tests.
//! Each test creates its own workspace for isolation — no state reset needed.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

use enigo::{Coordinate, Direction, Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings};
use serde_json::Value;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClientRect, GetWindowRect, SW_RESTORE, SetForegroundWindow, ShowWindow,
};

// --- Shared instance ---

static SHARED_INSTANCE: OnceLock<Mutex<GuiTestInstance>> = OnceLock::new();
static CLEANUP_PID: AtomicU32 = AtomicU32::new(0);

/// Acquire the shared GUI test instance.
/// The first call spawns the tasty process; subsequent calls reuse it.
pub fn shared() -> std::sync::MutexGuard<'static, GuiTestInstance> {
    let guard = SHARED_INSTANCE.get_or_init(|| {
        let inst = GuiTestInstance::spawn();
        // Register atexit to kill tasty when the test process exits
        CLEANUP_PID.store(inst.process_id(), Ordering::Relaxed);
        extern "C" fn on_exit() {
            let pid = CLEANUP_PID.load(Ordering::Relaxed);
            if pid != 0 {
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("taskkill")
                        .args(["/F", "/T", "/PID", &pid.to_string()])
                        .stdout(std::process::Stdio::null())
                        .stderr(std::process::Stdio::null())
                        .status();
                }
                #[cfg(not(target_os = "windows"))]
                // SAFETY: SIGTERM 송신은 thread-safe POSIX. pid가 이미 종료된 상태여도
                // kill은 errno만 set하고 UB 없음.
                unsafe {
                    libc::kill(pid as i32, libc::SIGTERM);
                }
            }
        }
        // SAFETY: atexit는 process-lifetime callback을 등록. on_exit는 'static fn 포인터.
        // shared() 첫 호출 시 한 번만 등록되며 (OnceLock get_or_init), 중복 등록 없음.
        unsafe {
            libc::atexit(on_exit);
        }
        Mutex::new(inst)
    });
    guard.lock().unwrap()
}

// --- GuiTestInstance ---

/// GUI test instance: a running tasty GUI process with IPC access and input simulation.
pub struct GuiTestInstance {
    process: Child,
    port: u16,
    port_file: PathBuf,
    pub enigo: Enigo,
    #[cfg(target_os = "windows")]
    hwnd: HWND,
}

// SAFETY: GuiTestInstance는 OnceLock<Mutex<>> 안에 들어가 모든 접근이 Mutex로 직렬화된다.
// HWND(*mut c_void) 자체는 OS thread affinity 측면에서 보면 main thread 윈도우 객체지만,
// 테스트 코드는 (1) instance를 단일 thread에서만 spawn하고 (2) HWND를 SetForegroundWindow
// 등 thread-safe Win32 호출에만 전달한다. 따라서 임의 스레드에서 HWND를 "소유"하는 게
// 아니라 단지 포인터 값을 전달하는 수준이므로 Send/Sync 추가가 안전하다.
unsafe impl Send for GuiTestInstance {}
// SAFETY: 위 Send와 동일 근거 — Mutex 직렬화 + 단순 포인터 값 전달.
unsafe impl Sync for GuiTestInstance {}

impl GuiTestInstance {
    /// Spawn a tasty GUI instance for testing.
    /// Waits for the window to appear and focuses it.
    pub fn spawn() -> Self {
        let port_file = std::env::temp_dir().join(format!(
            "tasty-gui-test-{}-{}.port",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        // Launch tasty in GUI mode with port-file for IPC
        let process = Command::new(env!("CARGO_BIN_EXE_tasty"))
            .arg("--port-file")
            .arg(port_file.to_str().unwrap())
            .spawn()
            .expect("failed to spawn tasty GUI");

        // Wait for port file (IPC ready)
        let start = Instant::now();
        let port = loop {
            if start.elapsed() > Duration::from_secs(15) {
                panic!("tasty GUI failed to write port file within 15 seconds");
            }
            if let Ok(content) = std::fs::read_to_string(&port_file)
                && let Ok(port) = content.trim().parse::<u16>()
            {
                break port;
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        // Wait for the window to appear
        #[cfg(target_os = "windows")]
        let hwnd = Self::wait_for_window("Tasty", Duration::from_secs(15));

        // Let the window fully initialize (GPU, terminal, etc.)
        std::thread::sleep(Duration::from_millis(1500));

        let enigo = Enigo::new(&EnigoSettings::default()).expect("failed to create enigo instance");

        let instance = Self {
            process,
            port,
            port_file,
            enigo,
            #[cfg(target_os = "windows")]
            hwnd,
        };

        // Focus the window
        instance.focus();
        std::thread::sleep(Duration::from_millis(300));

        instance
    }

    /// Get the child process ID.
    pub fn process_id(&self) -> u32 {
        self.process.id()
    }

    /// Focus the tasty window.
    pub fn focus(&self) {
        #[cfg(target_os = "windows")]
        // SAFETY: self.hwnd는 spawn 시 FindWindowW로 찾은 활성 윈도우 핸들.
        // 본 instance가 살아있는 동안 valid (Drop이 process 종료를 보장).
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
        #[cfg(target_os = "macos")]
        {
            use objc2_app_kit::{NSApplicationActivationOptions, NSRunningApplication};
            let pid = self.process.id() as i32;
            if let Some(app) = NSRunningApplication::runningApplicationWithProcessIdentifier(pid) {
                app.activateWithOptions(NSApplicationActivationOptions::ActivateAllWindows);
            }
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    /// Send a JSON-RPC request and return the result.
    pub fn call(&self, method: &str, params: Value) -> Value {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .expect("failed to connect to tasty IPC");
        stream.set_read_timeout(Some(Duration::from_secs(5))).ok();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let mut msg = serde_json::to_string(&request).unwrap();
        msg.push('\n');
        stream
            .write_all(msg.as_bytes())
            .expect("failed to send IPC");

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("failed to read IPC response");

        let resp: Value = serde_json::from_str(&line).expect("invalid JSON response");
        if let Some(error) = resp.get("error") {
            panic!("IPC error: {}", error);
        }
        resp.get("result").cloned().unwrap_or(Value::Null)
    }

    /// Query the UI overlay state.
    pub fn ui_state(&self) -> UiState {
        let result = self.call("ui.state", serde_json::json!({}));
        UiState {
            settings_open: result["settings_open"].as_bool().unwrap_or(false),
            notification_panel_open: result["notification_panel_open"].as_bool().unwrap_or(false),
            workspace_count: result["workspace_count"].as_u64().unwrap_or(0) as usize,
            active_workspace: result["active_workspace"].as_u64().unwrap_or(0) as usize,
            pane_count: result["pane_count"].as_u64().unwrap_or(0) as usize,
            tab_count: result["tab_count"].as_u64().unwrap_or(0) as usize,
        }
    }

    /// Create a new workspace via IPC. Returns the workspace index (0-based).
    #[allow(dead_code)]
    pub fn create_workspace(&self, name: &str) -> usize {
        let _result = self.call("workspace.create", serde_json::json!({ "name": name }));
        let state = self.ui_state();
        state.workspace_count - 1
    }

    /// Get the first surface ID from surface.list.
    #[allow(dead_code)]
    pub fn first_surface_id(&self) -> u64 {
        let surfaces = self.call("surface.list", serde_json::json!({}));
        surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    /// Get the first pane ID from pane.list.
    #[allow(dead_code)]
    pub fn first_pane_id(&self) -> u64 {
        let panes = self.call("pane.list", serde_json::json!({}));
        panes.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    // --- Input simulation helpers ---

    /// Press a key combination (e.g., Ctrl+Comma).
    pub fn press_key(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo
            .key(key, Direction::Click)
            .expect("key press failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Press Ctrl + a key.
    pub fn press_ctrl(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo
            .key(Key::Control, Direction::Press)
            .expect("ctrl press failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo
            .key(key, Direction::Click)
            .expect("key click failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo
            .key(Key::Control, Direction::Release)
            .expect("ctrl release failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Press Ctrl+Shift + a key.
    pub fn press_ctrl_shift(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo
            .key(Key::Control, Direction::Press)
            .expect("ctrl press failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo
            .key(Key::Shift, Direction::Press)
            .expect("shift press failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo
            .key(key, Direction::Click)
            .expect("key click failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo
            .key(Key::Shift, Direction::Release)
            .expect("shift release failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo
            .key(Key::Control, Direction::Release)
            .expect("ctrl release failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Press Alt + a key.
    pub fn press_alt(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo
            .key(Key::Alt, Direction::Press)
            .expect("alt press failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo
            .key(key, Direction::Click)
            .expect("key click failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo
            .key(Key::Alt, Direction::Release)
            .expect("alt release failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Type text into the focused terminal.
    pub fn type_text(&mut self, text: &str) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo.text(text).expect("text input failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Click at a position relative to the window's client area.
    #[allow(dead_code)]
    pub fn click_at(&mut self, x: i32, y: i32) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));

        // Convert window-relative coordinates to screen coordinates
        let (screen_x, screen_y) = self.client_to_screen(x, y);

        self.enigo
            .move_mouse(screen_x, screen_y, Coordinate::Abs)
            .expect("mouse move failed");
        std::thread::sleep(Duration::from_millis(50));
        self.enigo
            .button(enigo::Button::Left, Direction::Click)
            .expect("mouse click failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Get window client area size (width, height).
    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    pub fn client_size(&self) -> (i32, i32) {
        let mut rect = windows::Win32::Foundation::RECT::default();
        // SAFETY: self.hwnd valid (위 focus 주석과 동일). GetClientRect는 thread-safe.
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        (rect.right - rect.left, rect.bottom - rect.top)
    }

    /// Convert client-relative (x, y) to screen coordinates.
    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    fn client_to_screen(&self, x: i32, y: i32) -> (i32, i32) {
        let mut window_rect = windows::Win32::Foundation::RECT::default();
        let mut client_rect = windows::Win32::Foundation::RECT::default();
        // SAFETY: self.hwnd valid. GetWindowRect/GetClientRect는 thread-safe Win32 호출.
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut window_rect);
            let _ = GetClientRect(self.hwnd, &mut client_rect);
        }
        // The client area offset from window top-left
        let border_x =
            ((window_rect.right - window_rect.left) - (client_rect.right - client_rect.left)) / 2;
        let title_height = (window_rect.bottom - window_rect.top)
            - (client_rect.bottom - client_rect.top)
            - border_x;

        (
            window_rect.left + border_x + x,
            window_rect.top + title_height + y,
        )
    }

    #[cfg(not(target_os = "windows"))]
    fn client_to_screen(&self, x: i32, y: i32) -> (i32, i32) {
        // Fallback: assume no offset (non-Windows)
        (x, y)
    }

    /// Wait until a condition on ui_state is met, or panic after timeout.
    pub fn wait_for_ui<F: Fn(&UiState) -> bool>(
        &self,
        description: &str,
        timeout: Duration,
        condition: F,
    ) -> UiState {
        let start = Instant::now();
        loop {
            let state = self.ui_state();
            if condition(&state) {
                return state;
            }
            if start.elapsed() > timeout {
                panic!(
                    "Timeout waiting for UI condition: {}. Current state: {:?}",
                    description, state
                );
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }

    /// Shutdown the instance gracefully via IPC.
    #[allow(dead_code)]
    pub fn shutdown(&self) {
        let _ = self.call("system.shutdown", serde_json::json!({}));
    }

    // --- Windows-specific helpers ---

    #[cfg(target_os = "windows")]
    fn wait_for_window(title: &str, timeout: Duration) -> HWND {
        let wide_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                panic!("Window '{}' did not appear within {:?}", title, timeout);
            }
            // SAFETY: wide_title은 null-terminated UTF-16 local Vec, 호출 동안 살아있음.
            let hwnd = unsafe { FindWindowW(None, windows::core::PCWSTR(wide_title.as_ptr())) };
            match hwnd {
                Ok(h) if !h.is_invalid() => return h,
                _ => {
                    std::thread::sleep(Duration::from_millis(200));
                }
            }
        }
    }
}

impl Drop for GuiTestInstance {
    fn drop(&mut self) {
        // On Windows, kill the entire process tree (tasty + child shells).
        // process.kill() only kills the parent, leaving orphan shell processes.
        #[cfg(target_os = "windows")]
        {
            let pid = self.process.id();
            let _ = std::process::Command::new("taskkill")
                .args(["/F", "/T", "/PID", &pid.to_string()])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status();
        }
        #[cfg(not(target_os = "windows"))]
        {
            let _ = self.process.kill();
        }
        let _ = self.process.wait();
        let _ = std::fs::remove_file(&self.port_file);
    }
}

/// Snapshot of UI overlay state, queried via IPC.
#[derive(Debug, Clone)]
pub struct UiState {
    pub settings_open: bool,
    pub notification_panel_open: bool,
    pub workspace_count: usize,
    pub active_workspace: usize,
    pub pane_count: usize,
    pub tab_count: usize,
}
