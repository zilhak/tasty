//! OSC 133 기반 셸 명령 인덱싱.
//!
//! 셸 통합 (zsh/bash/fish/powershell + iTerm2/kitty 호환 layer) 가 emit 하는
//! `\e]133;{A|B|C|D};...\e\\` 시퀀스를 추적해, 명령 단위로
//! `tasty-memory` 의 `scope=surface:<id>` 위에 `tasty.commands.<unix-ms>` 키로
//! 영속화한다. 셸 통합이 미설치된 surface 는 boundary 가 도착하지 않으므로
//! 침묵 — heuristic fallback 은 후속 phase 에서 도입.
//!
//! Payload 파싱: `;` 으로 split. `key=value` 형태 토큰만 의미 있게 본다.
//! 표준화된 키:
//! - `cmd=<텍스트>` — 사용자가 친 명령 (B 또는 C 페이로드)
//! - `aid=<id>` / `cl=<line>` 등은 ignore (셸별 메타 정보)
//! - `D` 페이로드의 첫 token (`;` 가 아닌 경우) 은 exit code

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tasty_memory::{HOST_OWNER, MemoryValue, PutOpts, Scope, with_store};

/// Per-surface 명령 인덱서 상태.
#[derive(Default)]
pub struct CommandIndex {
    surfaces: HashMap<u32, Pending>,
}

#[derive(Default)]
struct Pending {
    a_at: Option<i64>,
    b_at: Option<i64>,
    c_at: Option<i64>,
    /// `cmd=` 페이로드에서 추출한 명령 텍스트. 셸 통합이 보내준 경우에만.
    command_text: Option<String>,
}

fn unix_ms_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

impl CommandIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// OSC 133 phase 도착 시 호출. `payload` 는 phase 문자 뒤의 `;`-분리 토큰들.
    /// D phase 가 도착하면 memory 에 record 저장 후 per-surface 상태 reset.
    pub fn on_boundary(&mut self, surface_id: u32, phase: char, payload: &str) {
        let now = unix_ms_now();
        let entry = self.surfaces.entry(surface_id).or_default();
        match phase {
            'A' => {
                *entry = Pending::default();
                entry.a_at = Some(now);
            }
            'B' => {
                entry.b_at = Some(now);
                if let Some(cmd) = extract_cmd(payload) {
                    entry.command_text = Some(cmd);
                }
            }
            'C' => {
                entry.c_at = Some(now);
                if entry.command_text.is_none()
                    && let Some(cmd) = extract_cmd(payload)
                {
                    entry.command_text = Some(cmd);
                }
            }
            'D' => {
                let exit_code = extract_exit_code(payload);
                let started_at = entry.a_at.or(entry.b_at).or(entry.c_at);
                let command_started_at = entry.c_at.or(entry.b_at);
                let key_ts = command_started_at.or(started_at).unwrap_or(now);
                let record = json!({
                    "prompt_started_at": entry.a_at,
                    "command_started_at": command_started_at,
                    "ended_at": now,
                    "exit_code": exit_code,
                    "command": entry.command_text,
                });
                let key = format!("tasty.commands.{key_ts}");
                let result = with_store(|s| {
                    s.put(
                        HOST_OWNER,
                        &Scope::Surface(surface_id),
                        &key,
                        &MemoryValue::Json(record),
                        &PutOpts::default(),
                    )
                });
                match result {
                    Some(Ok(_)) => {}
                    Some(Err(e)) => tracing::warn!(
                        "command_index: memory.put for surface {surface_id} '{key}' failed: {e}"
                    ),
                    None => tracing::warn!(
                        "command_index: memory store not initialised; dropping command record"
                    ),
                }
                *entry = Pending::default();
            }
            _ => {}
        }
    }

    /// Surface 가 닫힐 때 호출. 인덱서 상태만 비운다 — memory 에 저장된 record 는
    /// `purge_scope` (`SurfaceMetaStore::remove`) 가 별도로 정리한다.
    pub fn drop_surface(&mut self, surface_id: u32) {
        self.surfaces.remove(&surface_id);
    }
}

fn extract_cmd(payload: &str) -> Option<String> {
    for part in payload.split(';') {
        if let Some(rest) = part.strip_prefix("cmd=") {
            let s = rest.trim();
            if !s.is_empty() {
                return Some(s.to_string());
            }
        }
    }
    None
}

fn extract_exit_code(payload: &str) -> Option<i32> {
    // `D` 페이로드는 첫 토큰이 exit code (예: `D;0` → payload="0", `D;127;aid=x` → "127;aid=x")
    let first = payload.split(';').next()?;
    first.trim().parse::<i32>().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_cmd_from_kvs() {
        assert_eq!(extract_cmd("aid=1;cmd=ls -la;cl=5"), Some("ls -la".to_string()));
        assert_eq!(extract_cmd(""), None);
        assert_eq!(extract_cmd("cmd="), None);
        assert_eq!(extract_cmd("nothing here"), None);
    }

    #[test]
    fn extract_exit_code_basic() {
        assert_eq!(extract_exit_code("0"), Some(0));
        assert_eq!(extract_exit_code("127"), Some(127));
        assert_eq!(extract_exit_code("127;aid=x"), Some(127));
        assert_eq!(extract_exit_code(""), None);
        assert_eq!(extract_exit_code("not-a-num"), None);
    }
}
