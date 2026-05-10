//! Plugin 자식 프로세스 + 호스트와의 양방향 채널.
//!
//! `PluginProcess::spawn(...)`는:
//! 1. 토큰 생성
//! 2. 자식 프로세스 spawn (env로 host port + token + plugin id 전달, stdout/stderr는 로그 파일)
//! 3. listener에서 token 매칭된 connection 수신 (timeout 10s)
//! 4. 송신/수신 스레드 가동 → mpsc 채널로 호스트 메인 루프에 노출
//!
//! plugin이 응답할 때마다 `last_pong`이 갱신된다. 헬스체크는 `since_last_pong()` 비교.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

use crate::plugin::listener::HostListener;
use crate::plugin::manifest::{PluginPackage, HOST_API_VERSION};
use crate::plugin::protocol::{PluginEvent, PluginRequest, PluginResponse};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(10);

pub struct PluginProcess {
    pub plugin_id: String,
    child: Option<Child>,
    pub req_tx: mpsc::Sender<PluginRequest>,
    pub resp_rx: mpsc::Receiver<PluginResponse>,
    pub event_rx: mpsc::Receiver<PluginEvent>,
    last_pong: Arc<Mutex<Instant>>,
    pub log_path: PathBuf,
}

impl PluginProcess {
    pub fn spawn(
        package: &PluginPackage,
        listener: &HostListener,
        log_dir: &Path,
        waker: tasty_core::SharedWakerFactory,
    ) -> anyhow::Result<Self> {
        let token = generate_token();
        std::fs::create_dir_all(log_dir).ok();
        let log_path = log_dir.join(format!("{}.log", sanitize_id(&package.manifest.id)));
        let log_file = std::fs::File::create(&log_path)?;
        let log_clone = log_file.try_clone()?;

        let entry_path = package.entry_command_path();
        let mut cmd = Command::new(&entry_path);
        cmd.args(package.entry_args())
            .env("TASTY_PLUGIN_ID", &package.manifest.id)
            .env("TASTY_HOST_API_VERSION", HOST_API_VERSION)
            .env("TASTY_HOST_IPC_PORT", listener.port().to_string())
            .env("TASTY_PLUGIN_TOKEN", &token)
            .env("TASTY_PLUGIN_DIR", &package.dir)
            .current_dir(&package.dir)
            .stdin(Stdio::null())
            .stdout(Stdio::from(log_file))
            .stderr(Stdio::from(log_clone));

        // plugin별 격리 디렉터리. 디렉터리 생성은 호스트가 미리 보장한다 — plugin이
        // fs.write 권한 없이도 자기 영역만은 자유롭게 쓸 수 있도록.
        if let Some(home) = tasty_core::paths::tasty_home() {
            let data_dir = home.join("plugin-data").join(&package.manifest.id);
            let config_path = home
                .join("plugin-config")
                .join(format!("{}.toml", &package.manifest.id));
            let _ = std::fs::create_dir_all(&data_dir);
            if let Some(parent) = config_path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            cmd.env("TASTY_PLUGIN_DATA_DIR", &data_dir);
            cmd.env("TASTY_PLUGIN_CONFIG_PATH", &config_path);
            cmd.env("TASTY_PLUGIN_LOG_PATH", &log_path);
        }

        let child = cmd.spawn().map_err(|e| {
            anyhow::anyhow!(
                "failed to spawn plugin '{}' ({}): {}",
                package.manifest.id,
                entry_path.display(),
                e
            )
        })?;

        let stream = match listener.expect_connection(&token, HANDSHAKE_TIMEOUT) {
            Some(s) => s,
            None => {
                anyhow::bail!(
                    "plugin '{}' did not connect within {}s — log: {}",
                    package.manifest.id,
                    HANDSHAKE_TIMEOUT.as_secs(),
                    log_path.display()
                );
            }
        };

        let last_pong = Arc::new(Mutex::new(Instant::now()));
        let (req_tx, req_rx) = mpsc::channel::<PluginRequest>();
        let (resp_tx, resp_rx) = mpsc::channel::<PluginResponse>();
        let (event_tx, event_rx) = mpsc::channel::<PluginEvent>();

        // 송신 스레드
        let mut writer = stream.try_clone()?;
        let plugin_id_tx = package.manifest.id.clone();
        std::thread::Builder::new()
            .name(format!("plugin-tx-{}", sanitize_id(&plugin_id_tx)))
            .spawn(move || {
                for req in req_rx.iter() {
                    let line = match serde_json::to_string(&req) {
                        Ok(s) => s,
                        Err(e) => {
                            tracing::warn!(
                                "plugin '{}' request encode error: {}",
                                plugin_id_tx,
                                e
                            );
                            continue;
                        }
                    };
                    if writeln!(writer, "{line}").is_err() {
                        break;
                    }
                    if writer.flush().is_err() {
                        break;
                    }
                }
            })?;

        // 수신 스레드
        let waker_clone = waker.clone();
        let last_pong_clone = last_pong.clone();
        let plugin_id_rx = package.manifest.id.clone();
        std::thread::Builder::new()
            .name(format!("plugin-rx-{}", sanitize_id(&plugin_id_rx)))
            .spawn(move || {
                let reader = BufReader::new(stream);
                for line in reader.lines() {
                    let line = match line {
                        Ok(l) => l,
                        Err(_) => break,
                    };
                    let trim = line.trim();
                    if trim.is_empty() {
                        continue;
                    }
                    handle_incoming_line(
                        trim,
                        &resp_tx,
                        &event_tx,
                        &last_pong_clone,
                        &plugin_id_rx,
                    );
                    waker_clone.make_default_waker()();
                }
            })?;

        Ok(Self {
            plugin_id: package.manifest.id.clone(),
            child: Some(child),
            req_tx,
            resp_rx,
            event_rx,
            last_pong,
            log_path,
        })
    }

    pub fn ping(&self, next_id: u64) {
        let _ = self.req_tx.send(PluginRequest {
            method: "ping".into(),
            params: serde_json::json!({}),
            id: next_id,
        });
    }

    pub fn since_last_pong(&self) -> Duration {
        self.last_pong
            .lock()
            .map(|t| t.elapsed())
            .unwrap_or(Duration::MAX)
    }

    pub fn shutdown(mut self, timeout: Duration) {
        let _ = self.req_tx.send(PluginRequest {
            method: "shutdown".into(),
            params: serde_json::json!({}),
            id: u64::MAX,
        });
        if let Some(mut child) = self.child.take() {
            let deadline = Instant::now() + timeout;
            loop {
                match child.try_wait() {
                    Ok(Some(_)) => return,
                    Ok(None) => {
                        if Instant::now() > deadline {
                            break;
                        }
                        std::thread::sleep(Duration::from_millis(50));
                    }
                    Err(_) => break,
                }
            }
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

impl Drop for PluginProcess {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

fn handle_incoming_line(
    line: &str,
    resp_tx: &mpsc::Sender<PluginResponse>,
    event_tx: &mpsc::Sender<PluginEvent>,
    last_pong: &Arc<Mutex<Instant>>,
    plugin_id: &str,
) {
    let v: serde_json::Value = match serde_json::from_str(line) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!("plugin '{plugin_id}' sent unparseable line: {e}");
            return;
        }
    };
    if v.get("id").and_then(|x| x.as_u64()).is_some() {
        match serde_json::from_value::<PluginResponse>(v) {
            Ok(resp) => {
                if let Ok(mut p) = last_pong.lock() {
                    *p = Instant::now();
                }
                let _ = resp_tx.send(resp);
            }
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' response decode error: {e}");
            }
        }
        return;
    }
    if let Some(ev_value) = v.get("event") {
        match serde_json::from_value::<PluginEvent>(ev_value.clone()) {
            Ok(ev) => {
                let _ = event_tx.send(ev);
            }
            Err(e) => {
                tracing::warn!("plugin '{plugin_id}' event decode error: {e}");
            }
        }
    }
}

fn sanitize_id(id: &str) -> String {
    id.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '.' || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

fn generate_token() -> String {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    // 단순한 의사 랜덤 — 단계 07에서 강화 가능 (rand 크레이트 등).
    let a = (nanos as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    let b = ((nanos >> 64) as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    format!("{a:016x}{b:016x}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_is_32_hex_chars() {
        let t = generate_token();
        assert_eq!(t.len(), 32);
        assert!(t.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn sanitize_strips_special() {
        assert_eq!(sanitize_id("com.foo/bar:baz"), "com.foo_bar_baz");
        assert_eq!(sanitize_id("com.example-x"), "com.example-x");
    }
}
