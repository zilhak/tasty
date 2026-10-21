//! 정리되지 않은 만료 행 위의 쓰기 판정. 사례마다 새 저장소에서 owner `a`의 만료 행 하나로 시작하고
//! `purge_expired`는 부르지 않는다. 각 사례를 단독 호출과 쓰기 묶음 두 경로로 실행한다.

use super::*;
use crate::{BatchMode, WriteOp, WriteOutput};

const A: &str = "com.tasty.a";
const B: &str = "com.tasty.b";

#[derive(Clone, Copy, Debug)]
enum Route {
    Single,
    Batch,
}

const ROUTES: [Route; 2] = [Route::Single, Route::Batch];

fn past() -> PutOpts {
    PutOpts {
        expires_at: Some(unix_ms_now() - 1_000),
        cas: None,
    }
}

fn opts(cas: Option<u64>) -> PutOpts {
    PutOpts {
        expires_at: None,
        cas,
    }
}

fn batch_version(results: crate::BatchResult) -> Result<u64> {
    let mut results = results.expect("묶음 자체는 실패하지 않는다");
    assert_eq!(results.len(), 1);
    results.remove(0).map(|out| match out {
        WriteOutput::Version(v) => v,
        WriteOutput::Deleted => panic!("put이 삭제 결과를 냈다"),
    })
}

fn put_via(
    route: Route,
    s: &mut MemoryStore,
    owner: &str,
    key: &str,
    value: &str,
    opts: PutOpts,
) -> Result<u64> {
    match route {
        Route::Single => s.put(owner, &Scope::Global, key, &text(value), &opts),
        Route::Batch => batch_version(s.write_batch(
            BatchMode::Independent,
            &[WriteOp::Put {
                owner: owner.into(),
                scope: Scope::Global,
                key: key.into(),
                value: text(value),
                opts,
            }],
        )),
    }
}

fn put_secret_via(
    route: Route,
    s: &mut MemoryStore,
    owner: &str,
    key: &str,
    value: &str,
    opts: PutOpts,
) -> Result<u64> {
    match route {
        Route::Single => s.put_secret(owner, &Scope::Global, key, &text(value), &opts),
        Route::Batch => batch_version(s.write_batch(
            BatchMode::Independent,
            &[WriteOp::PutSecret {
                owner: owner.into(),
                scope: Scope::Global,
                key: key.into(),
                value: text(value),
                opts,
            }],
        )),
    }
}

fn kinds(changes: &[MemoryChange]) -> Vec<(MemoryChangeKind, Option<u64>)> {
    changes.iter().map(|c| (c.kind, c.version)).collect()
}

fn with_expired(config: MemoryConfig, value: &str) -> MemoryStore {
    let mut s = MemoryStore::open_in_memory_with_config(config).unwrap();
    s.put(A, &Scope::Global, "k", &text(value), &past())
        .unwrap();
    assert!(s.get(&Scope::Global, "k").unwrap().is_none());
    s.take_pending_changes();
    s
}

fn assert_counter_matches_disk(s: &MemoryStore) {
    assert_eq!(
        s.regular_used_bytes,
        MemoryStore::scan_regular_used(&s.conn),
        "quota 카운터가 디스크와 다르다"
    );
}

#[test]
fn an_expired_row_does_not_block_another_owner() {
    for route in ROUTES {
        let mut s = with_expired(MemoryConfig::default(), "x");
        let v = put_via(route, &mut s, B, "k", "y", opts(None));
        assert_eq!(v.unwrap(), 1, "{route:?}");
        let entry = s.get(&Scope::Global, "k").unwrap().unwrap();
        assert_eq!(
            (entry.owner.as_deref(), entry.version),
            (Some(B), 1),
            "{route:?}"
        );
        assert_eq!(
            kinds(&s.take_pending_changes()),
            vec![
                (MemoryChangeKind::Expired, None),
                (MemoryChangeKind::Created, Some(1))
            ],
            "{route:?}"
        );
        // 이제 b의 유효 행이므로 a는 기존 owner 보호 규칙대로 거절된다.
        let err = put_via(route, &mut s, A, "k", "z", opts(None)).unwrap_err();
        assert!(
            matches!(err, MemoryError::OwnedByOther { ref owner } if owner == B),
            "{route:?}: {err:?}"
        );
        assert_counter_matches_disk(&s);
    }
}

#[test]
fn the_same_owner_recreates_an_expired_key_from_version_one() {
    for route in ROUTES {
        let mut s = with_expired(MemoryConfig::default(), "x");
        let v = put_via(route, &mut s, A, "k", "y", opts(None));
        assert_eq!(v.unwrap(), 1, "{route:?}");
        assert_eq!(
            kinds(&s.take_pending_changes()),
            vec![
                (MemoryChangeKind::Expired, None),
                (MemoryChangeKind::Created, Some(1))
            ],
            "{route:?}"
        );
        assert_eq!(
            put_via(route, &mut s, A, "k", "z", opts(Some(1))).unwrap(),
            2,
            "{route:?}: 새 행의 version으로 CAS가 이어진다"
        );
        assert_counter_matches_disk(&s);
    }
}

#[test]
fn a_cas_on_an_expired_key_sees_no_row() {
    for route in ROUTES {
        let mut s = with_expired(MemoryConfig::default(), "x");
        let err = put_via(route, &mut s, A, "k", "y", opts(Some(1))).unwrap_err();
        assert!(
            matches!(
                err,
                MemoryError::CasConflict {
                    expected: 1,
                    actual: 0
                }
            ),
            "{route:?}: {err:?}"
        );
        assert!(s.take_pending_changes().is_empty(), "{route:?}");
        assert_counter_matches_disk(&s);
    }
}

#[test]
fn a_live_row_still_rejects_another_owner() {
    for route in ROUTES {
        let mut s = MemoryStore::open_in_memory().unwrap();
        s.put(A, &Scope::Global, "k", &text("live"), &PutOpts::default())
            .unwrap();
        let err = put_via(route, &mut s, B, "k", "y", opts(None)).unwrap_err();
        assert!(matches!(err, MemoryError::OwnedByOther { .. }), "{route:?}");
    }
}

fn small_regular_quota() -> MemoryConfig {
    MemoryConfig {
        entry_max_bytes: 1024,
        regular_quota_total_bytes: 20,
        ..MemoryConfig::default()
    }
}

#[test]
fn the_regular_quota_leaves_out_unpurged_expired_rows() {
    for route in ROUTES {
        // 만료 16 byte가 남은 채 다른 키에 12 byte: 디스크 합은 28이지만 판정은 12다.
        let mut s = with_expired(small_regular_quota(), &"e".repeat(16));
        let v = put_via(route, &mut s, B, "other", &"n".repeat(12), opts(None));
        assert_eq!(v.unwrap(), 1, "{route:?}");
        assert_counter_matches_disk(&s);
        // 유효 바이트가 한도를 넘으면 지금과 같이 거절하고 유효 바이트를 알린다.
        let err = put_via(route, &mut s, B, "more", &"m".repeat(9), opts(None)).unwrap_err();
        match err {
            MemoryError::QuotaExceeded { area, used, limit } => {
                assert_eq!(area, MemoryArea::Regular, "{route:?}");
                assert_eq!((used, limit), (21, 20), "{route:?}");
            }
            other => panic!("{route:?}: {other:?}"),
        }
        assert_counter_matches_disk(&s);
    }
}

#[test]
fn replacing_an_expired_row_counts_only_the_new_value() {
    for route in ROUTES {
        let mut s = with_expired(small_regular_quota(), &"e".repeat(16));
        let v = put_via(route, &mut s, B, "k", &"n".repeat(20), opts(None));
        assert_eq!(v.unwrap(), 1, "{route:?}");
        assert_eq!(s.regular_used_bytes, 20, "{route:?}");
        assert_counter_matches_disk(&s);
    }
}

fn small_secret_quota() -> MemoryConfig {
    MemoryConfig {
        entry_max_bytes: 1024,
        secret_quota_per_owner_bytes: 20,
        ..MemoryConfig::default()
    }
}

#[test]
fn the_secret_quota_and_version_leave_out_unpurged_expired_rows() {
    for route in ROUTES {
        let mut s = MemoryStore::open_in_memory_with_config(small_secret_quota()).unwrap();
        s.put_secret(A, &Scope::Global, "k", &text(&"e".repeat(16)), &past())
            .unwrap();
        assert!(s.get_secret(A, &Scope::Global, "k").unwrap().is_none());

        // 다른 키: owner 합계에서 만료 16 byte를 뺀다.
        let v = put_secret_via(route, &mut s, A, "other", &"n".repeat(12), opts(None));
        assert_eq!(v.unwrap(), 1, "{route:?}");
        // 같은 키: 신규 version 1, CAS는 행이 없는 것으로 본다.
        let err = put_secret_via(route, &mut s, A, "k", "y", opts(Some(1))).unwrap_err();
        assert!(
            matches!(err, MemoryError::CasConflict { actual: 0, .. }),
            "{route:?}: {err:?}"
        );
        assert_eq!(
            put_secret_via(route, &mut s, A, "k", &"y".repeat(8), opts(None)).unwrap(),
            1,
            "{route:?}"
        );
        assert_eq!(
            s.get_secret(A, &Scope::Global, "k")
                .unwrap()
                .unwrap()
                .version,
            1
        );
        // 유효 합계 20을 넘는 쓰기는 거절한다.
        let err = put_secret_via(route, &mut s, A, "more", "z", opts(None)).unwrap_err();
        assert!(
            matches!(
                err,
                MemoryError::QuotaExceeded {
                    area: MemoryArea::Secret,
                    used: 21,
                    limit: 20
                }
            ),
            "{route:?}: {err:?}"
        );
        assert!(
            s.take_pending_changes().is_empty(),
            "secret 은 이벤트가 없다"
        );
    }
}

#[test]
fn a_rejected_replacement_keeps_the_expired_row() {
    for route in ROUTES {
        let mut s = with_expired(small_regular_quota(), &"e".repeat(16));
        let err = put_via(route, &mut s, B, "k", &"n".repeat(21), opts(None)).unwrap_err();
        assert!(
            matches!(err, MemoryError::QuotaExceeded { .. }),
            "{route:?}"
        );
        let left: i64 = s
            .conn
            .query_row("SELECT COUNT(*) FROM memory WHERE key='k'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(left, 1, "{route:?}: 거절된 쓰기가 만료 행을 지웠다");
        assert!(s.take_pending_changes().is_empty(), "{route:?}");
        assert_counter_matches_disk(&s);
    }
}
