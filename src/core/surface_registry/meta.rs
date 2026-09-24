//! surface 메타데이터를 MemoryStorage의 Surface scope에 저장한다.
//! surface.meta를 통한 쓰기의 소유자는 요청 plugin이 아니라 HOST_OWNER다.
//! 호출자가 확보한 저장소를 받아 락을 다시 얻지 않는다.

use std::collections::HashMap;
use std::io;

use crate::core::pty_registry::is_surface_id_space;
use tasty_memory::{HOST_OWNER, MemoryError, MemoryStorage, MemoryValue, PutOpts, Scope};

fn memory_err_to_io(e: MemoryError) -> io::Error {
    io::Error::other(format!("memory: {e}"))
}

pub struct SurfaceMetaStore;

impl SurfaceMetaStore {
    /// 메모리 저장소는 scope 사전 생성이 필요 없어 성공만 반환한다.
    pub fn ensure_created(_surface_id: u32) -> io::Result<()> {
        Ok(())
    }

    // 닫힘 시 scope 전체 삭제는 AppState의 수명 정리가 맡는다. plugin/Lua가 직접 쓴 키도 함께 처리해야 한다.

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

    /// Text와 문자열로 변환한 JSON을 반환한다. 없음·만료·조회 실패·Binary는 None이다.
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

    /// 없는 키 삭제는 성공으로 처리한다. 다른 저장소 오류는 반환한다.
    pub fn unset(mem: &mut dyn MemoryStorage, surface_id: u32, key: &str) -> io::Result<()> {
        match mem.delete(HOST_OWNER, &Scope::Surface(surface_id), key, None) {
            Ok(()) => Ok(()),
            Err(MemoryError::NotFound { .. }) => Ok(()),
            Err(e) => Err(memory_err_to_io(e)),
        }
    }

    /// Text·JSON만 문자열로 반환한다. 목록 조회 실패는 로그 후 빈 맵이다.
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
                MemoryValue::Binary(_) => {}
            }
        }
        out
    }

    /// PTY 범위 아래의 Surface ID 최대값. 없음·목록 조회 실패는 0이다.
    /// 복원 시 발급 기준을 높일 때 PTY 범위의 기록까지 따라가지 않도록 제외한다.
    #[cfg(any(feature = "gui", test))]
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

    /// PTY 범위에 들어간 Surface scope를 삭제하고 성공한 scope 수를 반환한다.
    /// 목록 조회 실패는 0이고 개별 삭제 실패는 로그 후 계속한다. ID 발급기 자체의 범위 검사는 아니다.
    #[cfg(any(feature = "gui", test))]
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

    /// 전달받은 live 집합 밖의 Surface scope를 삭제한다. 호출자가 완전한 복원 집합을 넘겨야 한다.
    /// 성공한 scope 수를 반환하며 목록·개별 삭제 실패는 로그로 남긴다.
    #[cfg(any(feature = "gui", test))]
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

    /// scope 열거 순서의 첫 key=value surface를 찾는다. 정렬하지 않으며 조회 실패는 None이다.
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
