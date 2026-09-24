//! OSC 133의 A·B·C·D 경계를 추적해 surface별 명령 이력을 저장한다. 경계가 없으면 추정하지 않는다.
//! B·C의 cmd= 값과 D의 첫 토큰(exit code)을 읽으며 다른 셸 메타데이터는 무시한다.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tasty_memory::{HOST_OWNER, MemoryStorage, MemoryValue, PutOpts, Scope};

type MemArc = Arc<Mutex<dyn MemoryStorage>>;

/// surface별 한 번 경고하고 기록은 계속하는 기준.
const COMMAND_SOFT_CAP: u64 = 10_000;
/// 한 surface의 명령 기록이 공용 메모리 용량을 계속 차지하지 못하게 하는 상한.
const COMMAND_HARD_CAP: u64 = 100_000;

/// 호출자가 사용자 알림으로 변환할 기록 한도 이벤트.
pub enum CommandCapEvent {
    /// 경고 후에도 기록을 계속한다.
    SoftWarn {
        // 이유: 호출자가 surface ID를 이미 알고 있어 현재 읽지 않는다.
        #[allow(dead_code)]
        surface_id: u32,
        count: u64,
    },
    /// 이후 기록을 중단한다.
    HardBlocked {
        // 이유: 호출자가 surface ID를 이미 알고 있어 현재 읽지 않는다.
        #[allow(dead_code)]
        surface_id: u32,
    },
}

pub struct CommandIndex {
    surfaces: HashMap<u32, Pending>,
    /// 첫 기록 때 행 수를 읽고 이후 put 성공마다 늘린다. 매번 COUNT 쿼리를 하지 않기 위한 캐시다.
    counts: HashMap<u32, u64>,
    soft_warned: std::collections::HashSet<u32>,
    hard_notified: std::collections::HashSet<u32>,
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

    /// D 경계에서 수집 상태를 비우고 기록을 시도한다. 한도 알림은 surface별 한 번 반환한다.
    /// surface가 없는 headless PTY ID는 제외한다. 이를 Surface scope로 저장하면 다음 부팅의 ID 기준에 섞인다.
    pub fn on_boundary(
        &mut self,
        memory: &MemArc,
        surface_id: u32,
        phase: char,
        payload: &str,
    ) -> Option<CommandCapEvent> {
        if !crate::core::pty_registry::is_surface_id_space(surface_id) {
            return None;
        }
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
                *entry = Pending::default();
                return self.finalize_and_record_command(memory, surface_id, key_ts, record);
            }
            _ => {}
        }
        None
    }

    fn finalize_and_record_command(
        &mut self,
        memory: &MemArc,
        surface_id: u32,
        key_ts: i64,
        record: serde_json::Value,
    ) -> Option<CommandCapEvent> {
        let key = format!("tasty.commands.{key_ts}");

        let mut guard = crate::poison::recover_mutex(
            memory.lock(),
            crate::core::MEMORY_WHAT,
            &crate::core::MEMORY_POISONED,
        );
        let count = *self.counts.entry(surface_id).or_insert_with(|| {
            guard
                .count(&Scope::Surface(surface_id), Some("tasty.commands."))
                .unwrap_or(0)
        });

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

        if new_count >= self.soft_cap && self.soft_warned.insert(surface_id) {
            return Some(CommandCapEvent::SoftWarn {
                surface_id,
                count: new_count,
            });
        }
        None
    }

    /// 메모리 안의 인덱서 상태만 지운다. 저장된 기록의 scope 정리는 별도다.
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

pub(crate) fn extract_exit_code(payload: &str) -> Option<i32> {
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

    #[test]
    fn headless_pty_ids_are_not_indexed() {
        use crate::core::pty_registry::PTY_ID_BASE;
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStore;

        let mem: MemArc = Arc::new(Mutex::new(MemoryStore::open_in_memory().unwrap()));
        let mut idx = CommandIndex::new();
        let pty_id = PTY_ID_BASE + 499;

        for phase in ['A', 'B', 'C', 'D'] {
            assert!(idx.on_boundary(&mem, pty_id, phase, "0").is_none());
        }

        let g = mem.lock().unwrap();
        assert_eq!(
            g.count(&Scope::Surface(pty_id), Some("tasty.commands."))
                .unwrap(),
            0,
            "headless PTY id 로 surface scope 가 생성되면 안 된다"
        );
        assert!(
            g.scopes().unwrap().is_empty(),
            "어떤 scope 도 만들어지지 않아야 한다"
        );
    }

    #[test]
    fn command_cap_soft_warn_then_hard_block() {
        use std::sync::{Arc, Mutex};
        use tasty_memory::MemoryStore;

        let mem: MemArc = Arc::new(Mutex::new(MemoryStore::open_in_memory().unwrap()));
        let sid = 7u32;
        let mut idx = CommandIndex::with_caps(3, 5);

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

        assert!(matches!(
            idx.on_boundary(&mem, sid, 'D', "0"),
            Some(CommandCapEvent::SoftWarn { count: 3, .. })
        ));
        assert!(idx.on_boundary(&mem, sid, 'D', "0").is_none());
        assert!(idx.on_boundary(&mem, sid, 'D', "0").is_none());
        assert!(matches!(
            idx.on_boundary(&mem, sid, 'D', "0"),
            Some(CommandCapEvent::HardBlocked { .. })
        ));
        assert!(idx.on_boundary(&mem, sid, 'D', "0").is_none());

        idx.drop_surface(sid);
        assert!(!idx.counts.contains_key(&sid));
        assert!(!idx.soft_warned.contains(&sid));
        assert!(!idx.hard_notified.contains(&sid));
    }

    #[test]
    fn on_boundary_persists_exit_code_for_success_and_failure() {
        use std::sync::{Arc, Mutex};
        use tasty_memory::{ListOpts, MemoryStore, MemoryValue};

        let mem: MemArc = Arc::new(Mutex::new(MemoryStore::open_in_memory().unwrap()));
        let mut idx = CommandIndex::new();

        // 같은 밀리초라도 키가 충돌하지 않도록 서로 다른 surface scope를 쓴다.
        assert!(idx.on_boundary(&mem, 501, 'D', "0").is_none());
        assert!(idx.on_boundary(&mem, 502, 'D', "7").is_none());

        let read_exit_code = |sid: u32| -> Option<i64> {
            let guard = mem.lock().unwrap();
            let entries = guard
                .list(&Scope::Surface(sid), &ListOpts::default())
                .expect("list should succeed");
            assert_eq!(entries.len(), 1, "surface {sid}에 정확히 1건 기록돼야 한다");
            let MemoryValue::Json(v) = &entries[0].value else {
                panic!("expected Json value");
            };
            v["exit_code"].as_i64()
        };

        assert_eq!(
            read_exit_code(501),
            Some(0),
            "성공(exit 0) 도 exit_code 보존"
        );
        assert_eq!(
            read_exit_code(502),
            Some(7),
            "실패(exit 7) 도 exit_code 보존"
        );
    }
}
