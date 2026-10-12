//! OSC 133의 A·B·C·D 경계를 추적해 surface별 명령 이력을 저장한다. 경계가 없으면 추정하지 않는다.
//! B·C의 cmd= 값과 D의 첫 토큰(exit code)을 읽으며 다른 셸 메타데이터는 무시한다.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::json;
use tasty_memory::writer::{MemoryAccess, MemoryRequest, MemoryTicket};
use tasty_memory::{HOST_OWNER, MemoryValue, PutOpts, Scope};
use tasty_terminal::ResourceGeneration;

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

/// 한도 알림과 그 알림이 속한 터미널 연결.
pub struct CapNotice {
    pub generation: ResourceGeneration,
    pub event: CommandCapEvent,
}

/// writer 안에서 판정한 기록 결과와 그 뒤의 행 수.
enum Recorded {
    Stored(u64),
    Blocked(u64),
}

struct Recording {
    surface_id: u32,
    generation: ResourceGeneration,
    ticket: MemoryTicket,
}

pub struct CommandIndex {
    surfaces: HashMap<u32, Pending>,
    /// 거둔 기록 결과의 행 수. 매번 COUNT 쿼리를 하지 않기 위한 캐시이고 메인 루프만 바꾼다.
    /// writer 요청은 다시 실행될 수 있어 저장소 밖 상태를 바꾸지 않으므로, 결과를 거둘 때 갱신한다.
    counts: HashMap<u32, u64>,
    /// writer에 넣었지만 결과를 아직 거두지 않은 기록.
    recordings: Vec<Recording>,
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
            recordings: Vec::new(),
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

    /// D 경계에서 수집 상태를 비우고 기록을 writer에 넣는다. 한도 판정은 writer 안에서 하고 그 결과는
    /// [`Self::take_cap_notices`]가 거둔다. surface가 없는 headless PTY ID는 제외한다. 이를 Surface
    /// scope로 저장하면 다음 부팅의 ID 기준에 섞인다.
    pub fn on_boundary(
        &mut self,
        memory: &MemoryAccess,
        surface_id: u32,
        generation: ResourceGeneration,
        phase: char,
        payload: &str,
    ) {
        if !crate::runtime::terminal_store::is_surface_id_space(surface_id) {
            return;
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
                self.record_command(memory, surface_id, generation, key_ts, record);
            }
            _ => {}
        }
    }

    fn record_command(
        &mut self,
        memory: &MemoryAccess,
        surface_id: u32,
        generation: ResourceGeneration,
        key_ts: i64,
        record: serde_json::Value,
    ) {
        if self.hard_notified.contains(&surface_id) {
            return;
        }
        let key = format!("tasty.commands.{key_ts}");
        // 캐시가 있으면 아직 거두지 않은 앞 기록이 모두 저장된다고 보고 더한다. 없으면 writer 안에서
        // 센다(앞 기록이 writer 순서로 먼저 반영돼 있다).
        let in_flight = self
            .recordings
            .iter()
            .filter(|recording| recording.surface_id == surface_id)
            .count() as u64;
        let known = self.counts.get(&surface_id).map(|count| count + in_flight);
        let hard_cap = self.hard_cap;
        let bytes = key.len() + record.to_string().len();
        let request = MemoryRequest::unit(bytes, move |store| {
            let scope = Scope::Surface(surface_id);
            let count = match known {
                Some(count) => count,
                None => store.count(&scope, Some("tasty.commands.")).unwrap_or(0),
            };
            if count >= hard_cap {
                return Ok(Recorded::Blocked(count));
            }
            store.put(
                HOST_OWNER,
                &scope,
                &key,
                &MemoryValue::Json(record.clone()),
                &PutOpts::default(),
            )?;
            Ok(Recorded::Stored(count + 1))
        });
        match memory.submit(request) {
            Ok(ticket) => self.recordings.push(Recording {
                surface_id,
                generation,
                ticket,
            }),
            Err(error) => {
                tracing::warn!(
                    "command_index: surface {surface_id} record was not accepted: {error}"
                )
            }
        }
    }

    /// 끝난 기록을 거두고 surface별 한 번의 한도 알림을 돌려준다.
    pub fn take_cap_notices(&mut self) -> Vec<(u32, CapNotice)> {
        let mut notices = Vec::new();
        let mut index = 0;
        while index < self.recordings.len() {
            let Some(completion) = self.recordings[index].ticket.try_take() else {
                index += 1;
                continue;
            };
            let Recording {
                surface_id,
                generation,
                ..
            } = self.recordings.remove(index);
            let Some(event) = self.settle_recording(surface_id, completion) else {
                continue;
            };
            notices.push((surface_id, CapNotice { generation, event }));
        }
        notices
    }

    /// 기록 결과로 행 수 캐시를 고치고, 처음 닿은 한도면 그 알림을 돌려준다.
    fn settle_recording(
        &mut self,
        surface_id: u32,
        completion: tasty_memory::writer::MemoryCompletion,
    ) -> Option<CommandCapEvent> {
        let recorded = match tasty_memory::writer::settle(completion) {
            Ok(output) => output.into_value::<Recorded>(),
            Err(failure) => {
                tracing::warn!(
                    "command_index: memory.put for surface {surface_id} failed: {failure:?}"
                );
                None
            }
        };
        let Some(recorded) = recorded else {
            // 저장 여부를 모르므로 다음 기록이 writer 안에서 다시 센다.
            self.counts.remove(&surface_id);
            return None;
        };
        match recorded {
            Recorded::Blocked(count) => {
                self.counts.insert(surface_id, count);
                if !self.hard_notified.insert(surface_id) {
                    return None;
                }
                let hard_cap = self.hard_cap;
                tracing::warn!(
                    "command_index: surface {surface_id} reached hard cap {hard_cap}; \
                     dropping further command records"
                );
                Some(CommandCapEvent::HardBlocked { surface_id })
            }
            Recorded::Stored(count) => {
                self.counts.insert(surface_id, count);
                (count >= self.soft_cap && self.soft_warned.insert(surface_id))
                    .then_some(CommandCapEvent::SoftWarn { surface_id, count })
            }
        }
    }

    /// 메모리 안의 인덱서 상태만 지운다. 저장된 기록의 scope 정리는 별도다. 거두지 않은 기록의 결과는
    /// 버려 행 수 캐시를 다시 채우지 않는다.
    pub fn drop_surface(&mut self, surface_id: u32) {
        self.surfaces.remove(&surface_id);
        self.recordings
            .retain(|recording| recording.surface_id != surface_id);
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
    use std::sync::{Arc, Mutex};

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

    type MemArc = Arc<Mutex<dyn tasty_memory::MemoryStorage>>;

    fn generation() -> ResourceGeneration {
        tasty_terminal::Terminal::new_detached(1, 1).resource_generation()
    }

    fn store() -> (MemArc, MemoryAccess) {
        let mem: MemArc = Arc::new(Mutex::new(
            tasty_memory::MemoryStore::open_in_memory().unwrap(),
        ));
        let access = MemoryAccess::inline(Arc::clone(&mem));
        (mem, access)
    }

    fn record(idx: &mut CommandIndex, access: &MemoryAccess, sid: u32) -> Vec<CommandCapEvent> {
        idx.on_boundary(access, sid, generation(), 'D', "0");
        idx.take_cap_notices()
            .into_iter()
            .map(|(_, notice)| notice.event)
            .collect()
    }

    #[test]
    fn headless_pty_ids_are_not_indexed() {
        use crate::runtime::terminal_store::PTY_ID_BASE;

        let (mem, access) = store();
        let mut idx = CommandIndex::new();
        let pty_id = PTY_ID_BASE + 499;

        for phase in ['A', 'B', 'C', 'D'] {
            idx.on_boundary(&access, pty_id, generation(), phase, "0");
        }
        assert!(idx.take_cap_notices().is_empty());

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
        let (mem, access) = store();
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
            record(&mut idx, &access, sid).as_slice(),
            [CommandCapEvent::SoftWarn { count: 3, .. }]
        ));
        assert!(record(&mut idx, &access, sid).is_empty());
        assert!(record(&mut idx, &access, sid).is_empty());
        assert!(matches!(
            record(&mut idx, &access, sid).as_slice(),
            [CommandCapEvent::HardBlocked { .. }]
        ));
        assert!(record(&mut idx, &access, sid).is_empty());

        idx.drop_surface(sid);
        assert!(!idx.counts.contains_key(&sid));
        assert!(!idx.soft_warned.contains(&sid));
        assert!(!idx.hard_notified.contains(&sid));
    }

    // 결과를 거두기 전에 같은 surface 기록이 이어져도 캐시에 진행 중 수를 더해 순서대로 센다.
    #[test]
    fn records_submitted_before_their_results_are_counted_in_order() {
        let (_mem, access) = store();
        let sid = 9u32;
        let mut idx = CommandIndex::with_caps(2, 3);
        idx.on_boundary(&access, sid, generation(), 'D', "0");
        assert!(idx.take_cap_notices().is_empty());
        for _ in 0..4 {
            idx.on_boundary(&access, sid, generation(), 'D', "0");
        }
        let events: Vec<_> = idx
            .take_cap_notices()
            .into_iter()
            .map(|(_, notice)| notice.event)
            .collect();
        assert!(matches!(
            events.as_slice(),
            [
                CommandCapEvent::SoftWarn { count: 2, .. },
                CommandCapEvent::HardBlocked { .. }
            ]
        ));
    }

    // writer 요청은 다시 실행될 수 있어 캐시를 바꾸지 않는다. 캐시는 결과를 거둘 때만 바뀌고, 거두기 전
    // 기록은 캐시에 진행 중 수를 더해 판정한다.
    #[test]
    fn the_count_cache_changes_only_when_results_are_reaped() {
        let (mem, access) = store();
        let sid = 11u32;
        let mut idx = CommandIndex::with_caps(100, 3);
        idx.on_boundary(&access, sid, generation(), 'D', "0");
        assert!(!idx.counts.contains_key(&sid), "거두기 전에는 캐시가 없다");
        assert!(idx.take_cap_notices().is_empty());
        assert_eq!(idx.counts.get(&sid), Some(&1));

        // 다른 경로가 행을 지워도 캐시를 믿고 진행 중 수를 더한다. 캐시가 writer 안에서 바뀌었다면
        // 지운 뒤의 행 수로 셌을 것이다.
        mem.lock()
            .unwrap()
            .purge_scope(&Scope::Surface(sid))
            .expect("purge");
        for _ in 0..3 {
            idx.on_boundary(&access, sid, generation(), 'D', "0");
        }
        assert_eq!(idx.counts.get(&sid), Some(&1));
        let events: Vec<_> = idx
            .take_cap_notices()
            .into_iter()
            .map(|(_, notice)| notice.event)
            .collect();
        assert!(matches!(
            events.as_slice(),
            [CommandCapEvent::HardBlocked { .. }]
        ));
        assert_eq!(idx.counts.get(&sid), Some(&3));
    }

    #[test]
    fn on_boundary_persists_exit_code_for_success_and_failure() {
        use tasty_memory::ListOpts;

        let (mem, access) = store();
        let mut idx = CommandIndex::new();

        // 같은 밀리초라도 키가 충돌하지 않도록 서로 다른 surface scope를 쓴다.
        idx.on_boundary(&access, 501, generation(), 'D', "0");
        idx.on_boundary(&access, 502, generation(), 'D', "7");
        assert!(idx.take_cap_notices().is_empty());

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
