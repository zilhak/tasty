//! tasty-memory unit tests — 원본 lib.rs 의 `#[cfg(test)] mod tests` 분리.

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

/// owner 검사는 **이미 있는 행**만 지킨다 — 아직 없는 키는 누구든 만들 수 있고,
/// 그러면 그 키의 owner 가 된다. 그래서 `tasty.audit.` 같은 호스트 namespace 는
/// "호스트가 이미 쓴 키" 만 보호될 뿐, 그 옆에 새 키를 심는 것은 store 가 막지
/// 않는다. 심은 행은 호스트 자신의 prefix 조회에 그대로 섞인다.
///
/// 이것을 막는 것은 store 가 아니라 IPC 핸들러의 예약 namespace 정책이다
/// (`memory::HOST_KEY_NAMESPACE`, [ADR-0141]). 그 전제를 여기 박아 둔다 — store 가
/// 언젠가 namespace 를 직접 지키게 되면 이 테스트가 먼저 깨진다.
///
/// [ADR-0141]: ../../../docs/adr/0141-host-key-namespace-is-reserved-in-raw-memory-kv.md
#[test]
fn ownership_does_not_reserve_a_key_namespace() {
    let mut s = store();
    let scope = Scope::Global;
    s.put(
        HOST_OWNER,
        &scope,
        "tasty.audit.0001",
        &text("deny"),
        &PutOpts::default(),
    )
    .unwrap();

    // 있는 행은 지켜진다.
    assert!(matches!(
        s.put(
            PLUGIN_A,
            &scope,
            "tasty.audit.0001",
            &text("x"),
            &PutOpts::default()
        ),
        Err(MemoryError::OwnedByOther { .. })
    ));
    assert!(matches!(
        s.delete(PLUGIN_A, &scope, "tasty.audit.0001", None),
        Err(MemoryError::OwnedByOther { .. })
    ));

    // 없는 키는 지켜지지 않는다 — 그리고 호스트의 prefix 조회에 섞인다.
    s.put(
        PLUGIN_A,
        &scope,
        "tasty.audit.9999",
        &text("forged"),
        &PutOpts::default(),
    )
    .unwrap();
    let rows = s
        .list(
            &scope,
            &ListOpts {
                prefix: Some("tasty.audit.".into()),
                ..Default::default()
            },
        )
        .unwrap();
    assert_eq!(rows.len(), 2, "심은 행이 호스트 조회에 섞이지 않는다");
    assert!(
        rows.iter().any(|e| e.owner.as_deref() == Some(PLUGIN_A)),
        "심은 행의 owner 가 plugin 인데도 같은 조회에 함께 나온다"
    );
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

/// `regular_used_bytes` 증분 카운터가 모든 변이 경로 후 실제 전체 스캔과 일치하는지
/// (드리프트 회귀 가드). put(insert/update-grow/update-shrink)/delete/purge_scope 검증.
#[test]
fn regular_used_bytes_stays_consistent() {
    let mut s = store();
    fn assert_consistent(s: &MemoryStore) {
        assert_eq!(
            s.regular_used_bytes,
            MemoryStore::scan_regular_used(&s.conn),
            "regular_used_bytes drifted from actual SUM(LENGTH(value))"
        );
    }
    assert_consistent(&s);

    s.put(
        HOST_OWNER,
        &Scope::Global,
        "a",
        &text("hello"),
        &PutOpts::default(),
    )
    .unwrap();
    assert_consistent(&s);

    // update grow
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "a",
        &text("hello world"),
        &PutOpts::default(),
    )
    .unwrap();
    assert_consistent(&s);

    // update shrink
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "a",
        &text("x"),
        &PutOpts::default(),
    )
    .unwrap();
    assert_consistent(&s);

    s.put(
        HOST_OWNER,
        &Scope::Surface(1),
        "b",
        &text("0123456789"),
        &PutOpts::default(),
    )
    .unwrap();
    assert_consistent(&s);

    s.delete(HOST_OWNER, &Scope::Global, "a", None).unwrap();
    assert_consistent(&s);

    // purge_scope 후에도 정합 (재계산 경로).
    s.purge_scope(&Scope::Surface(1)).unwrap();
    assert_consistent(&s);
    assert_eq!(s.regular_used_bytes, 0, "all entries removed → 0");
}

/// quota 거부 시 카운터가 변하지 않아야 한다 (commit 이전 return).
#[test]
fn regular_used_bytes_unchanged_on_quota_reject() {
    let mut s = MemoryStore::open_in_memory_with_config(MemoryConfig {
        entry_max_bytes: 1024,
        regular_quota_total_bytes: 20,
        ..MemoryConfig::default()
    })
    .unwrap();
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "a",
        &text("0123456789ab"),
        &PutOpts::default(),
    )
    .unwrap();
    let before = s.regular_used_bytes;
    let err = s.put(
        HOST_OWNER,
        &Scope::Global,
        "b",
        &text("0123456789ab"),
        &PutOpts::default(),
    );
    assert!(matches!(err, Err(MemoryError::QuotaExceeded { .. })));
    assert_eq!(
        s.regular_used_bytes, before,
        "counter must not change on rejected put"
    );
    assert_eq!(
        s.regular_used_bytes,
        MemoryStore::scan_regular_used(&s.conn)
    );
}

/// count 기반 retention: 최근 N 개만 남기고 prefix 매칭 로그를 삭제. 비매칭 키는 보존,
/// 카운터 정합 유지, N 이하면 no-op.
#[test]
fn prune_prefix_keep_recent_caps_logs() {
    let mut s = store();
    // 시간순 정렬되는 zero-padded 키 (audit/telemetry 와 동형).
    for i in 0..10 {
        s.put(
            HOST_OWNER,
            &Scope::Global,
            &format!("tasty.audit.{i:04}"),
            &text("x"),
            &PutOpts::default(),
        )
        .unwrap();
    }
    // 비-로그 키는 prune 영향 없어야 함.
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "real.data",
        &text("keep"),
        &PutOpts::default(),
    )
    .unwrap();

    // 최근 3개만 유지 → 7개 삭제.
    let deleted = s.prune_prefix_keep_recent("tasty.audit.", 3).unwrap();
    assert_eq!(deleted, 7);
    assert!(s.get(&Scope::Global, "tasty.audit.0009").unwrap().is_some());
    assert!(s.get(&Scope::Global, "tasty.audit.0007").unwrap().is_some());
    assert!(s.get(&Scope::Global, "tasty.audit.0006").unwrap().is_none());
    assert!(s.get(&Scope::Global, "tasty.audit.0000").unwrap().is_none());
    // 비-로그 키 보존.
    assert!(s.get(&Scope::Global, "real.data").unwrap().is_some());
    // 카운터 정합.
    assert_eq!(
        s.regular_used_bytes,
        MemoryStore::scan_regular_used(&s.conn)
    );

    // 남은 개수(3) 이하 cap → no-op.
    let deleted2 = s.prune_prefix_keep_recent("tasty.audit.", 5).unwrap();
    assert_eq!(deleted2, 0);

    // 사실상 무제한 cap 도 no-op 이어야 한다 — i64 로 좁힐 때 음수가 되면 OFFSET 이
    // 무효가 되어 전량 삭제로 돌변한다.
    assert_eq!(
        s.prune_prefix_keep_recent("tasty.audit.", u64::MAX)
            .unwrap(),
        0
    );
    assert!(s.get(&Scope::Global, "tasty.audit.0009").unwrap().is_some());
}

#[test]
fn prune_prefix_older_than_cuts_by_timestamp_in_key() {
    let mut s = store();
    // audit/telemetry 와 동형인 `{prefix}{ts:013}.{seq:04}` 키.
    for ts in [1_000u64, 5_000, 9_000] {
        s.put(
            HOST_OWNER,
            &Scope::Global,
            &format!("tasty.audit.{ts:013}.0000"),
            &text("x"),
            &PutOpts::default(),
        )
        .unwrap();
    }
    // 다른 prefix 는 같은 ts 여도 건드리지 않는다.
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "tasty.telemetry.event.0000000001000.0000",
        &text("x"),
        &PutOpts::default(),
    )
    .unwrap();

    assert_eq!(s.prune_prefix_older_than("tasty.audit.", 5_000).unwrap(), 1);
    assert!(
        s.get(&Scope::Global, "tasty.audit.0000000005000.0000")
            .unwrap()
            .is_some(),
        "cutoff 와 같은 ts 는 만료가 아니다"
    );
    assert!(
        s.get(&Scope::Global, "tasty.audit.0000000001000.0000")
            .unwrap()
            .is_none()
    );
    assert!(
        s.get(&Scope::Global, "tasty.telemetry.event.0000000001000.0000")
            .unwrap()
            .is_some()
    );
    assert_eq!(
        s.regular_used_bytes,
        MemoryStore::scan_regular_used(&s.conn)
    );
    // cutoff 0 = 아무것도 만료되지 않음.
    assert_eq!(s.prune_prefix_older_than("tasty.audit.", 0).unwrap(), 0);
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

// ---- WAL 되감기 한도 ----
//
// 아래 세 테스트만 파일 기반 DB 를 쓴다(나머지는 인메모리) — WAL 은 파일이 있어야
// 존재하고, 이 항목의 회귀는 "파일 크기" 로만 드러나기 때문이다.

fn disk_store(dir: &std::path::Path) -> (MemoryStore, std::path::PathBuf) {
    let path = dir.join("memory.db");
    let s = MemoryStore::open(&path).unwrap();
    (s, path)
}

fn wal_len(db_path: &std::path::Path) -> u64 {
    let wal = db_path.with_extension("db-wal");
    std::fs::metadata(&wal).map(|m| m.len()).unwrap_or(0)
}

/// 커밋 하나가 WAL 에 남기는 양을 키우려고 큰 값을 넣는다 — 기본 4KiB 페이지라
/// 작은 값으로는 상한(4MiB)에 닿기까지 수천 커밋이 필요하다.
fn append_rows(s: &mut MemoryStore, scope: &Scope, from: usize, count: usize, bytes: usize) {
    let blob = "x".repeat(bytes);
    for i in from..from + count {
        s.put(
            PLUGIN_A,
            scope,
            &format!("k{i}"),
            &text(&blob),
            &PutOpts::default(),
        )
        .unwrap();
    }
}

#[test]
fn journal_size_limit_is_applied_to_disk_databases() {
    let tmp = tempfile::tempdir().unwrap();
    let (s, _path) = disk_store(tmp.path());
    let limit: i64 = s
        .conn
        .query_row("PRAGMA journal_size_limit", [], |r| r.get(0))
        .unwrap();
    assert_eq!(limit, WAL_SIZE_LIMIT_BYTES);

    // 상한이 autocheckpoint 임계(페이지 수 × page_size)와 같은 값이라는 것이
    // 이 값을 고른 근거 자체다 — SQLite 기본값이 바뀌면 근거가 무너지므로 고정한다.
    let pages: i64 = s
        .conn
        .query_row("PRAGMA wal_autocheckpoint", [], |r| r.get(0))
        .unwrap();
    let page_size: i64 = s
        .conn
        .query_row("PRAGMA page_size", [], |r| r.get(0))
        .unwrap();
    assert_eq!(pages * page_size, WAL_SIZE_LIMIT_BYTES);
}

/// 실패·불일치가 **조용하지 않은가**. `tracing` 출력을 그대로 받아 본다.
///
/// 이 헬퍼가 없으면 "경고를 낸다" 가 소스를 읽어야만 보이는 주장으로 남는다.
/// 같은 쓰임의 선례가 `crates/tasty-timer/src/waker_poison_tests.rs` 에 있다.
fn captured_log(body: impl FnOnce()) -> String {
    use std::io::Write;

    #[derive(Clone)]
    struct Capture(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    impl Write for Capture {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0
                .lock()
                .expect("capture lock")
                .extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    let capture = Capture(std::sync::Arc::new(std::sync::Mutex::new(Vec::new())));
    let writer = capture.clone();
    let subscriber = tracing_subscriber::fmt()
        .without_time()
        .with_ansi(false)
        .with_writer(move || writer.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, body);
    let bytes = capture.0.lock().expect("capture lock").clone();
    String::from_utf8(bytes).expect("UTF-8 log")
}

/// 읽기 전용 DB 에서 pragma 가 안 서는 것이 **관측된다**.
///
/// 이 갈래가 옛 코드에서 정확히 조용했다 — 세 pragma 를 `.ok()` 로 버렸으므로
/// journal_mode 가 요청과 다른 채로 아무 흔적도 안 남았다. 읽기 전용 열기는 실제로
/// 일어나는 조건이다(파일 권한 · 읽기 전용 마운트).
#[test]
fn a_read_only_database_says_that_the_requested_pragmas_did_not_take() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("readonly.db");
    // 먼저 평범하게 만든다 — 이 시점의 journal_mode 는 SQLite 기본값(delete)이다.
    rusqlite::Connection::open(&path).unwrap();

    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let log = captured_log(|| {
        crate::pragma::apply_connection_pragmas(&conn, &path);
    });

    assert!(
        log.contains("journal_mode is delete, not the requested WAL"),
        "요청과 다른 journal_mode 가 조용히 지나갔다:\n{log}"
    );
    assert!(log.contains("WARN"), "경고 수준이 아니다:\n{log}");
}

/// 정상 경로는 **조용하다** — 위 시험이 잡는 것이 경고 그 자체임을 못 박는다.
#[test]
fn a_healthy_database_logs_nothing_while_setting_its_pragmas() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("healthy.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    let log = captured_log(|| {
        crate::pragma::apply_connection_pragmas(&conn, &path);
    });
    assert!(log.is_empty(), "정상 열기에서 경고가 났다:\n{log}");
}

/// in-memory DB 의 `memory` 는 실패가 아니라 그 모드의 정상 결과다 — 경고가 없어야
/// 한다. 이것이 없으면 위 경고가 매 테스트·매 부팅마다 울려 무의미해진다.
#[test]
fn an_in_memory_database_does_not_warn_about_its_own_journal_mode() {
    let conn = rusqlite::Connection::open_in_memory().unwrap();
    let log = captured_log(|| {
        crate::pragma::apply_connection_pragmas(&conn, std::path::Path::new(":memory:"));
    });
    assert!(log.is_empty(), "in-memory 정상 결과에 경고가 났다:\n{log}");
}

/// 요청한 `journal_mode` 와 **실제 적용값**은 다른 축이다.
///
/// `pragma_update` 는 두 모드 모두 `Ok(())` 를 내므로 반환값만 보면 둘이 구별되지
/// 않는다. 파일 DB 는 요청대로 `wal` 이 되고, in-memory DB 는 SQLite 가 WAL 을 못
/// 쓰므로 조용히 `memory` 로 남는다. 이 시험이 그 둘을 각각 못 박는다 — 여기가
/// 무너지면 "소스에 WAL 이라고 적혀 있다" 를 runtime 보장으로 쓴 것이 된다.
#[test]
fn the_effective_journal_mode_differs_between_a_file_and_an_in_memory_database() {
    let tmp = tempfile::tempdir().unwrap();
    let (disk, _path) = disk_store(tmp.path());
    assert_eq!(
        crate::pragma::effective_journal_mode(&disk.conn).unwrap(),
        "wal",
        "파일 DB 가 요청한 WAL 로 안 섰다"
    );

    let mem = store();
    assert_eq!(
        crate::pragma::effective_journal_mode(&mem.conn).unwrap(),
        "memory",
        "in-memory DB 의 journal_mode 가 바뀌었다 — 이 모드의 허용 결과를 다시 정해야 한다"
    );
}

/// 이 항목의 본체 — 한 번 부푼 WAL 이 **다시 줄어드는가**.
///
/// "꾸준한 append 로는 안 자란다" 를 단정하면 공허한 테스트가 된다(실측: pragma 를
/// 빼도 통과한다). autocheckpoint 가 WAL 내부를 순환 재사용하므로 평상시 파일 크기는
/// pragma 와 무관하게 임계 근처에 머물기 때문이다. 실제로 갈리는 지점은 큰 트랜잭션
/// 이나 VACUUM 으로 한 번 부푼 **뒤**다 — pragma 가 없으면 그 크기가 프로세스 수명
/// 내내 고착되고(169MB 사례), 있으면 다음 되감기에서 상한으로 회수된다.
#[test]
fn a_ballooned_wal_shrinks_back_under_the_limit() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, path) = disk_store(tmp.path());
    let scope = Scope::Surface(1);

    append_rows(&mut s, &scope, 0, 400, 64 * 1024);
    for i in 0..350 {
        s.delete(PLUGIN_A, &scope, &format!("k{i}"), None).unwrap();
    }
    // VACUUM 은 DB 를 통째로 다시 쓰므로 WAL 을 한도보다 훨씬 크게 부풀린다 —
    // 이 항목이 재현하려는 "한 번 커진 WAL" 의 실제 발생 경로 중 하나다.
    assert!(s.vacuum_if_fragmented(1).unwrap(), "VACUUM 이 돌지 않았다");
    let ballooned = wal_len(&path);
    assert!(
        ballooned > WAL_SIZE_LIMIT_BYTES as u64,
        "WAL 이 한도를 넘겨 부풀지 않아 회수를 검증할 수 없다: {ballooned}"
    );

    // 이후의 평범한 커밋 몇 개면 되감기가 일어나고, 그때 상한으로 잘린다.
    append_rows(&mut s, &scope, 1000, 5, 1024);
    let after = wal_len(&path);
    assert!(
        after <= WAL_SIZE_LIMIT_BYTES as u64,
        "부푼 WAL 이 회수되지 않고 고착됐다: {ballooned} -> {after}, limit={WAL_SIZE_LIMIT_BYTES}"
    );
}

/// 이미 커진 WAL 은 pragma 만으로는 즉시 줄지 않는다 — 부팅 시 되감기를 강제하는
/// 경로가 그것을 회수하고, 그 과정에서 레코드를 잃지 않는다.
#[test]
fn checkpoint_truncate_reclaims_the_wal_without_losing_rows() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, path) = disk_store(tmp.path());
    let scope = Scope::Surface(1);
    append_rows(&mut s, &scope, 0, 300, 64 * 1024);

    let before: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(before, 300);
    assert!(wal_len(&path) > 0, "회수할 WAL 이 애초에 없다");

    assert!(s.checkpoint_truncate().unwrap(), "체크포인트가 busy 였다");

    assert_eq!(wal_len(&path), 0, "truncate 후에도 WAL 이 남아 있다");
    let after: i64 = s
        .conn
        .query_row("SELECT COUNT(*) FROM memory", [], |r| r.get(0))
        .unwrap();
    assert_eq!(after, before, "체크포인트가 레코드를 훼손했다");
    // 회수 후에도 정상적으로 읽힌다(본체로 흡수됐는지 값 수준에서 확인).
    assert!(s.get(&scope, "k0").unwrap().is_some());
    assert!(s.get(&scope, "k299").unwrap().is_some());
}

// ---- DB 지연 계측 ----

/// commit 누계가 **성공한 쓰기 수**를 센다. 거부된 쓰기는 트랜잭션을 열고도
/// commit 없이 돌아가므로 세면 안 된다 — 그러면 평균이 "쓰기 한 건이 걸리는
/// 시간" 을 더는 뜻하지 않는다.
#[test]
fn only_a_write_that_committed_is_counted_as_a_commit() {
    let mut s = store();
    let gauge = s.db_latency();
    let scope = Scope::Surface(1);
    assert_eq!(gauge.snapshot().commits, 0, "열기만 해서는 commit 이 없다");

    s.put(PLUGIN_A, &scope, "a", &text("hello"), &PutOpts::default())
        .unwrap();
    assert_eq!(gauge.snapshot().commits, 1);

    // 없는 키에 cas 를 걸면 트랜잭션은 열리지만 commit 전에 돌아간다.
    let rejected = s.put(
        PLUGIN_A,
        &scope,
        "b",
        &text("nope"),
        &PutOpts {
            cas: Some(7),
            ..Default::default()
        },
    );
    assert!(rejected.is_err(), "cas 충돌이 거부되지 않았다");
    assert_eq!(
        gauge.snapshot().commits,
        1,
        "거부된 쓰기가 commit 으로 셌다"
    );

    s.delete(PLUGIN_A, &scope, "a", None).unwrap();
    assert_eq!(gauge.snapshot().commits, 2);
}

/// checkpoint 는 commit 과 **다른 모수**다. 한 값에 섞이면 운영자가 "쓰기가
/// 느리다" 와 "WAL 되감기가 느리다" 를 못 가른다.
#[test]
fn a_checkpoint_lands_in_its_own_population() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _path) = disk_store(tmp.path());
    let gauge = s.db_latency();
    let scope = Scope::Surface(1);
    append_rows(&mut s, &scope, 0, 4, 1024);

    let before = gauge.snapshot();
    assert_eq!(before.checkpoints, 0);
    assert!(before.commits >= 4, "쓰기가 commit 으로 안 셌다");

    assert!(s.checkpoint_truncate().unwrap(), "체크포인트가 busy 였다");

    let after = gauge.snapshot();
    assert_eq!(after.checkpoints, 1);
    assert_eq!(after.checkpoints_busy, 0, "끝까지 갔는데 busy 로 셌다");
    assert_eq!(
        after.commits, before.commits,
        "checkpoint 가 commit 으로 셌다"
    );
}

// ---- 적용값과 저장 실패 (RF20) ----

/// 열린 스토어가 **되읽은 실제값**을 들고 있고, 두 모드가 각자의 허용 결과로 선다.
///
/// 파일 DB 와 in-memory DB 의 `journal_mode` 실제값이 달라도 둘 다 `degraded` 가
/// 아니어야 한다 — 허용 결과표가 모드마다 한 열이기 때문이다. 표에서 열을 바꿔
/// 읽으면(파일 DB 에 `memory` 를 허용) 이 시험이 아니라 아래 읽기 전용 시험이 죽고,
/// 모드 판정을 뒤집으면 여기가 죽는다.
#[test]
fn an_open_store_carries_the_pragmas_that_took_in_each_mode() {
    let tmp = tempfile::tempdir().unwrap();
    let (disk, _path) = disk_store(tmp.path());
    let d = disk.applied_pragmas();
    assert!(!d.in_memory, "파일 DB 를 in-memory 로 판정했다");
    assert!(!d.degraded(), "정상 파일 DB 가 degraded 다: {d:?}");
    let j = d.get("journal_mode").unwrap();
    assert_eq!(j.requested, "WAL");
    assert_eq!(j.effective.as_deref(), Some("wal"));
    assert_eq!(
        d.get("synchronous").unwrap().effective.as_deref(),
        Some("NORMAL")
    );
    assert_eq!(
        d.get("foreign_keys").unwrap().effective.as_deref(),
        Some("ON")
    );
    assert_eq!(
        d.get("journal_size_limit").unwrap().effective.as_deref(),
        Some(WAL_SIZE_LIMIT_BYTES.to_string().as_str())
    );

    // 스토어가 들고 있는 값이 **지금 연결의 실제값**과 같은지 직접 대조한다.
    assert_eq!(
        j.effective.as_deref().unwrap(),
        crate::pragma::effective_journal_mode(&disk.conn).unwrap()
    );

    let mem = store();
    let m = mem.applied_pragmas();
    assert!(m.in_memory, "in-memory DB 를 파일로 판정했다");
    assert!(!m.degraded(), "in-memory 정상 결과가 degraded 다: {m:?}");
    assert_eq!(
        m.get("journal_mode").unwrap().effective.as_deref(),
        Some("memory")
    );
}

/// 요청이 안 선 DB 는 **값으로** degraded 라고 말한다 — 경고 로그만이 아니다.
#[test]
fn a_read_only_database_reports_itself_degraded() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("readonly.db");
    rusqlite::Connection::open(&path).unwrap();
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();

    let applied = crate::pragma::apply_connection_pragmas(&conn, &path);

    assert!(applied.degraded(), "안 선 요청이 degraded 로 안 나왔다");
    let j = applied.get("journal_mode").unwrap();
    assert!(!j.took);
    assert_eq!(j.effective.as_deref(), Some("delete"));
}

/// 파일 DB 에서 `memory` 는 허용 결과가 **아니다** — in-memory 의 정상값을 파일 DB
/// 에 빌려주면 WAL 이 조용히 안 선 것을 삼킨다.
#[test]
fn a_file_database_in_memory_journal_mode_is_degraded() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("m.db");
    let conn = rusqlite::Connection::open(&path).unwrap();
    // 파일 DB 에 직접 memory 모드를 건다 — 그 뒤 WAL 요청은 거절되지 않고 받아들여지므로,
    // 판정 함수만 떼어 대조한다.
    let mode: String = conn
        .query_row("PRAGMA journal_mode=MEMORY", [], |r| r.get(0))
        .unwrap();
    assert_eq!(mode, "memory");
    let applied = crate::pragma::apply_connection_pragmas(&conn, &path);
    assert!(!applied.in_memory);
    // WAL 로 바뀌었으면 정상이고, memory 로 남았으면 degraded 여야 한다 — 어느 쪽이든
    // "memory 인데 정상" 인 조합은 없어야 한다.
    let j = applied.get("journal_mode").unwrap();
    assert!(
        !(j.effective.as_deref() == Some("memory") && j.took),
        "파일 DB 의 memory 를 정상으로 봤다: {j:?}"
    );
}

/// 잠긴 DB 에 쓰면 **실패로** 돌아오고 원인이 `busy` 로 갈리며, 스토어의 메모리 쪽
/// 상태(quota 카운터 · 변경 버퍼)는 실패 전 그대로다.
///
/// 이것이 "실패한 commit 을 정상 저장으로 표현하지 않는다" 의 저장 축이다. 카운터가
/// 먼저 움직이면 다음 quota 판정이 디스크에 없는 바이트를 세고, 변경 버퍼가 먼저
/// 움직이면 `memory.changed` 가 없는 쓰기를 알린다.
#[test]
fn a_write_to_a_locked_database_fails_as_busy_and_leaves_the_cache_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, path) = disk_store(tmp.path());
    let scope = Scope::Surface(1);
    s.put(PLUGIN_A, &scope, "before", &text("x"), &PutOpts::default())
        .unwrap();
    s.take_pending_changes();
    let used_before = s.regular_used_bytes;
    // 기본 busy_timeout(5 s)을 기다리지 않게 한다 — 판정하려는 것은 분류지 대기가 아니다.
    s.conn.busy_timeout(std::time::Duration::ZERO).unwrap();

    let holder = rusqlite::Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();

    let err = s
        .put(PLUGIN_A, &scope, "k", &text("value"), &PutOpts::default())
        .expect_err("잠긴 DB 에 쓰기가 성공으로 돌아왔다");
    assert_eq!(err.storage_failure(), Some(StorageFailure::Busy), "{err}");
    assert_eq!(
        s.regular_used_bytes, used_before,
        "실패한 쓰기가 카운터를 옮겼다"
    );
    assert!(
        s.take_pending_changes().is_empty(),
        "실패한 쓰기가 변경 알림을 남겼다"
    );

    holder.execute_batch("ROLLBACK").unwrap();
    assert!(
        s.get(&scope, "k").unwrap().is_none(),
        "실패한 쓰기가 디스크에 남았다"
    );
}

/// 용량이 찬 DB 에 쓰면 `disk_full` 로 갈린다. 볼륨을 채우는 대신 `max_page_count`
/// 로 DB 의 페이지 상한을 낮춘다 — SQLite 는 두 경우에 같은 `SQLITE_FULL` 을 낸다.
#[test]
fn a_write_past_the_page_ceiling_fails_as_disk_full_and_leaves_the_cache_untouched() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _path) = disk_store(tmp.path());
    let scope = Scope::Surface(1);
    let pages: i64 = s
        .conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    s.conn
        .query_row(&format!("PRAGMA max_page_count={pages}"), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    let used_before = s.regular_used_bytes;

    let big = "x".repeat(64 * 1024);
    let err = s
        .put(PLUGIN_A, &scope, "big", &text(&big), &PutOpts::default())
        .expect_err("상한을 넘는 쓰기가 성공으로 돌아왔다");
    assert_eq!(
        err.storage_failure(),
        Some(StorageFailure::DiskFull),
        "{err}"
    );
    assert_eq!(s.regular_used_bytes, used_before);
    assert!(s.take_pending_changes().is_empty());
}

/// 요청 거부는 저장 실패가 **아니다** — 저장소가 멀쩡히 답한 것이다.
#[test]
fn a_refused_request_is_not_a_storage_failure() {
    let mut s = store();
    let err = s
        .delete(PLUGIN_A, &Scope::Surface(1), "missing", None)
        .unwrap_err();
    assert_eq!(err.storage_failure(), None);
}

/// 초기화 오류가 저장 경로와 **같은 표**를 쓴다 — 깨진 파일은 `Corrupt` 로 간다.
#[test]
fn opening_a_file_that_is_not_a_database_is_classified_as_corrupt() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("memory.db");
    std::fs::write(&path, vec![0xAB; 8192]).unwrap();
    let err = match MemoryStore::open(&path) {
        Ok(_) => panic!("SQLite 가 아닌 파일이 열렸다"),
        Err(e) => e,
    };
    assert!(
        matches!(err, MemoryInitError::Corrupt(_)),
        "깨진 파일이 Corrupt 로 안 갔다: {err}"
    );
}
