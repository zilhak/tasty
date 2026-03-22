use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::PathBuf;
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use serde_json::Value;

pub struct TastyInstance {
    process: Child,
    port: u16,
    port_file: PathBuf,
}

impl TastyInstance {
    pub fn spawn() -> Self {
        let port_file = std::env::temp_dir().join(format!(
            "tasty-test-{}-{}.port",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));

        let process = Command::new(env!("CARGO_BIN_EXE_tasty"))
            .arg("--headless")
            .arg("--port-file")
            .arg(port_file.to_str().unwrap())
            .spawn()
            .expect("failed to spawn tasty");

        // Wait for port file
        let start = Instant::now();
        let port = loop {
            if start.elapsed() > Duration::from_secs(10) {
                panic!("tasty failed to start within 10 seconds");
            }
            if let Ok(content) = std::fs::read_to_string(&port_file) {
                if let Ok(port) = content.trim().parse::<u16>() {
                    break port;
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        };

        let instance = Self {
            process,
            port,
            port_file,
        };

        // Wait until the shell is actually ready (has screen content),
        // not a fixed sleep. This guarantees all tests start with a live shell.
        let start = Instant::now();
        loop {
            let text = instance.screen_text();
            if !text.trim().is_empty() {
                break;
            }
            if start.elapsed() > Duration::from_secs(10) {
                panic!("shell did not produce output within 10 seconds");
            }
            std::thread::sleep(Duration::from_millis(50));
        }

        instance
    }

    /// Send a JSON-RPC request and return the result value.
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
        stream.write_all(msg.as_bytes()).expect("failed to send");

        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .expect("failed to read response");

        let resp: Value = serde_json::from_str(&line).expect("invalid JSON response");
        if let Some(error) = resp.get("error") {
            panic!("IPC error: {}", error);
        }
        resp.get("result").cloned().unwrap_or(Value::Null)
    }

    /// Send a JSON-RPC request and return the full response (including errors).
    pub fn call_raw(&self, method: &str, params: Value) -> Value {
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
        stream.write_all(msg.as_bytes()).expect("failed to send");
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        reader.read_line(&mut line).expect("failed to read response");
        serde_json::from_str(&line).expect("invalid JSON response")
    }

    /// Send text to the focused terminal.
    pub fn send_text(&self, text: &str) {
        self.call("surface.send", serde_json::json!({ "text": text }));
    }

    /// Set a read mark on the focused terminal.
    pub fn set_mark(&self) {
        self.call("surface.set_mark", serde_json::json!({}));
    }

    /// Read output since the last mark, stripping ANSI.
    pub fn read_since_mark(&self) -> String {
        let result = self.call(
            "surface.read_since_mark",
            serde_json::json!({ "strip_ansi": true }),
        );
        result
            .get("text")
            .and_then(|t| t.as_str())
            .unwrap_or("")
            .to_string()
    }

    /// Wait until read_since_mark contains the expected text (with timeout).
    pub fn wait_for_output(&self, expected: &str, timeout: Duration) -> String {
        let start = Instant::now();
        loop {
            let output = self.read_since_mark();
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

    /// Get screen text of the focused terminal.
    pub fn screen_text(&self) -> String {
        let result = self.call("surface.screen_text", serde_json::json!({}));
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
        let _ = std::fs::remove_file(&self.port_file);
    }
}
