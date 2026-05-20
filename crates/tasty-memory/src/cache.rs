//! Cache — workspace 단위 TTL-기반 키-값 캐시.
//!
//! `tasty.cache.<key>` 키로 regular memory 영역에 저장된다. 각 entry 는 반드시
//! TTL 을 가져야 하며 (`cache_put { ttl_secs }`), 만료 entry 는 read 시 자동 제외.
//!
//! 사용 시나리오:
//!   - 비용이 큰 계산 결과 캐시 (LLM 응답, 외부 API 호출 등)
//!   - hot-path lookup table
//!
//! `key` 는 호출자가 의미 있는 식별자로 직접 지정한다 (예: 입력의 SHA256 hex
//! 문자열). 검증은 일반 memory key 와 동일 (`[a-z0-9._-]+`, ≤200 chars —
//! `tasty.cache.` prefix 와 합쳐 256 한도 안에 들어오도록).

use crate::{ListOpts, MemoryEntry, MemoryError, MemoryStore, MemoryValue, PutOpts, Result, Scope};

pub const CACHE_KEY_PREFIX: &str = "tasty.cache.";
pub const CACHE_KEY_MAX: usize = 200;

fn validate_cache_key(key: &str) -> Result<()> {
    if key.is_empty() {
        return Err(MemoryError::InvalidKey("cache key: empty".into()));
    }
    if key.len() > CACHE_KEY_MAX {
        return Err(MemoryError::InvalidKey(format!(
            "cache key: too long ({} > {CACHE_KEY_MAX})",
            key.len()
        )));
    }
    for (i, c) in key.bytes().enumerate() {
        let ok =
            c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'.' || c == b'_' || c == b'-';
        if !ok {
            return Err(MemoryError::InvalidKey(format!(
                "cache key: invalid char {:?} at {i}",
                c as char
            )));
        }
    }
    Ok(())
}

fn storage_key(key: &str) -> String {
    format!("{CACHE_KEY_PREFIX}{key}")
}

/// 캐시 entry 쓰기. `ttl_secs` 는 양수 — 0 은 의미가 없어 거부 (`InvalidKey`).
pub fn cache_put(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    key: &str,
    value: &MemoryValue,
    ttl_secs: u64,
) -> Result<u64> {
    validate_cache_key(key)?;
    if ttl_secs == 0 {
        return Err(MemoryError::InvalidKey("ttl_secs must be > 0".into()));
    }
    let now = now_ms_local();
    let add_ms = ttl_secs.saturating_mul(1000).min(i64::MAX as u64) as i64;
    let expires_at = now.saturating_add(add_ms);
    let opts = PutOpts {
        expires_at: Some(expires_at),
        cas: None,
    };
    store.put(
        owner,
        &Scope::Workspace(workspace_id),
        &storage_key(key),
        value,
        &opts,
    )
}

/// 캐시 entry 조회. 만료 / 미존재 → `Ok(None)`.
pub fn cache_get(store: &MemoryStore, workspace_id: u32, key: &str) -> Result<Option<MemoryEntry>> {
    validate_cache_key(key)?;
    store.get(&Scope::Workspace(workspace_id), &storage_key(key))
}

/// 단일 entry 무효화. 없으면 `Ok(())` (idempotent).
pub fn cache_invalidate(
    store: &mut MemoryStore,
    owner: &str,
    workspace_id: u32,
    key: &str,
) -> Result<()> {
    validate_cache_key(key)?;
    let scope = Scope::Workspace(workspace_id);
    let k = storage_key(key);
    match store.delete(owner, &scope, &k, None) {
        Ok(()) => Ok(()),
        Err(MemoryError::NotFound { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

/// workspace 의 모든 캐시 entry 삭제 (owner 가 modify 권한 있는 entry 만).
///
/// Returns: 삭제된 entry 수.
pub fn cache_clear(store: &mut MemoryStore, owner: &str, workspace_id: u32) -> Result<usize> {
    let scope = Scope::Workspace(workspace_id);
    let opts = ListOpts {
        prefix: Some(CACHE_KEY_PREFIX.to_string()),
        ..Default::default()
    };
    let entries = store.list(&scope, &opts)?;
    let mut removed = 0;
    for e in entries {
        store.delete(owner, &scope, &e.key, None)?;
        removed += 1;
    }
    Ok(removed)
}

/// workspace 의 캐시 키 목록 (정렬, prefix 제거된 형태).
pub fn cache_list(store: &MemoryStore, workspace_id: u32) -> Result<Vec<String>> {
    let opts = ListOpts {
        prefix: Some(CACHE_KEY_PREFIX.to_string()),
        ..Default::default()
    };
    let entries = store.list(&Scope::Workspace(workspace_id), &opts)?;
    Ok(entries
        .into_iter()
        .filter_map(|e| e.key.strip_prefix(CACHE_KEY_PREFIX).map(|s| s.to_string()))
        .collect())
}

fn now_ms_local() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::HOST_OWNER;

    fn open() -> MemoryStore {
        MemoryStore::open_in_memory().expect("open in memory")
    }

    #[test]
    fn put_then_get_roundtrip() {
        let mut s = open();
        let v = MemoryValue::Text("hello".into());
        cache_put(&mut s, HOST_OWNER, 1, "k1", &v, 60).unwrap();
        let entry = cache_get(&s, 1, "k1").unwrap().expect("entry");
        assert_eq!(entry.value, v);
        assert!(entry.expires_at.is_some());
    }

    #[test]
    fn invalidate_removes_entry() {
        let mut s = open();
        cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "k",
            &MemoryValue::Text("x".into()),
            60,
        )
        .unwrap();
        cache_invalidate(&mut s, HOST_OWNER, 1, "k").unwrap();
        assert!(cache_get(&s, 1, "k").unwrap().is_none());
    }

    #[test]
    fn invalidate_missing_is_noop() {
        let mut s = open();
        cache_invalidate(&mut s, HOST_OWNER, 1, "ghost").unwrap();
    }

    #[test]
    fn clear_removes_only_cache_entries() {
        let mut s = open();
        cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "a",
            &MemoryValue::Text("1".into()),
            60,
        )
        .unwrap();
        cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "b",
            &MemoryValue::Text("2".into()),
            60,
        )
        .unwrap();
        // 다른 키도 추가해서 캐시만 삭제되는지 검증.
        s.put(
            HOST_OWNER,
            &Scope::Workspace(1),
            "other.key",
            &MemoryValue::Text("keep".into()),
            &PutOpts::default(),
        )
        .unwrap();
        let removed = cache_clear(&mut s, HOST_OWNER, 1).unwrap();
        assert_eq!(removed, 2);
        assert!(cache_get(&s, 1, "a").unwrap().is_none());
        assert!(cache_get(&s, 1, "b").unwrap().is_none());
        assert!(s.get(&Scope::Workspace(1), "other.key").unwrap().is_some());
    }

    #[test]
    fn list_returns_unprefixed_keys() {
        let mut s = open();
        cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "alpha",
            &MemoryValue::Text("x".into()),
            60,
        )
        .unwrap();
        cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "beta",
            &MemoryValue::Text("y".into()),
            60,
        )
        .unwrap();
        let keys = cache_list(&s, 1).unwrap();
        assert_eq!(keys, vec!["alpha".to_string(), "beta".to_string()]);
    }

    #[test]
    fn ttl_zero_rejected() {
        let mut s = open();
        let err = cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "k",
            &MemoryValue::Text("x".into()),
            0,
        )
        .unwrap_err();
        assert!(matches!(err, MemoryError::InvalidKey(_)), "{err:?}");
    }

    #[test]
    fn invalid_key_rejected() {
        let mut s = open();
        let v = MemoryValue::Text("x".into());
        assert!(cache_put(&mut s, HOST_OWNER, 1, "", &v, 60).is_err());
        assert!(cache_put(&mut s, HOST_OWNER, 1, "UPPER", &v, 60).is_err());
        assert!(cache_put(&mut s, HOST_OWNER, 1, "with space", &v, 60).is_err());
        assert!(
            cache_put(
                &mut s,
                HOST_OWNER,
                1,
                &"a".repeat(CACHE_KEY_MAX + 1),
                &v,
                60
            )
            .is_err()
        );
    }

    #[test]
    fn expired_entry_is_none() {
        let mut s = open();
        // expires_at = 과거 시각으로 직접 set.
        let opts = PutOpts {
            expires_at: Some(1),
            cas: None,
        };
        s.put(
            HOST_OWNER,
            &Scope::Workspace(1),
            "tasty.cache.expired",
            &MemoryValue::Text("gone".into()),
            &opts,
        )
        .unwrap();
        assert!(cache_get(&s, 1, "expired").unwrap().is_none());
    }

    #[test]
    fn list_isolated_by_workspace() {
        let mut s = open();
        cache_put(
            &mut s,
            HOST_OWNER,
            1,
            "a",
            &MemoryValue::Text("x".into()),
            60,
        )
        .unwrap();
        cache_put(
            &mut s,
            HOST_OWNER,
            2,
            "b",
            &MemoryValue::Text("y".into()),
            60,
        )
        .unwrap();
        assert_eq!(cache_list(&s, 1).unwrap(), vec!["a".to_string()]);
        assert_eq!(cache_list(&s, 2).unwrap(), vec!["b".to_string()]);
    }
}
