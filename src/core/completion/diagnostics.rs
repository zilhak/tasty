//! Local installation evidence is separate from the connected server's version.
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};
pub fn cli_version() -> String {
    static VERSION: OnceLock<String> = OnceLock::new();
    VERSION.get_or_init(probe).clone()
}
fn probe() -> String {
    let Ok(mut child) = Command::new("codex")
        .arg("--version")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
    else {
        return "unavailable: direct websocket transport does not require a local proxy".into();
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => {
                return child
                    .wait_with_output()
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
                    .unwrap_or_else(|e| format!("version_read_failed: {e}"));
            }
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                if let Err(error) = child.kill() {
                    tracing::warn!("completion version probe cleanup: {error}");
                }
                if let Err(error) = child.wait() {
                    tracing::warn!("completion version probe wait: {error}");
                }
                return "version_probe_timeout".into();
            }
        }
    }
}
