//! IPC `session` 단위 테스트.

#![cfg(test)]

use super::*;
use tempfile::TempDir;

fn fresh() -> (TempDir, MemoryStore) {
    let td = tempfile::tempdir().unwrap();
    let mem = MemoryStore::open(&td.path().join("mem.db")).unwrap();
    (td, mem)
}

fn perms(items: &[Permission]) -> Vec<Permission> {
    items.to_vec()
}

#[test]
fn issue_then_resolve_returns_session() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, session) = store
        .issue(
            "child:1",
            Some("com.tasty.claude".into()),
            perms(&[Permission::SurfaceRead, Permission::AgentManage]),
            None,
            1_000,
        )
        .unwrap();
    let resolved = store.resolve(&token, 2_000).unwrap().unwrap();
    assert_eq!(resolved.agent_id, "child:1");
    assert_eq!(resolved.parent.as_deref(), Some("com.tasty.claude"));
    assert!(resolved.permission_set().contains(&Permission::SurfaceRead));
    assert!(resolved.permission_set().contains(&Permission::AgentManage));
    assert_eq!(session.created_at_ms, 1_000);
}

#[test]
fn unknown_token_resolves_to_none() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let fake = SessionToken::generate();
    assert!(store.resolve(&fake, 1_000).unwrap().is_none());
}

#[test]
fn expired_token_is_evicted_on_resolve() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store
        .issue("a1", None, perms(&[]), Some(1_000), 1_000)
        .unwrap();
    // ttl=1000 → expires_at=2000. now=2000 이면 expire.
    assert!(store.resolve(&token, 2_000).unwrap().is_none());
    // 한 번 더 호출해도 None (이미 evict).
    assert!(store.resolve(&token, 3_000).unwrap().is_none());
}

#[test]
fn revoked_token_resolves_to_none() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store
        .issue("a", None, perms(&[Permission::SurfaceRead]), None, 0)
        .unwrap();
    assert!(store.revoke(&token).unwrap());
    assert!(store.resolve(&token, 1_000).unwrap().is_none());
}

#[test]
fn revoke_unknown_returns_false() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let t = SessionToken::generate();
    assert!(!store.revoke(&t).unwrap());
}

#[test]
fn list_returns_alive_only_and_evicts_expired() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (alive_token, _) = store.issue("alive", None, perms(&[]), Some(10_000), 0).unwrap();
    let (revoked_token, _) = store.issue("dead", None, perms(&[]), None, 0).unwrap();
    store.revoke(&revoked_token).unwrap();
    let (_expired_token, _) = store.issue("oldie", None, perms(&[]), Some(1), 0).unwrap();
    let all = store.list(1_000).unwrap();
    assert_eq!(all.len(), 1, "only alive should remain");
    assert_eq!(all[0].agent_id, "alive");
    // alive_token 은 그대로 resolve 가능.
    assert!(store.resolve(&alive_token, 2_000).unwrap().is_some());
}

#[test]
fn empty_agent_id_rejected() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let err = store.issue("", None, perms(&[]), None, 0).unwrap_err();
    assert!(matches!(err, SessionError::InvalidArgument(_)));
}

#[test]
fn grant_then_revoke_removes_temp() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store
        .issue("a", None, perms(&[Permission::SurfaceRead]), None, 0)
        .unwrap();
    store
        .grant_permission(&token, "fs.write", None, 0)
        .unwrap();
    assert!(store.revoke_permission(&token, "fs.write", 1_000).unwrap());
    // 두 번째 revoke 는 false (이미 없음).
    assert!(!store.revoke_permission(&token, "fs.write", 1_000).unwrap());
    let s = store.resolve(&token, 1_000).unwrap().unwrap();
    assert!(s.temp_grants.is_empty(), "revoked grant removed from store");
}

#[test]
fn grant_with_ttl_expires_via_resolve() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store.issue("a", None, perms(&[]), None, 0).unwrap();
    store
        .grant_permission(&token, "fs.write", Some(1_000), 0)
        .unwrap();
    // now=1_000 → grant 만료 (now>=expires).
    let s = store.resolve(&token, 1_000).unwrap().unwrap();
    assert!(s.temp_grants.is_empty(), "expired grant evicted on resolve");
}

#[test]
fn grant_duplicate_extends_expiry() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store.issue("a", None, perms(&[]), None, 0).unwrap();
    // 첫 grant: 짧은 TTL.
    store
        .grant_permission(&token, "fs.write", Some(100), 0)
        .unwrap();
    // 두 번째 grant: 더 긴 TTL → 갱신되어야 함.
    store
        .grant_permission(&token, "fs.write", Some(10_000), 0)
        .unwrap();
    let s = store.resolve(&token, 200).unwrap().unwrap();
    assert_eq!(s.temp_grants.len(), 1);
    assert_eq!(s.temp_grants[0].expires_at_ms, Some(10_000));
}

#[test]
fn grant_none_ttl_overrides_finite() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store.issue("a", None, perms(&[]), None, 0).unwrap();
    store
        .grant_permission(&token, "fs.write", Some(100), 0)
        .unwrap();
    // None TTL → 무기한으로 격상.
    store
        .grant_permission(&token, "fs.write", None, 0)
        .unwrap();
    let s = store.resolve(&token, 100_000).unwrap().unwrap();
    assert_eq!(s.temp_grants.len(), 1);
    assert_eq!(s.temp_grants[0].expires_at_ms, None);
}

#[test]
fn grant_existing_base_permission_skips() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store
        .issue("a", None, perms(&[Permission::SurfaceRead]), None, 0)
        .unwrap();
    // base 에 이미 있으므로 grant 가 noop.
    let added = store
        .grant_permission(&token, "surface.read", Some(1_000), 0)
        .unwrap();
    assert!(!added);
    let s = store.resolve(&token, 100).unwrap().unwrap();
    assert!(s.temp_grants.is_empty());
}

#[test]
fn grant_unknown_permission_rejected() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store.issue("a", None, perms(&[]), None, 0).unwrap();
    let err = store
        .grant_permission(&token, "not.a.real.perm", None, 0)
        .unwrap_err();
    assert!(matches!(err, SessionError::InvalidArgument(_)));
}

#[test]
fn grant_on_unknown_token_rejected() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let fake = SessionToken::generate();
    let err = store
        .grant_permission(&fake, "fs.write", None, 0)
        .unwrap_err();
    assert!(matches!(err, SessionError::InvalidArgument(_)));
}

#[test]
fn find_by_agent_id_returns_match() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store
        .issue("child:42", None, perms(&[Permission::SurfaceRead]), None, 0)
        .unwrap();
    let (found_token, found) = store
        .find_by_agent_id("child:42", 1_000)
        .unwrap()
        .expect("should find");
    assert_eq!(found.agent_id, "child:42");
    assert_eq!(found_token.as_str(), token.as_str());
}

#[test]
fn find_by_agent_id_skips_revoked_and_expired() {
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (rt, _) = store.issue("dup", None, perms(&[]), None, 0).unwrap();
    store.revoke(&rt).unwrap();
    let (_et, _) = store.issue("dup", None, perms(&[]), Some(1), 0).unwrap();
    // 두 후보가 모두 invalid (revoked + expired) → None.
    assert!(store.find_by_agent_id("dup", 10_000).unwrap().is_none());
}

#[test]
fn unknown_permission_tokens_are_dropped_on_load() {
    // 미래에 plugin manifest 에서 permission token 이 사라져도 디스크 데이터는
    // 정상적으로 load 되어야 한다.
    let (_td, mut mem) = fresh();
    let mut store = SessionStore::new(&mut mem, "_host");
    let (token, _) = store
        .issue("a", None, perms(&[Permission::SurfaceRead]), None, 0)
        .unwrap();
    // 강제로 알 수 없는 토큰 삽입.
    let mut session = store.get_raw(&token).unwrap().unwrap();
    session.permissions.push("future.unknown.token".into());
    store.put(&token, &session).unwrap();
    let resolved = store.resolve(&token, 1_000).unwrap().unwrap();
    let set = resolved.permission_set();
    assert!(set.contains(&Permission::SurfaceRead));
    assert_eq!(set.len(), 1, "unknown token dropped");
}
