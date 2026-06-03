use std::collections::VecDeque;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use serde_json::Value;

const STDERR_RING_CAPACITY: usize = 256;
const STDERR_TAIL_LINES: usize = 30;

pub struct TastyInstance {
    process: Child,
    port: u16,
    port_file: PathBuf,
    isolated_home: PathBuf,
    stderr_ring: Arc<Mutex<VecDeque<String>>>,
    stderr_drain: Option<JoinHandle<()>>,
}

fn stderr_tail(ring: &Arc<Mutex<VecDeque<String>>>, n: usize) -> String {
    let ring = ring.lock().unwrap();
    let total = ring.len();
    let start = total.saturating_sub(n);
    ring.iter()
        .skip(start)
        .cloned()
        .collect::<Vec<_>>()
        .join("\n")
}

impl TastyInstance {
    pub fn spawn() -> Self {
        let unique = format!(
            "{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        let port_file = std::env::temp_dir().join(format!("tasty-test-{}.port", unique));

        // 격리된 HOME — 사용자의 ~/.zshrc / oh-my-zsh / p10k / ~/.tasty/ 등이
        // e2e PTY shell 에 새어들어와 prompt 형태와 ZLE binding 을 바꾸는 일을
        // 차단한다. tasty 본 바이너리의 paths 도 모두 HOME 기반이라 같이 격리됨.
        let isolated_home = std::env::temp_dir().join(format!("tasty-test-home-{}", unique));
        std::fs::create_dir_all(&isolated_home).expect("failed to create isolated home");
        // 빈 .zshrc — zsh 가 사용자 customization 안 보게. ZDOTDIR 이 이 디렉토리를
        // 가리키므로 zsh 는 여기서 rc 를 찾는다.
        std::fs::write(isolated_home.join(".zshrc"), "").ok();
        std::fs::write(isolated_home.join(".bashrc"), "").ok();

        let mut process = Command::new(env!("CARGO_BIN_EXE_tasty"))
            .arg("--port-file")
            .arg(port_file.to_str().unwrap())
            .env("HOME", &isolated_home)
            .env("ZDOTDIR", &isolated_home)
            .env_remove("OH_MY_ZSH")
            .env_remove("ZSH")
            .stderr(Stdio::piped())
            .spawn()
            .expect("failed to spawn tasty");

        // drain stderr into a ring buffer so we can attach a tail to spawn-phase
        // panics. background thread avoids OS pipe backpressure blocking the child
        // (Linux 64 KB / macOS 16 KB default).
        let stderr_ring: Arc<Mutex<VecDeque<String>>> =
            Arc::new(Mutex::new(VecDeque::with_capacity(STDERR_RING_CAPACITY)));
        let stderr_drain = process.stderr.take().map(|stderr| {
            let ring = Arc::clone(&stderr_ring);
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for line in reader.lines().map_while(Result::ok) {
                    let mut ring = ring.lock().unwrap();
                    if ring.len() == STDERR_RING_CAPACITY {
                        ring.pop_front();
                    }
                    ring.push_back(line);
                }
            })
        });

        // Wait for port file
        let start = Instant::now();
        let port = loop {
            if start.elapsed() > Duration::from_secs(10) {
                panic!(
                    "tasty failed to start within 10 seconds.\n--- stderr (last {} lines) ---\n{}",
                    STDERR_TAIL_LINES,
                    stderr_tail(&stderr_ring, STDERR_TAIL_LINES)
                );
            }
            if let Ok(content) = std::fs::read_to_string(&port_file)
                && let Ok(port) = content.trim().parse::<u16>()
            {
                break port;
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let instance = Self {
            process,
            port,
            port_file,
            isolated_home,
            stderr_ring: Arc::clone(&stderr_ring),
            stderr_drain,
        };

        // Wait until the shell is actually ready (has screen content).
        let start = Instant::now();
        loop {
            let text = instance.screen_text_of(instance.first_surface_id());
            if !text.trim().is_empty() {
                break;
            }
            if start.elapsed() > Duration::from_secs(10) {
                panic!(
                    "shell did not produce output within 10 seconds.\n--- stderr (last {} lines) ---\n{}",
                    STDERR_TAIL_LINES,
                    stderr_tail(&instance.stderr_ring, STDERR_TAIL_LINES)
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        instance
    }

    /// Send a JSON-RPC request and return the result value.
    /// Retries on timeout (event loop may be slow when window is unfocused).
    pub fn call(&self, method: &str, params: Value) -> Value {
        for attempt in 0..3 {
            let mut stream = match TcpStream::connect(format!("127.0.0.1:{}", self.port)) {
                Ok(s) => s,
                Err(e) => {
                    if attempt < 2 {
                        std::thread::sleep(Duration::from_millis(500));
                        continue;
                    }
                    panic!("failed to connect for '{}': {}", method, e);
                }
            };
            stream.set_read_timeout(Some(Duration::from_secs(10))).ok();

            let request = serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
                "id": 1
            });

            let mut msg = serde_json::to_string(&request).unwrap();
            msg.push('\n');
            if stream.write_all(msg.as_bytes()).is_err() {
                if attempt < 2 {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                panic!("failed to send for '{}'", method);
            }

            let mut reader = BufReader::new(&stream);
            let mut line = String::new();
            match reader.read_line(&mut line) {
                Ok(_) => {}
                Err(_) if attempt < 2 => {
                    std::thread::sleep(Duration::from_millis(500));
                    continue;
                }
                Err(e) => panic!("failed to read response for '{}': {}", method, e),
            }

            let resp: Value = serde_json::from_str(&line).expect("invalid JSON response");
            if let Some(error) = resp.get("error") {
                panic!("IPC error for '{}': {}", method, error);
            }
            return resp.get("result").cloned().unwrap_or(Value::Null);
        }
        unreachable!()
    }

    /// Send a JSON-RPC request and return the full response (including errors).
    pub fn call_raw(&self, method: &str, params: Value) -> Value {
        let mut stream = TcpStream::connect(format!("127.0.0.1:{}", self.port))
            .expect("failed to connect to tasty IPC");
        stream.set_read_timeout(Some(Duration::from_secs(15))).ok();
        let request = serde_json::json!({
            "jsonrpc": "2.0",
            "method": method,
            "params": params,
            "id": 1
        });
        let mut msg = serde_json::to_string(&request).unwrap();
        msg.push('\n');
        stream.write_all(msg.as_bytes()).expect("failed to send");
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("failed to read response");
        serde_json::from_str(&line).expect("invalid JSON response")
    }

    /// Get the first surface ID from surface.list.
    pub fn first_surface_id(&self) -> u64 {
        let surfaces = self.call("surface.list", serde_json::json!({}));
        surfaces.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    /// Get the first pane ID from pane.list.
    pub fn first_pane_id(&self) -> u64 {
        let panes = self.call("pane.list", serde_json::json!({}));
        panes.as_array().unwrap()[0]["id"].as_u64().unwrap()
    }

    /// Send text to a specific surface.
    pub fn send_text(&self, surface_id: u64, text: &str) {
        self.call(
            "surface.send",
            serde_json::json!({ "surface_id": surface_id, "text": text }),
        );
    }

    /// Set a read mark on a specific surface.
    pub fn set_mark(&self, surface_id: u64) {
        self.call(
            "surface.set_mark",
            serde_json::json!({ "surface_id": surface_id }),
        );
    }

    /// Read output since the last mark, stripping ANSI.
    pub fn read_since_mark(&self, surface_id: u64) -> String {
        let result = self.call(
            "surface.read_since_mark",
            serde_json::json!({ "surface_id": surface_id, "strip_ansi": true }),
        );
        result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Wait until read_since_mark contains the expected text (with timeout).
    pub fn wait_for_output(&self, surface_id: u64, expected: &str, timeout: Duration) -> String {
        let start = Instant::now();
        loop {
            let output = self.read_since_mark(surface_id);
            if output.contains(expected) {
                return output;
            }
            if start.elapsed() > timeout {
                panic!(
                    "timeout waiting for '{}' in output. Got:\n{}",
                    expected, output
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    }

    /// Get screen text of a specific surface.
    pub fn screen_text_of(&self, surface_id: u64) -> String {
        let result = self.call(
            "surface.screen_text",
            serde_json::json!({ "surface_id": surface_id }),
        );
        result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Shutdown the instance gracefully.
    pub fn shutdown(&self) {
        let _ = self.call("system.shutdown", serde_json::json!({}));
    }
}

impl Drop for TastyInstance {
    fn drop(&mut self) {
        // Try graceful shutdown
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.shutdown();
        }));
        // Wait briefly, then force kill the entire process tree.
        std::thread::sleep(Duration::from_millis(200));
        #[cfg(target_os = "windows")]
        {
            let pid = self.process.id();
            let _ = Command::new("taskkill")
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
        if let Some(handle) = self.stderr_drain.take() {
            let _ = handle.join(); // join drain thread; ignore panic in drainer
        }
        let _ = std::fs::remove_file(&self.port_file);
        let _ = std::fs::remove_dir_all(&self.isolated_home);
    }
}
