//! Goal — surface 단위 단일 목표 문장.
//!
//! `tasty.goal` 키로 regular memory 영역의 `Scope::Surface(id)` 에 저장된다.
//! bb / plan / cache 와 달리 **prefix 가 아니라 단일 키**다 — goal 은 surface 당
//! 하나뿐이고, 소비자(Stop-훅 게이트 등)가 읽을 자리가 코드에 고정되어야 한다.
//!
//! 스코프가 surface 인 이유: Claude 세션은 surface 단위로 존재한다. workspace 로
//! 올리면 부모와 spawn 된 자식들이 goal 을 공유하게 되어, 자식이 자기 subtask 를
//! 끝냈는데도 부모의 goal 미충족을 이유로 계속 도는 문제가 생긴다. 상속 없음.
//!
//! TTL 이 없는 이유: surface 스코프 데이터는 surface 가 닫힐 때
//! (`purge_surface_memory_scope`) 와 앱 시작 시 복원되지 않은 surface 정리
//! (`purge_dead_surfaces`) 로 scope 통째로 삭제된다. 두 경로 모두 키 필터가 없어
//! goal 도 자동 포함되므로 **goal 수명 = surface 수명** 이며, TTL 을 걸면 만료
//! 전에 purge 가 먼저 지워 도달 불가 코드가 된다.

use crate::{MemoryEntry, MemoryError, MemoryStorage, MemoryValue, PutOpts, Result, Scope};

/// surface goal 이 저장되는 예약 키. prefix 가 아닌 완전한 키다.
pub const GOAL_KEY: &str = "tasty.goal";

/// 빈/공백-only goal 을 거부한다.
///
/// 내용 없는 goal 이 저장되면 소비자가 "내용 없는 목표를 향해 계속 진행하라" 는
/// 절을 주입하게 된다 — 무의미한 값이 게이트 동작을 변질시키는 것을 등록 시점에
/// 막는다. `MemoryError::InvalidKey` 를 쓰는 것은 `cache_put` 의 `ttl_secs` 검증과
/// 같은 선례(도메인 인자 검증 실패를 이 variant 로 표현)를 따른 것이다.
fn validate_goal(goal: &str) -> Result<()> {
    if goal.trim().is_empty() {
        return Err(MemoryError::InvalidKey("goal: empty or blank".into()));
    }
    Ok(())
}

/// goal 설정. 기존 값이 있으면 덮어쓴다(CAS 없음 — 명시적 재선언 행위).
///
/// Returns: 새 `version`.
pub fn goal_set(
    store: &mut dyn MemoryStorage,
    owner: &str,
    surface_id: u32,
    goal: &str,
) -> Result<u64> {
    validate_goal(goal)?;
    store.put(
        owner,
        &Scope::Surface(surface_id),
        GOAL_KEY,
        &MemoryValue::Text(goal.to_string()),
        &PutOpts::default(),
    )
}

/// goal 조회. 미설정 → `Ok(None)`.
pub fn goal_get(store: &dyn MemoryStorage, surface_id: u32) -> Result<Option<MemoryEntry>> {
    store.get(&Scope::Surface(surface_id), GOAL_KEY)
}

/// goal 삭제. 없으면 `Ok(())` (idempotent).
pub fn goal_clear(store: &mut dyn MemoryStorage, owner: &str, surface_id: u32) -> Result<()> {
    match store.delete(owner, &Scope::Surface(surface_id), GOAL_KEY, None) {
        Ok(()) => Ok(()),
        Err(MemoryError::NotFound { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{HOST_OWNER, MemoryStore};

    fn open() -> MemoryStore {
        MemoryStore::open_in_memory().expect("open in memory")
    }

    #[test]
    fn set_then_get_roundtrip() {
        let mut s = open();
        goal_set(&mut s, HOST_OWNER, 3, "테스트를 통과시킨다").unwrap();
        let entry = goal_get(&s, 3).unwrap().expect("entry");
        assert_eq!(entry.value, MemoryValue::Text("테스트를 통과시킨다".into()));
        assert_eq!(entry.key, GOAL_KEY);
        assert_eq!(entry.owner.as_deref(), Some(HOST_OWNER));
    }

    #[test]
    fn get_missing_is_none() {
        let s = open();
        assert!(goal_get(&s, 99).unwrap().is_none());
    }

    #[test]
    fn clear_is_idempotent() {
        let mut s = open();
        goal_set(&mut s, HOST_OWNER, 1, "g").unwrap();
        goal_clear(&mut s, HOST_OWNER, 1).unwrap();
        assert!(goal_get(&s, 1).unwrap().is_none());
        // 두 번째 clear 도 성공해야 한다.
        goal_clear(&mut s, HOST_OWNER, 1).unwrap();
    }

    #[test]
    fn set_overwrites_and_bumps_version() {
        let mut s = open();
        let v1 = goal_set(&mut s, HOST_OWNER, 1, "첫 목표").unwrap();
        let v2 = goal_set(&mut s, HOST_OWNER, 1, "두 번째 목표").unwrap();
        assert!(v2 > v1, "version must increase: {v1} → {v2}");
        let entry = goal_get(&s, 1).unwrap().expect("entry");
        assert_eq!(entry.value, MemoryValue::Text("두 번째 목표".into()));
        assert_eq!(entry.version, v2);
    }

    /// TTL 없음 회귀 방지 — goal 수명은 surface 수명이며 만료로 사라지면 안 된다.
    #[test]
    fn stored_entry_has_no_expiry() {
        let mut s = open();
        goal_set(&mut s, HOST_OWNER, 1, "g").unwrap();
        let entry = goal_get(&s, 1).unwrap().expect("entry");
        assert!(entry.expires_at.is_none(), "goal must not have a TTL");
    }

    #[test]
    fn blank_goal_rejected() {
        let mut s = open();
        assert!(matches!(
            goal_set(&mut s, HOST_OWNER, 1, "").unwrap_err(),
            MemoryError::InvalidKey(_)
        ));
        assert!(matches!(
            goal_set(&mut s, HOST_OWNER, 1, "   \t\n ").unwrap_err(),
            MemoryError::InvalidKey(_)
        ));
        assert!(goal_get(&s, 1).unwrap().is_none());
    }

    #[test]
    fn isolated_by_surface() {
        let mut s = open();
        goal_set(&mut s, HOST_OWNER, 1, "surface 1 목표").unwrap();
        assert!(goal_get(&s, 2).unwrap().is_none());
        goal_set(&mut s, HOST_OWNER, 2, "surface 2 목표").unwrap();
        assert_eq!(
            goal_get(&s, 1).unwrap().unwrap().value,
            MemoryValue::Text("surface 1 목표".into())
        );
        assert_eq!(
            goal_get(&s, 2).unwrap().unwrap().value,
            MemoryValue::Text("surface 2 목표".into())
        );
        // 한쪽 clear 가 다른 쪽에 영향을 주지 않는다.
        goal_clear(&mut s, HOST_OWNER, 1).unwrap();
        assert!(goal_get(&s, 1).unwrap().is_none());
        assert!(goal_get(&s, 2).unwrap().is_some());
    }
}
