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
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope};

type MemArc = Arc<Mutex<dyn MemoryStorage>>;

/// 명령 이력 per-surface soft cap — 도달 시 1회 경고(기록은 계속). 정상 사용(수십~수백)
/// 은 한참 못 미치며, 거의 안 닫히는 장수 shell / 폭주 스크립트 정도만 도달한다.
const COMMAND_SOFT_CAP: u64 = 10_000;
/// per-surface hard cap — 이 이상은 기록을 막는다. 비현실적 폭주가 전역 memory quota
/// 를 잠식해 다른 쓰기(audit/agent 등)까지 망가뜨리는 것을 방지하는 안전밸브.
const COMMAND_HARD_CAP: u64 = 100_000;

/// `on_boundary` 가 cap 임계에 도달했을 때 호출자(cascade)에게 알리는 신호.
/// 호출자가 사용자 알림(`TerminalNotification`)으로 변환한다.
pub enum CommandCapEvent {
    /// soft cap 도달 — 경고만(명령은 계속 기록됨). surface 당 1회.
    SoftWarn {
        // 이유: 호출자(Core::apply_terminal_event)가 이미 sid 를 알고 있어 미사용 —
        // TODO63 (engine.rs → core/ 재배치)로 command_index 가 pub(crate) 로 캡슐화되며 드러남.
        #[allow(dead_code)]
        surface_id: u32,
        count: u64,
    },
    /// hard cap 도달 — 이후 명령 기록 중단. surface 당 1회.
    HardBlocked {
        // 이유: 위 SoftWarn 과 동일 — 호출자가 이미 sid 를 알고 있어 미사용.
        #[allow(dead_code)]
        surface_id: u32,
    },
}

/// Per-surface 명령 인덱서 상태.
pub struct CommandIndex {
    surfaces: HashMap<u32, Pending>,
    /// per-surface `tasty.commands.*` 행 수 캐시. 매 put 마다 COUNT 쿼리를 피하려고
    /// 메모리에 유지 — 첫 'D' 때 1회 조회 후 증분. cap 검사용.
    counts: HashMap<u32, u64>,
    /// soft cap 경고를 surface 당 1회만 보내기 위한 가드.
    soft_warned: std::collections::HashSet<u32>,
    /// hard cap 알림을 surface 당 1회만 보내기 위한 가드.
    hard_notified: std::collections::HashSet<u32>,
    /// per-surface soft/hard cap (기본값은 위 const, 테스트는 작은 값 주입).
    soft_cap: u64,
    hard_cap: u64,
}

impl Default for CommandIndex {
    fn default() -> Self {
        Self {
            surfaces: HashMap::new(),
            counts: HashMap::new(),
            soft_warned: std::collections::HashSet::new(),
            hard_notified: std::collections::HashSet::new(),
            soft_cap: COMMAND_SOFT_CAP,
            hard_cap: COMMAND_HARD_CAP,
        }
    }
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

    #[cfg(test)]
    fn with_caps(soft_cap: u64, hard_cap: u64) -> Self {
        Self {
            soft_cap,
            hard_cap,
            ..Self::default()
        }
    }

    /// OSC 133 phase 도착 시 호출. `payload` 는 phase 문자 뒤의 `;`-분리 토큰들.
    /// D phase 가 도착하면 memory 에 record 저장 후 per-surface 상태 reset.
    /// cap 임계 도달 시 `Some(CommandCapEvent)` 반환(호출자가 사용자 알림으로 변환).
    pub fn on_boundary(
        &mut self,
        memory: &MemArc,
        surface_id: u32,
        phase: char,
        payload: &str,
    ) -> Option<CommandCapEvent> {
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
                *entry = Pending::default();

                let mut guard = match memory.lock() {
                    Ok(g) => g,
                    Err(p) => p.into_inner(),
                };
                // 현재 행 수 확보(첫 'D' 때 1회 인덱스 COUNT, 이후 in-memory 증분).
                let count = *self.counts.entry(surface_id).or_insert_with(|| {
                    guard
                        .count(&Scope::Surface(surface_id), Some("tasty.commands."))
                        .unwrap_or(0)
                });

                // hard cap: 기록 중단(런어웨이 차단). surface 당 1회 알림.
                if count >= self.hard_cap {
                    if self.hard_notified.insert(surface_id) {
                        let hard_cap = self.hard_cap;
                        tracing::warn!(
                            "command_index: surface {surface_id} reached hard cap {hard_cap}; \
                             dropping further command records"
                        );
                        return Some(CommandCapEvent::HardBlocked { surface_id });
                    }
                    return None;
                }

                if let Err(e) = guard.put(
                    HOST_OWNER,
                    &Scope::Surface(surface_id),
                    &key,
                    &MemoryValue::Json(record),
                    &PutOpts::default(),
                ) {
                    tracing::warn!(
                        "command_index: memory.put for surface {surface_id} '{key}' failed: {e}"
                    );
                    return None;
                }
                let new_count = count + 1;
                self.counts.insert(surface_id, new_count);

                // soft cap: 막 도달했으면 1회 경고(기록은 계속).
                if new_count >= self.soft_cap && self.soft_warned.insert(surface_id) {
                    return Some(CommandCapEvent::SoftWarn {
                        surface_id,
                        count: new_count,
                    });
                }
            }
            _ => {}
        }
        None
    }

    /// Surface 가 닫힐 때 호출. 인덱서 상태만 비운다 — memory 에 저장된 record 는
    /// `purge_scope` (`SurfaceMetaStore::remove`) 가 별도로 정리한다.
    pub fn drop_surface(&mut self, surface_id: u32) {
        self.surfaces.remove(&surface_id);
        self.counts.remove(&surface_id);
        self.soft_warned.remove(&surface_id);
        self.hard_notified.remove(&surface_id);
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
        assert_eq!(
            extract_cmd("aid=1;cmd=ls -la;cl=5"),
            Some("ls -la".to_string())
        );
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

    /// 명령 cap: soft 도달 시 1회 경고, hard 도달 시 기록 차단 + 1회 알림.
    #[test]
    fn command_cap_soft_warn_then_hard_block() {
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStore;

        let mem: MemArc = Arc::new(Mutex::new(MemoryStore::open_in_memory().unwrap()));
        let sid = 7u32;
        let mut idx = CommandIndex::with_caps(3, 5);

        // 기존 명령 2건 seed → 다음 'D'(3번째)가 soft cap(3) 에 막 도달.
        {
            let mut g = mem.lock().unwrap();
            for i in 0..2u32 {
                g.put(
                    HOST_OWNER,
                    &Scope::Surface(sid),
                    &format!("tasty.commands.{i:03}"),
                    &MemoryValue::Json(json!({})),
                    &PutOpts::default(),
                )
                .unwrap();
            }
        }

        // 3번째 → count 2→3 == soft → SoftWarn(once).
        assert!(matches!(
            idx.on_boundary(&mem, sid, 'D', "0"),
            Some(CommandCapEvent::SoftWarn { count: 3, .. })
        ));
        // 4번째 → 기록은 계속되지만 soft 경고는 1회뿐.
        assert!(idx.on_boundary(&mem, sid, 'D', "0").is_none());
        // 5번째 → count 4→5.
        assert!(idx.on_boundary(&mem, sid, 'D', "0").is_none());
        // 6번째 → count 5 >= hard(5) → 차단 + HardBlocked(once).
        assert!(matches!(
            idx.on_boundary(&mem, sid, 'D', "0"),
            Some(CommandCapEvent::HardBlocked { .. })
        ));
        // 7번째 → 여전히 차단, 알림은 1회뿐.
        assert!(idx.on_boundary(&mem, sid, 'D', "0").is_none());

        // drop_surface 가 per-surface 상태(카운트/가드)를 정리.
        idx.drop_surface(sid);
        assert!(!idx.counts.contains_key(&sid));
        assert!(!idx.soft_warned.contains(&sid));
        assert!(!idx.hard_notified.contains(&sid));
    }
}
