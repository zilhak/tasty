//! tasty-memory unit tests — 원본 lib.rs 의 `#[cfg(test)] mod tests` 분리.

#![cfg(test)]

use super::*;

fn store() -> MemoryStore {
    MemoryStore::open_in_memory().unwrap()
}

fn text(s: &str) -> MemoryValue {
    MemoryValue::Text(s.into())
}

const PLUGIN_A: &str = "com.tasty.a";
const PLUGIN_B: &str = "com.tasty.b";

// ---- Regular ----

#[test]
fn put_get_delete_roundtrip() {
    let mut s = store();
    let scope = Scope::Surface(1);

    let v1 = s
        .put(PLUGIN_A, &scope, "a", &text("hello"), &PutOpts::default())
        .unwrap();
    assert_eq!(v1, 1);

    let entry = s.get(&scope, "a").unwrap().unwrap();
    assert_eq!(entry.value, text("hello"));
    assert_eq!(entry.version, 1);
    assert_eq!(entry.owner.as_deref(), Some(PLUGIN_A));

    let v2 = s
        .put(PLUGIN_A, &scope, "a", &text("world"), &PutOpts::default())
        .unwrap();
    assert_eq!(v2, 2);

    s.delete(PLUGIN_A, &scope, "a", None).unwrap();
    assert!(s.get(&scope, "a").unwrap().is_none());
}

#[test]
fn scopes_are_isolated() {
    let mut s = store();
    s.put(
        HOST_OWNER,
        &Scope::Surface(1),
        "k",
        &text("s1"),
        &PutOpts::default(),
    )
    .unwrap();
    s.put(
        HOST_OWNER,
        &Scope::Surface(2),
        "k",
        &text("s2"),
        &PutOpts::default(),
    )
    .unwrap();
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "k",
        &text("g"),
        &PutOpts::default(),
    )
    .unwrap();
    assert_eq!(
        s.get(&Scope::Surface(1), "k").unwrap().unwrap().value,
        text("s1")
    );
    assert_eq!(
        s.get(&Scope::Surface(2), "k").unwrap().unwrap().value,
        text("s2")
    );
    assert_eq!(
        s.get(&Scope::Global, "k").unwrap().unwrap().value,
        text("g")
    );
}

#[test]
fn cas_conflict_blocks_update() {
    let mut s = store();
    let scope = Scope::Workspace(1);
    s.put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
        .unwrap();

    let err = s
        .put(
            PLUGIN_A,
            &scope,
            "k",
            &text("v2"),
            &PutOpts {
                cas: Some(99),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(
        err,
        MemoryError::CasConflict {
            actual: 1,
            expected: 99
        }
    ));

    s.put(
        PLUGIN_A,
        &scope,
        "k",
        &text("v2"),
        &PutOpts {
            cas: Some(1),
            ..Default::default()
        },
    )
    .unwrap();
}

#[test]
fn regular_owned_by_other_on_update() {
    let mut s = store();
    let scope = Scope::Global;
    s.put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
        .unwrap();

    let err = s
        .put(PLUGIN_B, &scope, "k", &text("v2"), &PutOpts::default())
        .unwrap_err();
    let owner = match err {
        MemoryError::OwnedByOther { owner } => owner,
        other => panic!("expected OwnedByOther, got {other:?}"),
    };
    assert_eq!(owner, PLUGIN_A);

    // 원래 owner는 정상.
    s.put(PLUGIN_A, &scope, "k", &text("v3"), &PutOpts::default())
        .unwrap();
    // _host는 root로 통과.
    s.put(HOST_OWNER, &scope, "k", &text("v4"), &PutOpts::default())
        .unwrap();
}

#[test]
fn regular_owned_by_other_on_delete() {
    let mut s = store();
    let scope = Scope::Global;
    s.put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
        .unwrap();

    let err = s.delete(PLUGIN_B, &scope, "k", None).unwrap_err();
    assert!(matches!(err, MemoryError::OwnedByOther { .. }));

    // _host는 root로 통과해 삭제 가능.
    s.delete(HOST_OWNER, &scope, "k", None).unwrap();
    assert!(s.get(&scope, "k").unwrap().is_none());
}

#[test]
fn read_is_shared_across_callers() {
    let mut s = store();
    let scope = Scope::Global;
    s.put(PLUGIN_A, &scope, "k", &text("v"), &PutOpts::default())
        .unwrap();
    // Plugin B도 읽을 수 있고, 응답에 owner=PLUGIN_A가 보인다.
    let entry = s.get(&scope, "k").unwrap().unwrap();
    assert_eq!(entry.owner.as_deref(), Some(PLUGIN_A));

    let list = s.list(&scope, &ListOpts::default()).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].owner.as_deref(), Some(PLUGIN_A));
}

#[test]
fn expired_keys_treated_as_missing() {
    let mut s = store();
    let scope = Scope::Surface(1);
    let past = unix_ms_now() - 1000;
    s.put(
        HOST_OWNER,
        &scope,
        "k",
        &text("v"),
        &PutOpts {
            expires_at: Some(past),
            ..Default::default()
        },
    )
    .unwrap();
    assert!(s.get(&scope, "k").unwrap().is_none());
    assert_eq!(s.count(&scope, None).unwrap(), 0);
    assert!(s.list(&scope, &ListOpts::default()).unwrap().is_empty());
}

#[test]
fn value_size_cap_enforced() {
    let mut s = store();
    let cap = s.config().entry_max_bytes as usize;
    let big = vec![0u8; cap + 1];
    let err = s
        .put(
            HOST_OWNER,
            &Scope::Global,
            "k",
            &MemoryValue::Binary(big),
            &PutOpts::default(),
        )
        .unwrap_err();
    assert!(matches!(err, MemoryError::ValueTooLarge { .. }));
}

#[test]
fn entry_max_configurable() {
    let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
        entry_max_bytes: 16,
        ..MemoryConfig::default()
    })
    .unwrap();
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "k",
        &text("0123456789ab"),
        &PutOpts::default(),
    )
    .unwrap();
    let err = s
        .put(
            HOST_OWNER,
            &Scope::Global,
            "big",
            &text("0123456789abcdefghij"),
            &PutOpts::default(),
        )
        .unwrap_err();
    assert!(matches!(err, MemoryError::ValueTooLarge { actual, max } if max == 16 && actual == 20));
}

#[test]
fn regular_quota_exceeded() {
    let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
        entry_max_bytes: 1024,
        regular_quota_total_bytes: 20,
        ..MemoryConfig::default()
    })
    .unwrap();
    // 12 byte 저장: 합산 12 ≤ 20 통과.
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "a",
        &text("0123456789ab"),
        &PutOpts::default(),
    )
    .unwrap();
    // 새 entry 12 byte 추가 시 projected=24 → 거부.
    let err = s
        .put(
            HOST_OWNER,
            &Scope::Global,
            "b",
            &text("0123456789ab"),
            &PutOpts::default(),
        )
        .unwrap_err();
    match err {
        MemoryError::QuotaExceeded { area, used, limit } => {
            assert_eq!(area, MemoryArea::Regular);
            assert_eq!(used, 24);
            assert_eq!(limit, 20);
        }
        other => panic!("expected QuotaExceeded, got {other:?}"),
    }
    // 기존 entry 의 in-place 갱신은 existing_size 만큼 차감 후 평가 → 같은 크기면 통과.
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "a",
        &text("ABCDEFGHIJKL"),
        &PutOpts::default(),
    )
    .unwrap();
}

#[test]
fn secret_quota_exceeded_per_owner() {
    // entry 1개 = 12 byte. 한도 20 이면 1 통과, 2 거부.
    let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
        entry_max_bytes: 1024,
        secret_quota_per_owner_bytes: 20,
        ..MemoryConfig::default()
    })
    .unwrap();
    s.put_secret(
        PLUGIN_A,
        &Scope::Global,
        "a",
        &text("0123456789ab"),
        &PutOpts::default(),
    )
    .unwrap();
    let err = s
        .put_secret(
            PLUGIN_A,
            &Scope::Global,
            "b",
            &text("0123456789ab"),
            &PutOpts::default(),
        )
        .unwrap_err();
    assert!(matches!(
        err,
        MemoryError::QuotaExceeded {
            area: MemoryArea::Secret,
            ..
        }
    ));
    // Plugin B 영역은 독립.
    s.put_secret(
        PLUGIN_B,
        &Scope::Global,
        "a",
        &text("0123456789ab"),
        &PutOpts::default(),
    )
    .unwrap();
}

#[test]
fn invalid_key_rejected() {
    let mut s = store();
    let err = s
        .put(
            HOST_OWNER,
            &Scope::Global,
            "BAD",
            &text("x"),
            &PutOpts::default(),
        )
        .unwrap_err();
    assert!(matches!(err, MemoryError::InvalidKey(_)));
}

#[test]
fn invalid_owner_rejected() {
    let mut s = store();
    let err = s
        .put("", &Scope::Global, "k", &text("x"), &PutOpts::default())
        .unwrap_err();
    assert!(matches!(err, MemoryError::InvalidOwner(_)));
}

#[test]
fn delete_missing_returns_not_found() {
    let mut s = store();
    let err = s
        .delete(HOST_OWNER, &Scope::Global, "ghost", None)
        .unwrap_err();
    assert!(matches!(err, MemoryError::NotFound { .. }));
}

#[test]
fn list_prefix_and_limit() {
    let mut s = store();
    let scope = Scope::Surface(1);
    for k in ["a.1", "a.2", "b.1", "b.2", "c.1"] {
        s.put(HOST_OWNER, &scope, k, &text(k), &PutOpts::default())
            .unwrap();
    }
    let all = s.list(&scope, &ListOpts::default()).unwrap();
    assert_eq!(all.len(), 5);
    let a_only = s
        .list(
            &scope,
            &ListOpts {
                prefix: Some("a.".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        a_only.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        vec!["a.1", "a.2"]
    );
    let limited = s
        .list(
            &scope,
            &ListOpts {
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(limited.len(), 2);
    assert_eq!(s.count(&scope, None).unwrap(), 5);
    assert_eq!(s.count(&scope, Some("b.")).unwrap(), 2);
}

// ---- Secret ----

#[test]
fn secret_isolated_between_owners() {
    let mut s = store();
    let scope = Scope::Global;
    s.put_secret(
        PLUGIN_A,
        &scope,
        "tok",
        &text("A-token"),
        &PutOpts::default(),
    )
    .unwrap();
    s.put_secret(
        PLUGIN_B,
        &scope,
        "tok",
        &text("B-token"),
        &PutOpts::default(),
    )
    .unwrap();

    // 같은 (scope, key)지만 owner별로 분리 — Plugin A는 자기 값만 본다.
    let a = s.get_secret(PLUGIN_A, &scope, "tok").unwrap().unwrap();
    assert_eq!(a.value, text("A-token"));
    assert!(a.owner.is_none(), "secret 응답에는 owner 노출 금지");

    let b = s.get_secret(PLUGIN_B, &scope, "tok").unwrap().unwrap();
    assert_eq!(b.value, text("B-token"));

    // Plugin A가 자기 영역만 본다.
    let list_a = s
        .list_secret(PLUGIN_A, &scope, &ListOpts::default())
        .unwrap();
    assert_eq!(list_a.len(), 1);

    let scopes_a = s.scopes_secret(PLUGIN_A).unwrap();
    assert_eq!(scopes_a, vec!["global"]);
}

#[test]
fn secret_delete_only_affects_owner() {
    let mut s = store();
    let scope = Scope::Workspace(1);
    s.put_secret(PLUGIN_A, &scope, "tok", &text("A"), &PutOpts::default())
        .unwrap();
    s.put_secret(PLUGIN_B, &scope, "tok", &text("B"), &PutOpts::default())
        .unwrap();
    s.delete_secret(PLUGIN_A, &scope, "tok", None).unwrap();
    assert!(s.get_secret(PLUGIN_A, &scope, "tok").unwrap().is_none());
    assert!(s.get_secret(PLUGIN_B, &scope, "tok").unwrap().is_some());
}

#[test]
fn secret_cas_and_versioning() {
    let mut s = store();
    let scope = Scope::Global;
    let v1 = s
        .put_secret(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
        .unwrap();
    assert_eq!(v1, 1);
    let v2 = s
        .put_secret(
            PLUGIN_A,
            &scope,
            "k",
            &text("v2"),
            &PutOpts {
                cas: Some(1),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(v2, 2);
    let err = s
        .put_secret(
            PLUGIN_A,
            &scope,
            "k",
            &text("v3"),
            &PutOpts {
                cas: Some(99),
                ..Default::default()
            },
        )
        .unwrap_err();
    assert!(matches!(err, MemoryError::CasConflict { .. }));
}

#[test]
fn secret_stats_per_owner() {
    let mut s = store();
    s.put_secret(
        PLUGIN_A,
        &Scope::Global,
        "a",
        &text("xx"),
        &PutOpts::default(),
    )
    .unwrap();
    s.put_secret(
        PLUGIN_B,
        &Scope::Global,
        "a",
        &text("zzzz"),
        &PutOpts::default(),
    )
    .unwrap();

    // stats 는 평문 byte 를 그대로 보고한다 (암호화 안 함).
    let a = s.stats_secret(PLUGIN_A, None).unwrap();
    assert_eq!(a.entries, 1);
    assert_eq!(a.bytes, 2);

    let b = s.stats_secret(PLUGIN_B, None).unwrap();
    assert_eq!(b.entries, 1);
    assert_eq!(b.bytes, 4);
}

/// Secret value 는 평문 BLOB 으로 저장된다. 보호는 IPC owner 분리까지만.
/// DB 파일을 직접 여는 행위자는 secret 을 평문으로 본다.
#[test]
fn secret_at_rest_is_plaintext() {
    let mut s = store();
    let secret = "hunter2-supersecret-token-zzz";
    s.put_secret(
        PLUGIN_A,
        &Scope::Global,
        "tok",
        &text(secret),
        &PutOpts::default(),
    )
    .unwrap();
    let blob: Vec<u8> = s
        .conn
        .query_row(
            "SELECT value FROM memory_secret WHERE owner=?1 AND scope='global' AND key='tok'",
            params![PLUGIN_A],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(blob, secret.as_bytes());
}

// ---- GC ----

#[test]
fn purge_expired_removes_only_expired_rows() {
    let mut s = store();
    let scope = Scope::Workspace(1);

    // 영구 entry
    s.put(
        PLUGIN_A,
        &scope,
        "permanent",
        &text("p"),
        &PutOpts::default(),
    )
    .unwrap();
    // 이미 만료된 regular entry (expires_at = 과거)
    s.put(
        PLUGIN_A,
        &scope,
        "expired_reg",
        &text("r"),
        &PutOpts {
            expires_at: Some(1),
            cas: None,
        },
    )
    .unwrap();
    // 만료된 secret entry
    s.put_secret(
        PLUGIN_B,
        &scope,
        "expired_sec",
        &text("s"),
        &PutOpts {
            expires_at: Some(1),
            cas: None,
        },
    )
    .unwrap();

    // read 시 expired 는 not-found
    assert!(s.get(&scope, "expired_reg").unwrap().is_none());
    assert!(
        s.get_secret(PLUGIN_B, &scope, "expired_sec")
            .unwrap()
            .is_none()
    );

    // purge 전에는 row 가 디스크에 남아 있다
    let count_before: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_before, 2);

    let stats = s.purge_expired().unwrap();
    assert_eq!(stats.regular, 1);
    assert_eq!(stats.secret, 1);

    let count_after: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(count_after, 1, "permanent entry must remain");
    let sec_after: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM memory_secret", [], |r| r.get(0))
        .unwrap();
    assert_eq!(sec_after, 0);
}

#[test]
fn purge_scope_clears_both_areas_for_that_scope_only() {
    let mut s = store();
    let target = Scope::Surface(7);
    let other = Scope::Surface(8);

    s.put(PLUGIN_A, &target, "a", &text("x"), &PutOpts::default())
        .unwrap();
    s.put_secret(PLUGIN_A, &target, "sa", &text("y"), &PutOpts::default())
        .unwrap();
    s.put_secret(PLUGIN_B, &target, "sb", &text("z"), &PutOpts::default())
        .unwrap();

    // 다른 scope 의 entry 는 건드리지 않는다
    s.put(PLUGIN_A, &other, "keep", &text("k"), &PutOpts::default())
        .unwrap();

    let stats = s.purge_scope(&target).unwrap();
    assert_eq!(stats.regular, 1);
    assert_eq!(stats.secret, 2);

    assert!(s.get(&target, "a").unwrap().is_none());
    assert!(s.get_secret(PLUGIN_A, &target, "sa").unwrap().is_none());
    assert!(s.get_secret(PLUGIN_B, &target, "sb").unwrap().is_none());
    assert!(s.get(&other, "keep").unwrap().is_some());
}

// ---- Change events ----

#[test]
fn put_records_created_then_updated_change() {
    let mut s = store();
    let scope = Scope::Workspace(3);
    let _ = s.take_pending_changes();

    let v1 = s
        .put(PLUGIN_A, &scope, "k", &text("v1"), &PutOpts::default())
        .unwrap();
    let changes = s.take_pending_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, MemoryChangeKind::Created);
    assert_eq!(changes[0].key, "k");
    assert_eq!(changes[0].scope, scope.as_token());
    assert_eq!(changes[0].version, Some(v1));

    let v2 = s
        .put(PLUGIN_A, &scope, "k", &text("v2"), &PutOpts::default())
        .unwrap();
    let changes = s.take_pending_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, MemoryChangeKind::Updated);
    assert_eq!(changes[0].version, Some(v2));

    // 두 번째 take 는 빈 vec
    assert!(s.take_pending_changes().is_empty());
}

#[test]
fn delete_records_deleted_change() {
    let mut s = store();
    let scope = Scope::Workspace(3);
    s.put(PLUGIN_A, &scope, "k", &text("v"), &PutOpts::default())
        .unwrap();
    let _ = s.take_pending_changes();

    s.delete(PLUGIN_A, &scope, "k", None).unwrap();
    let changes = s.take_pending_changes();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].kind, MemoryChangeKind::Deleted);
    assert_eq!(changes[0].key, "k");
    assert!(changes[0].version.is_none());
}

#[test]
fn secret_change_does_not_emit_event() {
    let mut s = store();
    let scope = Scope::Workspace(3);
    let _ = s.take_pending_changes();

    s.put_secret(PLUGIN_A, &scope, "sk", &text("v"), &PutOpts::default())
        .unwrap();
    s.delete_secret(PLUGIN_A, &scope, "sk", None).unwrap();
    assert!(
        s.take_pending_changes().is_empty(),
        "secret changes must not be broadcast"
    );
}

#[test]
fn purge_expired_records_expired_changes_for_regular_only() {
    let mut s = store();
    let scope = Scope::Workspace(3);
    s.put(
        PLUGIN_A,
        &scope,
        "exp_r",
        &text("r"),
        &PutOpts {
            expires_at: Some(1),
            cas: None,
        },
    )
    .unwrap();
    s.put_secret(
        PLUGIN_A,
        &scope,
        "exp_s",
        &text("s"),
        &PutOpts {
            expires_at: Some(1),
            cas: None,
        },
    )
    .unwrap();
    let _ = s.take_pending_changes();

    let stats = s.purge_expired().unwrap();
    assert_eq!(stats.regular, 1);
    assert_eq!(stats.secret, 1);
    let changes = s.take_pending_changes();
    assert_eq!(changes.len(), 1, "only regular expired key emits event");
    assert_eq!(changes[0].kind, MemoryChangeKind::Expired);
    assert_eq!(changes[0].key, "exp_r");
}

// ---- Pagination + query + export/import ----

#[test]
fn list_supports_offset_limit_since_until() {
    let mut s = store();
    let scope = Scope::Workspace(9);
    for k in ["a", "b", "c", "d", "e"] {
        s.put(PLUGIN_A, &scope, k, &text(k), &PutOpts::default())
            .unwrap();
    }
    // 전체
    let all = s.list(&scope, &ListOpts::default()).unwrap();
    assert_eq!(
        all.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        ["a", "b", "c", "d", "e"]
    );

    // offset + limit
    let page = s
        .list(
            &scope,
            &ListOpts {
                offset: Some(1),
                limit: Some(2),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(
        page.iter().map(|e| e.key.as_str()).collect::<Vec<_>>(),
        ["b", "c"]
    );

    // since / until — 모두 같은 updated_at 이라 since=now+1 이면 0개
    let now = unix_ms_now();
    let future = s
        .list(
            &scope,
            &ListOpts {
                since: Some(now + 60_000),
                ..Default::default()
            },
        )
        .unwrap();
    assert!(future.is_empty());
}

#[test]
fn query_filters_by_dot_path_equality() {
    let mut s = store();
    let scope = Scope::Workspace(11);
    let make = |status: &str| {
        MemoryValue::Json(serde_json::json!({
            "task": { "status": status, "id": 1 }
        }))
    };
    s.put(PLUGIN_A, &scope, "t.1", &make("open"), &PutOpts::default())
        .unwrap();
    s.put(
        PLUGIN_A,
        &scope,
        "t.2",
        &make("closed"),
        &PutOpts::default(),
    )
    .unwrap();
    s.put(PLUGIN_A, &scope, "t.3", &make("open"), &PutOpts::default())
        .unwrap();
    // 텍스트 entry 는 query 에서 자동 제외
    s.put(
        PLUGIN_A,
        &scope,
        "x",
        &text("not-json"),
        &PutOpts::default(),
    )
    .unwrap();

    let hits = s
        .query(
            &scope,
            "task.status",
            &serde_json::json!("open"),
            &ListOpts::default(),
        )
        .unwrap();
    let mut keys: Vec<&str> = hits.iter().map(|e| e.key.as_str()).collect();
    keys.sort();
    assert_eq!(keys, ["t.1", "t.3"]);

    // path 가 존재하지 않으면 0개
    let none = s
        .query(
            &scope,
            "task.nope",
            &serde_json::json!("open"),
            &ListOpts::default(),
        )
        .unwrap();
    assert!(none.is_empty());
}

#[test]
fn export_and_import_roundtrip() {
    let mut s = store();
    let ws = Scope::Workspace(20);
    let sf = Scope::Surface(20);
    s.put(PLUGIN_A, &ws, "alpha", &text("a"), &PutOpts::default())
        .unwrap();
    s.put(
        PLUGIN_A,
        &sf,
        "beta",
        &MemoryValue::Json(serde_json::json!({"v":1})),
        &PutOpts::default(),
    )
    .unwrap();
    // Secret entry 가 있어도 export 에는 포함되지 않아야 한다
    s.put_secret(
        PLUGIN_A,
        &ws,
        "secret_k",
        &text("hidden"),
        &PutOpts::default(),
    )
    .unwrap();

    let exported = s.export_regular(None).unwrap();
    assert_eq!(exported.len(), 2);
    assert!(exported.iter().all(|e| e.scope != "secret"));

    // 다른 store 로 import
    let mut s2 = store();
    let stats = s2.import_regular(HOST_OWNER, &exported, false).unwrap();
    assert_eq!(stats.applied, 2);
    assert_eq!(stats.skipped, 0);

    // 두 entry 모두 복원
    assert!(s2.get(&ws, "alpha").unwrap().is_some());
    assert!(s2.get(&sf, "beta").unwrap().is_some());

    // 같은 store 에 다시 import (replace=false) → skip
    let stats = s2.import_regular(HOST_OWNER, &exported, false).unwrap();
    assert_eq!(stats.applied, 0);
    assert_eq!(stats.skipped, 2);

    // replace=true → 모두 applied
    let stats = s2.import_regular(HOST_OWNER, &exported, true).unwrap();
    assert_eq!(stats.applied, 2);
    assert_eq!(stats.skipped, 0);
}

#[test]
fn purge_scope_records_deleted_for_each_regular_key() {
    let mut s = store();
    let scope = Scope::Surface(11);
    s.put(PLUGIN_A, &scope, "a", &text("x"), &PutOpts::default())
        .unwrap();
    s.put(PLUGIN_A, &scope, "b", &text("y"), &PutOpts::default())
        .unwrap();
    s.put_secret(PLUGIN_A, &scope, "sa", &text("z"), &PutOpts::default())
        .unwrap();
    let _ = s.take_pending_changes();

    s.purge_scope(&scope).unwrap();
    let mut changes = s.take_pending_changes();
    changes.sort_by(|a, b| a.key.cmp(&b.key));
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0].kind, MemoryChangeKind::Deleted);
    assert_eq!(changes[0].key, "a");
    assert_eq!(changes[1].kind, MemoryChangeKind::Deleted);
    assert_eq!(changes[1].key, "b");
}
