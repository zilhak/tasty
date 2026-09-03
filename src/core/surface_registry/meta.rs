//! Per-surface metadata store.
//!
//! `surface.meta.*` IPC + 내부 호출자는 모두 이 facade 를 거쳐 `tasty-memory` 의
//! `Scope::Surface(id)` 위 `MemoryValue::Text` entry 로 저장된다. 모든 쓰기는
//! `HOST_OWNER` 로 수행되므로 plugin 이 `surface.meta.set` 으로 쓴 키도 host 가
//! 소유한다 (호환성 보존: 기존 surface.meta API 는 owner 가 없었다).
//!
//! 모든 메서드는 첫 인자로 `mem: &mut dyn MemoryStorage` 를 받는다 — 호출처가
//! `Core::with_memory` / `AppState::with_memory` wrapper 안에서 facade 를
//! 호출한다. Lock 재진입 위험을 막고 *port 단일 진입점* 정책에 부합.
//!
//! 반환 타입은 `io::Result<()>` / `Option<String>` 그대로 유지해 기존 호출자가
//! 영속 실패 시 동작 변경 없이 그대로 동작한다.

use std::collections::HashMap;
use std::io;

use crate::core::pty_registry::is_surface_id_space;
use tasty_memory::{HOST_OWNER, MemoryError, MemoryStorage, MemoryValue, PutOpts, Scope};

fn memory_err_to_io(e: MemoryError) -> io::Error {
    io::Error::other(format!("memory: {e}"))
}

/// File-based per-surface metadata store (now forwarding to `tasty-memory`).
pub struct SurfaceMetaStore;

impl SurfaceMetaStore {
    /// Surface 생성 시 호출. memory.db 는 scope 사전 생성 개념이 없으므로 no-op.
    pub fn ensure_created(_surface_id: u32) -> io::Result<()> {
        Ok(())
    }

    // Surface 닫힘 시의 scope 전체 삭제는 여기 없다 — `AppState::purge_surface_memory_scope`
    // (`src/state.rs`) 가 단독으로 소유한다. 여기에도 같은
    // `purge_scope(Scope::Surface(id))` 래퍼를 두면 close 경로가 같은 풀테이블 스캔을
    // surface 당 2회 돌린다(실제로 그랬다). 게다가 `Scope::Surface` 에는 plugin/Lua 가
    // memory API 로 직접 쓴 키도 들어 있어 scope teardown 은 meta 키 네임스페이스
    // facade 의 관심사가 아니다. **여기에 remove() 를 되살리지 말 것.**

    /// 키 set. 값은 `text/plain` UTF-8 문자열로 저장된다.
    pub fn set(
        mem: &mut dyn MemoryStorage,
        surface_id: u32,
        key: &str,
        value: &str,
    ) -> io::Result<()> {
        mem.put(
            HOST_OWNER,
            &Scope::Surface(surface_id),
            key,
            &MemoryValue::Text(value.to_string()),
            &PutOpts::default(),
        )
        .map(|_| ())
        .map_err(memory_err_to_io)
    }

    /// 키 get. 만료/없음/비문자열 값은 `None`.
    pub fn get(mem: &mut dyn MemoryStorage, surface_id: u32, key: &str) -> Option<String> {
        let entry = mem.get(&Scope::Surface(surface_id), key).ok().flatten()?;
        match entry.value {
            MemoryValue::Text(s) => Some(s),
            MemoryValue::Json(v) => {
                tracing::warn!(
                    "surface.meta.get: key '{key}' on surface {surface_id} holds JSON value; \
                     returning JSON stringified form for back-compat"
                );
                serde_json::to_string(&v).ok()
            }
            MemoryValue::Binary(_) => {
                tracing::warn!(
                    "surface.meta.get: key '{key}' on surface {surface_id} holds binary value; \
                     returning None"
                );
                None
            }
        }
    }

    /// 키 unset. 기존 파일 기반 구현은 키가 없어도 silently OK 였으므로 NotFound 무시.
    pub fn unset(mem: &mut dyn MemoryStorage, surface_id: u32, key: &str) -> io::Result<()> {
        match mem.delete(HOST_OWNER, &Scope::Surface(surface_id), key, None) {
            Ok(()) => Ok(()),
            Err(MemoryError::NotFound { .. }) => Ok(()),
            Err(e) => Err(memory_err_to_io(e)),
        }
    }

    /// 키 list. 문자열로 변환 가능한 값만 반환.
    pub fn list(mem: &mut dyn MemoryStorage, surface_id: u32) -> HashMap<String, String> {
        let entries = match mem.list(
            &Scope::Surface(surface_id),
            &tasty_memory::ListOpts::default(),
        ) {
            Ok(v) => v,
            Err(e) => {
                tracing::warn!("surface.meta.list({surface_id}) failed: {e}");
                return HashMap::new();
            }
        };
        let mut out = HashMap::with_capacity(entries.len());
        for entry in entries {
            match entry.value {
                MemoryValue::Text(s) => {
                    out.insert(entry.key, s);
                }
                MemoryValue::Json(v) => {
                    if let Ok(s) = serde_json::to_string(&v) {
                        out.insert(entry.key, s);
                    }
                }
                MemoryValue::Binary(_) => {
                    // surface.meta is string-only; binary values are skipped silently.
                }
            }
        }
        out
    }

    /// memory.db 에 존재하는 `Scope::Surface(id)` 중 **surface id 공간에 속하는**
    /// (`id < PTY_ID_BASE`) 최대 id (없으면 0).
    ///
    /// 재시작 시 surface 카운터 seed 용 — id 재사용으로 인한 stale 메타 유입을
    /// 막기 위해 복원 직전 카운터 floor 를 `max_surface_id + 1` 로 올린다.
    ///
    /// **PTY id 공간(`>= PTY_ID_BASE`)을 침범한 scope 는 최대값 산정에서 제외한다.**
    /// 포함하면 오염된 scope 하나가 카운터 floor 를 PTY 공간 위로 올리고, 그 실행이
    /// 발급한 surface 들이 다시 memory.db 에 기록되어 floor 가 영구 유지되는 비가역
    /// 래칫이 된다(`docs/adr/0094-surface-id-space-bounded-below-pty-base.md`).
    /// 오염 scope 자체의 제거는 [`purge_out_of_range_surfaces`](Self::purge_out_of_range_surfaces).
    pub fn max_surface_id(mem: &mut dyn MemoryStorage) -> u32 {
        let scopes = match mem.scopes() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("surface_meta max_surface_id: scopes() failed: {e}");
                return 0;
            }
        };
        scopes
            .iter()
            .filter_map(|tok| match Scope::parse(tok) {
                Ok(Scope::Surface(id)) if is_surface_id_space(id) => Some(id),
                _ => None,
            })
            .max()
            .unwrap_or(0)
    }

    /// PTY id 공간(`>= PTY_ID_BASE`)을 침범한 `Scope::Surface` 를 전부 purge.
    /// 반환: 지운 스코프 수.
    ///
    /// 그런 id 를 가진 surface 는 존재할 수 없으므로(surface 카운터는 PTY 공간에
    /// 진입하지 않는다) 여기 남은 것은 방어가 없던 시절의 잔재이거나 검증을 우회한
    /// 쓰기의 산물이다. 부팅 시 한 번 정리해 이미 걸린 래칫을 해소한다.
    pub fn purge_out_of_range_surfaces(mem: &mut dyn MemoryStorage) -> usize {
        let scopes = match mem.scopes() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("surface_meta purge_out_of_range_surfaces: scopes() failed: {e}");
                return 0;
            }
        };
        let polluted: Vec<u32> = scopes
            .iter()
            .filter_map(|tok| match Scope::parse(tok) {
                Ok(Scope::Surface(id)) if !is_surface_id_space(id) => Some(id),
                _ => None,
            })
            .collect();
        let mut purged = 0;
        for id in polluted {
            match mem.purge_scope(&Scope::Surface(id)) {
                Ok(_) => purged += 1,
                Err(e) => {
                    tracing::warn!("surface_meta: purge out-of-range surface:{id} failed: {e}");
                }
            }
        }
        purged
    }

    /// `live` 에 없는 모든 `Scope::Surface(id)` 스코프를 purge. 반환: 지운 스코프 수.
    /// 복원으로 확정된 live id 외 죽은 surface 메타(앱 강제 종료 등으로 graceful
    /// close 의 `AppState::purge_surface_memory_scope` 가 호출되지 못한 잔재)를
    /// 정리해 무한 누적을 막는다.
    pub fn purge_dead_surfaces(
        mem: &mut dyn MemoryStorage,
        live: &std::collections::HashSet<u32>,
    ) -> usize {
        let scopes = match mem.scopes() {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!("surface_meta purge_dead_surfaces: scopes() failed: {e}");
                return 0;
            }
        };
        let dead: Vec<u32> = scopes
            .iter()
            .filter_map(|tok| match Scope::parse(tok) {
                Ok(Scope::Surface(id)) if !live.contains(&id) => Some(id),
                _ => None,
            })
            .collect();
        let mut purged = 0;
        for id in dead {
            match mem.purge_scope(&Scope::Surface(id)) {
                Ok(_) => purged += 1,
                Err(e) => {
                    tracing::warn!("surface_meta GC: purge surface:{id} failed: {e}");
                }
            }
        }
        purged
    }

    /// `Surface(*)` 스코프 전체를 훑어 `key=value` 인 첫 surface id 반환.
    /// 닉네임 기반 pane 조회용. 정렬 보장 없음 (memory 의 scopes() 순서를 그대로 사용).
    pub fn find_by_value(mem: &mut dyn MemoryStorage, key: &str, value: &str) -> Option<u32> {
        let scopes = mem.scopes().ok()?;
        for token in scopes {
            let Ok(Scope::Surface(sid)) = Scope::parse(&token) else {
                continue;
            };
            if Self::get(mem, sid, key).as_deref() == Some(value) {
                return Some(sid);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::pty_registry::PTY_ID_BASE;
    use std::collections::HashSet;
    use tasty_memory::testing::InMemoryStorage;

    fn seed(mem: &mut InMemoryStorage, sid: u32, key: &str, val: &str) {
        SurfaceMetaStore::set(mem, sid, key, val).unwrap();
    }

    #[test]
    fn max_surface_id_picks_largest_surface_scope() {
        let mut mem = InMemoryStorage::new();
        seed(&mut mem, 2, "restore.command", "claude -r a");
        seed(&mut mem, 17, "restore.command", "claude -r b");
        seed(&mut mem, 9, "claude-session-id", "x");
        // 비-surface scope 는 무시되어야 한다.
        mem.put(
            HOST_OWNER,
            &Scope::Workspace(99),
            "k",
            &MemoryValue::Text("v".into()),
            &PutOpts::default(),
        )
        .unwrap();
        assert_eq!(SurfaceMetaStore::max_surface_id(&mut mem), 17);
    }

    #[test]
    fn max_surface_id_empty_is_zero() {
        let mut mem = InMemoryStorage::new();
        assert_eq!(SurfaceMetaStore::max_surface_id(&mut mem), 0);
    }

    #[test]
    fn max_surface_id_ignores_pty_space_scopes() {
        let mut mem = InMemoryStorage::new();
        seed(&mut mem, 3, "restore.command", "claude -r a");
        // PTY id 공간을 침범한 오염 scope — floor 산정에 포함되면 비가역 래칫이 된다.
        seed(&mut mem, PTY_ID_BASE, "restore.command", "polluted");
        seed(&mut mem, PTY_ID_BASE + 499, "claude-session-id", "polluted");
        assert_eq!(
            SurfaceMetaStore::max_surface_id(&mut mem),
            3,
            "PTY 공간 id 는 최대값 산정에서 제외돼야 한다"
        );
    }

    #[test]
    fn purge_out_of_range_removes_only_pty_space_scopes() {
        let mut mem = InMemoryStorage::new();
        seed(&mut mem, 3, "restore.command", "keep");
        seed(&mut mem, PTY_ID_BASE, "restore.command", "drop");
        seed(&mut mem, PTY_ID_BASE + 499, "claude-session-id", "drop");

        let purged = SurfaceMetaStore::purge_out_of_range_surfaces(&mut mem);

        assert_eq!(purged, 2);
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, 3, "restore.command").as_deref(),
            Some("keep")
        );
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, PTY_ID_BASE, "restore.command"),
            None
        );
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, PTY_ID_BASE + 499, "claude-session-id"),
            None
        );
    }

    #[test]
    fn purge_dead_surfaces_keeps_live_removes_rest() {
        let mut mem = InMemoryStorage::new();
        seed(&mut mem, 2, "restore.command", "claude -r a");
        seed(&mut mem, 4, "restore.command", "claude -r stale");
        seed(&mut mem, 6, "restore.command", "claude -r stale2");
        seed(&mut mem, 17, "claude-session-id", "live");

        let live: HashSet<u32> = [2, 17].into_iter().collect();
        let removed = SurfaceMetaStore::purge_dead_surfaces(&mut mem, &live);

        assert_eq!(
            removed, 2,
            "surface:4 와 surface:6 두 scope 가 purge 돼야 한다"
        );
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, 2, "restore.command").as_deref(),
            Some("claude -r a")
        );
        assert_eq!(
            SurfaceMetaStore::get(&mut mem, 17, "claude-session-id").as_deref(),
            Some("live")
        );
        assert_eq!(SurfaceMetaStore::get(&mut mem, 4, "restore.command"), None);
        assert_eq!(SurfaceMetaStore::get(&mut mem, 6, "restore.command"), None);
    }
}
