//! GUI integration test harness for tasty.
//!
//! Launches tasty in GUI mode with `--port-file` for IPC,
//! finds the window, simulates keyboard/mouse input via `enigo`,
//! and queries app state via JSON-RPC for verification.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use enigo::{
    Coordinate, Direction,
    Enigo, Key, Keyboard, Mouse, Settings as EnigoSettings,
};
use serde_json::Value;

#[cfg(target_os = "windows")]
use windows::Win32::Foundation::HWND;
#[cfg(target_os = "windows")]
use windows::Win32::UI::WindowsAndMessaging::{
    FindWindowW, GetClientRect, GetWindowRect, SetForegroundWindow, ShowWindow, SW_RESTORE,
};

/// GUI test instance: a running tasty GUI process with IPC access and input simulation.
pub struct GuiTestInstance {
    process: Child,
    port: u16,
    port_file: PathBuf,
    pub enigo: Enigo,
    #[cfg(target_os = "windows")]
    hwnd: HWND,
}

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
            if let Ok(content) = std::fs::read_to_string(&port_file) {
                if let Ok(port) = content.trim().parse::<u16>() {
                    break port;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        // Wait for the window to appear
        #[cfg(target_os = "windows")]
        let hwnd = Self::wait_for_window("Tasty", Duration::from_secs(15));

        // Let the window fully initialize (GPU, terminal, etc.)
        std::thread::sleep(Duration::from_millis(1500));

        let enigo = Enigo::new(&EnigoSettings::default())
            .expect("failed to create enigo instance");

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

    /// Focus the tasty window.
    pub fn focus(&self) {
        #[cfg(target_os = "windows")]
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_RESTORE);
            let _ = SetForegroundWindow(self.hwnd);
        }
        std::thread::sleep(Duration::from_millis(100));
    }

    /// Send a JSON-RPC request and return the result.
    pub fn call(&self, method: &str, params: Value) -> Value {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .expect("failed to connect to tasty IPC");
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .ok();

        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });

        let mut msg = serde_json::to_string(&request).unwrap();
        msg.push('\n');
        stream.write_all(msg.as_bytes()).expect("failed to send IPC");

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("failed to read IPC response");

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


    // --- Input simulation helpers ---

    /// Press a key combination (e.g., Ctrl+Comma).
    pub fn press_key(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo.key(key, Direction::Click).expect("key press failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Press Ctrl + a key.
    pub fn press_ctrl(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo.key(Key::Control, Direction::Press).expect("ctrl press failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo.key(key, Direction::Click).expect("key click failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo.key(Key::Control, Direction::Release).expect("ctrl release failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Press Ctrl+Shift + a key.
    pub fn press_ctrl_shift(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo.key(Key::Control, Direction::Press).expect("ctrl press failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo.key(Key::Shift, Direction::Press).expect("shift press failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo.key(key, Direction::Click).expect("key click failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo.key(Key::Shift, Direction::Release).expect("shift release failed");
        std::thread::sleep(Duration::from_millis(20));
        self.enigo.key(Key::Control, Direction::Release).expect("ctrl release failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Press Alt + a key.
    pub fn press_alt(&mut self, key: Key) {
        self.focus();
        std::thread::sleep(Duration::from_millis(50));
        self.enigo.key(Key::Alt, Direction::Press).expect("alt press failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo.key(key, Direction::Click).expect("key click failed");
        std::thread::sleep(Duration::from_millis(30));
        self.enigo.key(Key::Alt, Direction::Release).expect("alt release failed");
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

        self.enigo.move_mouse(screen_x, screen_y, Coordinate::Abs)
            .expect("mouse move failed");
        std::thread::sleep(Duration::from_millis(50));
        self.enigo.button(enigo::Button::Left, Direction::Click)
            .expect("mouse click failed");
        std::thread::sleep(Duration::from_millis(200));
    }

    /// Get window client area size (width, height).
    #[cfg(target_os = "windows")]
    #[allow(dead_code)]
    pub fn client_size(&self) -> (i32, i32) {
        let mut rect = windows::Win32::Foundation::RECT::default();
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
        unsafe {
            let _ = GetWindowRect(self.hwnd, &mut window_rect);
            let _ = GetClientRect(self.hwnd, &mut client_rect);
        }
        // The client area offset from window top-left
        let border_x = ((window_rect.right - window_rect.left) - (client_rect.right - client_rect.left)) / 2;
        let title_height = (window_rect.bottom - window_rect.top) - (client_rect.bottom - client_rect.top) - border_x;

        (window_rect.left + border_x + x, window_rect.top + title_height + y)
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

    // --- Windows-specific helpers ---

    #[cfg(target_os = "windows")]
    fn wait_for_window(title: &str, timeout: Duration) -> HWND {
        let wide_title: Vec<u16> = title.encode_utf16().chain(std::iter::once(0)).collect();
        let start = Instant::now();
        loop {
            if start.elapsed() > timeout {
                panic!("Window '{}' did not appear within {:?}", title, timeout);
            }
            let hwnd = unsafe {
                FindWindowW(None, windows::core::PCWSTR(wide_title.as_ptr()))
            };
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
        let _ = self.process.kill();
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
