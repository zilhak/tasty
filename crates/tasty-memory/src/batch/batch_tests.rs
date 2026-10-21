//! 실행 단위·쓰기 묶음 시험. 실제 파일 DB의 저장 실패와 시험용 저장소 대조를 포함한다.

use std::path::{Path, PathBuf};

use super::*;
use crate::testing::InMemoryStorage;
use crate::{
    HOST_OWNER, ListOpts, MemoryChangeKind, MemoryConfig, MemoryEntry, StorageFailure, blackboard,
    unix_ms_now,
};

const PLUGIN_A: &str = "com.tasty.a";
const PLUGIN_B: &str = "com.tasty.b";

fn text(s: &str) -> MemoryValue {
    MemoryValue::Text(s.into())
}

fn disk_store(dir: &Path) -> (MemoryStore, PathBuf) {
    let path = dir.join("memory.db");
    (MemoryStore::open(&path).unwrap(), path)
}

fn put(owner: &str, key: &str, value: &str, cas: Option<u64>) -> WriteOp {
    WriteOp::Put {
        owner: owner.into(),
        scope: Scope::Global,
        key: key.into(),
        value: text(value),
        opts: PutOpts {
            expires_at: None,
            cas,
        },
    }
}

fn delete(owner: &str, key: &str, cas: Option<u64>) -> WriteOp {
    WriteOp::Delete {
        owner: owner.into(),
        scope: Scope::Global,
        key: key.into(),
        cas,
    }
}

fn put_secret(owner: &str, key: &str, value: &str) -> WriteOp {
    WriteOp::PutSecret {
        owner: owner.into(),
        scope: Scope::Global,
        key: key.into(),
        value: text(value),
        opts: PutOpts::default(),
    }
}

/// 오류의 종류만 남긴다. 대조 시험이 메시지 차이로 갈리지 않게 한다.
fn kind(e: &MemoryError) -> &'static str {
    match e {
        MemoryError::NotFound { .. } => "not_found",
        MemoryError::AlreadyExists { .. } => "already_exists",
        MemoryError::CasConflict { .. } => "cas",
        MemoryError::OwnedByOther { .. } => "owned",
        MemoryError::QuotaExceeded { .. } => "quota",
        MemoryError::InvalidKey(_) => "invalid_key",
        MemoryError::InvalidScope(_) => "invalid_scope",
        MemoryError::InvalidOwner(_) => "invalid_owner",
        MemoryError::InvalidContentType(_) => "invalid_content_type",
        MemoryError::ValueTooLarge { .. } => "too_large",
        MemoryError::Db(_) => "db",
    }
}

fn shape(results: &[Result<WriteOutput>]) -> Vec<std::result::Result<WriteOutput, &'static str>> {
    results
        .iter()
        .map(|r| r.as_ref().map(|o| *o).map_err(kind))
        .collect()
}

fn kinds(changes: &[MemoryChange]) -> Vec<(String, MemoryChangeKind, Option<u64>)> {
    changes
        .iter()
        .map(|c| (c.key.clone(), c.kind, c.version))
        .collect()
}

fn assert_counter_matches_disk(s: &MemoryStore) {
    assert_eq!(
        s.regular_used_bytes,
        MemoryStore::scan_regular_used(&s.conn),
        "quota 카운터가 디스크와 다르다"
    );
}

// ---- 확인 절차 1: 정상 100건 ----

#[test]
fn a_hundred_puts_in_one_batch_commit_once_and_survive_a_reopen() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, path) = disk_store(tmp.path());
    let gauge = s.db_latency();
    let ops: Vec<WriteOp> = (0..100)
        .map(|i| put(HOST_OWNER, &format!("k{i:03}"), "v", None))
        .collect();

    let results = s.write_batch(BatchMode::Independent, &ops).unwrap();

    assert_eq!(results.len(), 100);
    assert!(
        results
            .iter()
            .all(|r| matches!(r, Ok(WriteOutput::Version(1))))
    );
    assert_eq!(gauge.snapshot().commits, 1, "묶음이 commit 하나가 아니다");
    assert_eq!(s.take_pending_changes().len(), 100);
    assert_counter_matches_disk(&s);
    drop(s);

    let reopened = MemoryStore::open(&path).unwrap();
    assert_eq!(reopened.count(&Scope::Global, None).unwrap(), 100);
}

// ---- 확인 절차 2: 원자 묶음·중간 충돌·owner·TTL·순서 ----

#[test]
fn an_atomic_batch_with_one_rejection_leaves_nothing_behind() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let gauge = s.db_latency();
    s.put(
        PLUGIN_A,
        &Scope::Global,
        "held",
        &text("x"),
        &PutOpts::default(),
    )
    .unwrap();
    s.take_pending_changes();
    let used = s.regular_used_bytes;
    let commits = gauge.snapshot().commits;

    let err = s
        .write_batch(
            BatchMode::Atomic,
            &[
                put(PLUGIN_A, "a", "1", None),
                put(PLUGIN_B, "held", "steal", None),
                put(PLUGIN_A, "b", "2", None),
            ],
        )
        .unwrap_err();

    assert_eq!(err.index, Some(1));
    assert_eq!(kind(&err.error), "owned");
    assert!(s.get(&Scope::Global, "a").unwrap().is_none());
    assert!(s.get(&Scope::Global, "b").unwrap().is_none());
    assert_eq!(s.regular_used_bytes, used);
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(
        gauge.snapshot().commits,
        commits,
        "되돌린 묶음을 commit 으로 셌다"
    );
}

#[test]
fn an_independent_batch_judges_each_request_after_the_ones_before_it() {
    let config = MemoryConfig {
        regular_quota_total_bytes: 1000,
        ..Default::default()
    };
    let mut s = MemoryStore::open_in_memory_with_config(config).unwrap();
    let gauge = s.db_latency();
    let big = "x".repeat(600);

    let results = s
        .write_batch(
            BatchMode::Independent,
            &[
                put(PLUGIN_A, "k", "v1", None),
                put(PLUGIN_A, "k", "v2", Some(1)),
                put(PLUGIN_A, "k", "stale", Some(1)),
                put(PLUGIN_B, "k", "other owner", None),
                put(PLUGIN_A, "big", &big, None),
                put(PLUGIN_A, "big2", &big, None),
                delete(PLUGIN_A, "big", None),
                put(PLUGIN_A, "big2", &big, None),
                put(PLUGIN_A, "k", "v3", Some(2)),
            ],
        )
        .unwrap();

    assert_eq!(
        shape(&results),
        vec![
            Ok(WriteOutput::Version(1)),
            Ok(WriteOutput::Version(2)),
            Err("cas"),
            Err("owned"),
            Ok(WriteOutput::Version(1)),
            Err("quota"),
            Ok(WriteOutput::Deleted),
            Ok(WriteOutput::Version(1)),
            Ok(WriteOutput::Version(3)),
        ]
    );
    let MemoryError::CasConflict { expected, actual } = results[2].as_ref().unwrap_err() else {
        unreachable!()
    };
    assert_eq!((*expected, *actual), (1, 2), "앞 요청의 version 을 못 봤다");
    assert_eq!(gauge.snapshot().commits, 1);
    let entry = s.get(&Scope::Global, "k").unwrap().unwrap();
    assert_eq!((entry.version, entry.value), (3, text("v3")));
    assert_eq!(
        kinds(&s.take_pending_changes()),
        vec![
            ("k".into(), MemoryChangeKind::Created, Some(1)),
            ("k".into(), MemoryChangeKind::Updated, Some(2)),
            ("big".into(), MemoryChangeKind::Created, Some(1)),
            ("big".into(), MemoryChangeKind::Deleted, None),
            ("big2".into(), MemoryChangeKind::Created, Some(1)),
            ("k".into(), MemoryChangeKind::Updated, Some(3)),
        ],
        "거절된 요청이 변경을 남겼거나 순서가 바뀌었다"
    );
    assert_counter_matches_disk(&s);
}

#[test]
fn a_key_deleted_and_recreated_in_one_batch_starts_over_at_version_one() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let results = s
        .write_batch(
            BatchMode::Atomic,
            &[
                put(PLUGIN_A, "x", "1", None),
                put(PLUGIN_A, "x", "2", None),
                delete(PLUGIN_A, "x", Some(2)),
                put(PLUGIN_B, "x", "new owner", None),
            ],
        )
        .unwrap();
    assert_eq!(
        shape(&results),
        vec![
            Ok(WriteOutput::Version(1)),
            Ok(WriteOutput::Version(2)),
            Ok(WriteOutput::Deleted),
            Ok(WriteOutput::Version(1)),
        ]
    );
    let entry = s.get(&Scope::Global, "x").unwrap().unwrap();
    assert_eq!(entry.owner.as_deref(), Some(PLUGIN_B));
}

#[test]
fn secret_requests_in_a_batch_stay_per_owner_and_count_toward_each_owner_quota() {
    let config = MemoryConfig {
        secret_quota_per_owner_bytes: 100,
        ..Default::default()
    };
    let mut s = MemoryStore::open_in_memory_with_config(config).unwrap();
    let sixty = "s".repeat(60);
    let results = s
        .write_batch(
            BatchMode::Independent,
            &[
                put_secret(PLUGIN_A, "k", &sixty),
                put_secret(PLUGIN_B, "k", &sixty),
                put_secret(PLUGIN_A, "k2", &sixty),
                WriteOp::DeleteSecret {
                    owner: PLUGIN_B.into(),
                    scope: Scope::Global,
                    key: "k".into(),
                    cas: Some(2),
                },
                put_secret(PLUGIN_A, "k", "short"),
            ],
        )
        .unwrap();
    assert_eq!(
        shape(&results),
        vec![
            Ok(WriteOutput::Version(1)),
            Ok(WriteOutput::Version(1)),
            Err("quota"),
            Err("cas"),
            Ok(WriteOutput::Version(2)),
        ]
    );
    let a = s
        .get_secret(PLUGIN_A, &Scope::Global, "k")
        .unwrap()
        .unwrap();
    assert_eq!(a.value, text("short"));
    let b = s
        .get_secret(PLUGIN_B, &Scope::Global, "k")
        .unwrap()
        .unwrap();
    assert_eq!(b.version, 1);
    assert!(s.get(&Scope::Global, "k").unwrap().is_none());
    assert!(
        s.take_pending_changes().is_empty(),
        "secret 이 변경 이벤트를 남겼다"
    );
}

/// 정리되지 않은 만료 행은 단독 put과 같이 묶음 안에서도 없는 것으로 판정한다. 뒤 요청은 앞 요청이
/// 새로 만든 행을 본다.
#[test]
fn an_unpurged_expired_row_is_judged_as_missing_inside_a_batch() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let past = unix_ms_now() - 1_000;
    s.put(
        PLUGIN_A,
        &Scope::Global,
        "ttl",
        &text("old"),
        &PutOpts {
            expires_at: Some(past),
            cas: None,
        },
    )
    .unwrap();
    assert!(s.get(&Scope::Global, "ttl").unwrap().is_none());
    s.take_pending_changes();

    let results = s
        .write_batch(
            BatchMode::Independent,
            &[
                put(PLUGIN_B, "ttl", "steal", None),
                put(PLUGIN_A, "ttl", "renewed", Some(1)),
            ],
        )
        .unwrap();
    assert_eq!(
        shape(&results),
        vec![Ok(WriteOutput::Version(1)), Err("owned")]
    );
    assert_eq!(
        kinds(&s.take_pending_changes()),
        vec![
            ("ttl".into(), MemoryChangeKind::Expired, None),
            ("ttl".into(), MemoryChangeKind::Created, Some(1)),
        ]
    );
    s.delete(PLUGIN_B, &Scope::Global, "ttl", None).unwrap();

    // 같은 단위에서 정리한 뒤에는 새 키처럼 판정한다.
    s.put(
        PLUGIN_A,
        &Scope::Global,
        "ttl",
        &text("expire again"),
        &PutOpts {
            expires_at: Some(past),
            cas: None,
        },
    )
    .unwrap();
    s.take_pending_changes();
    let version = s
        .run_unit(|s| {
            let purged = s.purge_expired()?;
            assert_eq!(purged.regular, 1);
            s.put(
                PLUGIN_B,
                &Scope::Global,
                "ttl",
                &text("b"),
                &PutOpts::default(),
            )
        })
        .unwrap();
    assert_eq!(version, 1);
    assert_eq!(
        kinds(&s.take_pending_changes()),
        vec![
            ("ttl".into(), MemoryChangeKind::Expired, None),
            ("ttl".into(), MemoryChangeKind::Created, Some(1)),
        ]
    );
    assert_counter_matches_disk(&s);
}

// ---- 실행 단위 ----

#[test]
fn a_unit_commits_once_and_its_reads_see_its_own_writes() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let gauge = s.db_latency();
    s.run_unit(|s| {
        for i in 0..10 {
            s.put(
                HOST_OWNER,
                &Scope::Global,
                &format!("k{i}"),
                &text("v"),
                &PutOpts::default(),
            )?;
        }
        assert_eq!(s.count(&Scope::Global, None)?, 10);
        assert!(
            s.take_pending_changes().is_empty(),
            "commit 전에 변경을 공개했다"
        );
        Ok(())
    })
    .unwrap();
    assert_eq!(gauge.snapshot().commits, 1);
    assert_eq!(s.take_pending_changes().len(), 10);
}

#[test]
fn a_unit_without_writes_is_not_counted_as_a_commit() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let gauge = s.db_latency();
    s.run_unit(|s| s.get(&Scope::Global, "missing").map(|_| ()))
        .unwrap();
    let rejected = s.run_unit(|s| s.delete(PLUGIN_A, &Scope::Global, "missing", None));
    assert_eq!(kind(&rejected.unwrap_err()), "not_found");
    assert_eq!(gauge.snapshot().commits, 0);
}

/// 실행 단위는 직렬화·commit 단위일 뿐 rollback 단위가 아니다. 단독 실행과 같은 부분 반영을 남긴다.
#[test]
fn a_compound_operation_keeps_its_partial_effect_inside_a_unit() {
    fn seed(s: &mut dyn MemoryStorage) {
        blackboard::bb_create(s, PLUGIN_A, 1, "bb", None).unwrap();
        blackboard::bb_put(s, PLUGIN_A, 1, "bb", "a", &text("1"), None).unwrap();
        blackboard::bb_put(s, PLUGIN_B, 1, "bb", "b", &text("2"), None).unwrap();
        blackboard::bb_put(s, PLUGIN_A, 1, "bb", "c", &text("3"), None).unwrap();
    }
    let mut alone = MemoryStore::open_in_memory().unwrap();
    let mut united = MemoryStore::open_in_memory().unwrap();
    seed(&mut alone);
    seed(&mut united);
    let alone_gauge = alone.db_latency();
    let united_gauge = united.db_latency();
    let (alone_before, united_before) = (
        alone_gauge.snapshot().commits,
        united_gauge.snapshot().commits,
    );

    let alone_err = blackboard::bb_delete(&mut alone, PLUGIN_A, 1, "bb").unwrap_err();
    let united_err =
        with_unit(&mut united, |s| blackboard::bb_delete(s, PLUGIN_A, 1, "bb")).unwrap_err();

    assert_eq!(kind(&alone_err), "owned");
    assert_eq!(kind(&united_err), "owned");
    let left = |s: &MemoryStore| -> Vec<String> {
        s.list(&Scope::Workspace(1), &ListOpts::default())
            .unwrap()
            .into_iter()
            .map(|e| e.key)
            .collect()
    };
    assert_eq!(left(&united), left(&alone), "단위가 부분 반영을 바꿨다");
    assert!(!left(&alone).iter().any(|k| k.ends_with(".a")));
    assert_eq!(alone_gauge.snapshot().commits - alone_before, 1);
    assert_eq!(united_gauge.snapshot().commits - united_before, 1);
    assert_counter_matches_disk(&united);
}

#[test]
fn import_inside_a_unit_keeps_the_partial_result_of_a_plain_import() {
    let entry = |key: &str| MemoryEntry {
        scope: "global".into(),
        key: key.into(),
        value: text("v"),
        created_at: 0,
        updated_at: 0,
        expires_at: None,
        version: 1,
        owner: None,
    };
    let entries = [entry("a"), entry("b"), entry("bad key!"), entry("c")];
    let mut alone = MemoryStore::open_in_memory().unwrap();
    let mut united = MemoryStore::open_in_memory().unwrap();
    let (alone_gauge, united_gauge) = (alone.db_latency(), united.db_latency());

    let alone_err = alone
        .import_regular(HOST_OWNER, &entries, false)
        .unwrap_err();
    let united_err = united
        .run_unit(|s| s.import_regular(HOST_OWNER, &entries, false))
        .unwrap_err();

    assert_eq!(kind(&alone_err), "invalid_key");
    assert_eq!(kind(&united_err), "invalid_key");
    for s in [&alone, &united] {
        assert_eq!(
            s.count(&Scope::Global, None).unwrap(),
            2,
            "앞 항목이 남지 않았다"
        );
    }
    assert_eq!(
        alone_gauge.snapshot().commits,
        2,
        "단독 import 는 항목마다 commit 한다"
    );
    assert_eq!(united_gauge.snapshot().commits, 1);
}

#[test]
fn an_atomic_batch_inside_a_unit_rolls_back_only_itself() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let gauge = s.db_latency();
    s.run_unit(|s| {
        s.put(
            PLUGIN_A,
            &Scope::Global,
            "kept",
            &text("1"),
            &PutOpts::default(),
        )?;
        let failure = s
            .write_batch(
                BatchMode::Atomic,
                &[
                    put(PLUGIN_A, "dropped", "2", None),
                    put(PLUGIN_A, "kept", "3", Some(9)),
                ],
            )
            .unwrap_err();
        assert_eq!(failure.index, Some(1));
        assert!(s.get(&Scope::Global, "dropped")?.is_none());
        s.put(
            PLUGIN_A,
            &Scope::Global,
            "after",
            &text("4"),
            &PutOpts::default(),
        )
        .map(|_| ())
    })
    .unwrap();
    assert_eq!(gauge.snapshot().commits, 1);
    assert_eq!(
        kinds(&s.take_pending_changes()),
        vec![
            ("kept".into(), MemoryChangeKind::Created, Some(1)),
            ("after".into(), MemoryChangeKind::Created, Some(1)),
        ]
    );
    assert_counter_matches_disk(&s);
}

#[test]
fn a_panic_inside_a_unit_rolls_it_back_and_leaves_the_store_usable() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        s.run_unit(|s| -> Result<()> {
            s.put(
                PLUGIN_A,
                &Scope::Global,
                "a",
                &text("1"),
                &PutOpts::default(),
            )?;
            panic!("body panicked")
        })
    }));
    assert!(caught.is_err());
    assert!(s.get(&Scope::Global, "a").unwrap().is_none());
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(
        s.put(
            PLUGIN_A,
            &Scope::Global,
            "a",
            &text("2"),
            &PutOpts::default()
        )
        .unwrap(),
        1
    );
    assert_counter_matches_disk(&s);
}

// ---- 실제 저장 실패 ----

/// version 열에 정수가 아닌 값을 넣어 조회가 실제 행 변환 오류(`MemoryError::Db`)를 내게 한다.
pub(super) fn plant_unreadable_row(s: &MemoryStore) {
    s.conn
        .execute(
            "INSERT INTO memory
               (scope, key, value, content_type, created_at, updated_at, expires_at, version, owner)
             VALUES ('global', 'unreadable', x'00', 'text/plain', 0, 0, NULL, 'not a number', '_host')",
            [],
        )
        .unwrap();
    let err = s.get(&Scope::Global, "unreadable").unwrap_err();
    assert_eq!(kind(&err), "db", "심은 행이 저장 실패를 내지 않았다: {err}");
}

/// 단위 안의 조회가 저장 실패를 내면 본문이 그 오류를 돌려주든 무시하든 앞선 쓰기까지 되돌린다.
#[test]
fn a_failed_read_inside_a_unit_undoes_the_writes_before_it() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    plant_unreadable_row(&s);
    let used = s.regular_used_bytes;
    let gauge = s.db_latency();

    let propagated = s
        .run_unit(|s| {
            s.put(
                HOST_OWNER,
                &Scope::Global,
                "a",
                &text("1"),
                &PutOpts::default(),
            )?;
            s.get(&Scope::Global, "unreadable").map(|_| ())
        })
        .unwrap_err();
    assert_eq!(kind(&propagated), "db");

    let mut later = None;
    let swallowed = s
        .run_unit(|s| {
            s.put(
                HOST_OWNER,
                &Scope::Global,
                "b",
                &text("2"),
                &PutOpts::default(),
            )?;
            assert!(s.get(&Scope::Global, "unreadable").is_err());
            later = Some(s.put(
                HOST_OWNER,
                &Scope::Global,
                "c",
                &text("3"),
                &PutOpts::default(),
            ));
            Ok(())
        })
        .unwrap_err();
    assert_eq!(kind(&swallowed), "db");
    assert_eq!(kind(&later.unwrap().unwrap_err()), "db");

    for key in ["a", "b", "c"] {
        assert!(
            s.get(&Scope::Global, key).unwrap().is_none(),
            "{key} 가 남았다"
        );
    }
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(s.regular_used_bytes, used);
    assert_eq!(gauge.snapshot().commits, 0);
    // 단위 밖 단독 조회의 실패는 이후 쓰기에 영향을 주지 않는다.
    assert!(s.get(&Scope::Global, "unreadable").is_err());
    assert_eq!(
        s.put(
            HOST_OWNER,
            &Scope::Global,
            "d",
            &text("4"),
            &PutOpts::default()
        )
        .unwrap(),
        1
    );
}

/// SQLite가 단위의 transaction을 스스로 끝낸 상태를 ROLLBACK으로 만든다. 이후 쓰기가 새
/// transaction에서 바로 commit되지 않고 단위 전체가 저장 실패로 끝나야 한다.
#[test]
fn a_unit_whose_transaction_sqlite_ended_does_not_commit_later_writes() {
    let mut s = MemoryStore::open_in_memory().unwrap();
    let gauge = s.db_latency();
    let mut after = None;

    let err = s
        .run_unit(|s| {
            s.put(
                HOST_OWNER,
                &Scope::Global,
                "a",
                &text("1"),
                &PutOpts::default(),
            )?;
            s.conn.execute_batch("ROLLBACK").unwrap();
            assert!(s.conn.is_autocommit());
            after = Some(s.put(
                HOST_OWNER,
                &Scope::Global,
                "b",
                &text("2"),
                &PutOpts::default(),
            ));
            Ok(())
        })
        .unwrap_err();

    assert_eq!(kind(&err), "db", "{err}");
    assert_eq!(kind(&after.unwrap().unwrap_err()), "db");
    assert!(s.get(&Scope::Global, "a").unwrap().is_none());
    assert!(
        s.get(&Scope::Global, "b").unwrap().is_none(),
        "끝난 transaction 뒤 쓰기가 commit됐다"
    );
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(s.regular_used_bytes, 0);
    assert_eq!(gauge.snapshot().commits, 0);

    // 쓰기 없이 끝나도 commit 단계에서 같은 상태를 실패로 본다.
    let ended = s.run_unit(|s| {
        s.put(
            HOST_OWNER,
            &Scope::Global,
            "c",
            &text("3"),
            &PutOpts::default(),
        )?;
        s.conn.execute_batch("ROLLBACK").unwrap();
        Ok(())
    });
    assert_eq!(kind(&ended.unwrap_err()), "db");
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(s.regular_used_bytes, 0);
    assert_eq!(
        s.put(
            HOST_OWNER,
            &Scope::Global,
            "d",
            &text("4"),
            &PutOpts::default()
        )
        .unwrap(),
        1
    );
}

/// 페이지 상한으로 묶음 중간 요청에 실제 SQLITE_FULL을 낸다. 앞 요청까지 함께 되돌린다.
#[test]
fn a_storage_failure_in_the_middle_of_a_batch_undoes_the_requests_before_it() {
    for mode in [BatchMode::Atomic, BatchMode::Independent] {
        let tmp = tempfile::tempdir().unwrap();
        let (mut s, _path) = disk_store(tmp.path());
        s.put(
            HOST_OWNER,
            &Scope::Global,
            "seed",
            &text("x"),
            &PutOpts::default(),
        )
        .unwrap();
        s.take_pending_changes();
        let gauge = s.db_latency();
        let commits = gauge.snapshot().commits;
        let used = s.regular_used_bytes;
        let pages: i64 = s
            .conn
            .query_row("PRAGMA page_count", [], |r| r.get(0))
            .unwrap();
        s.conn
            .query_row(&format!("PRAGMA max_page_count={pages}"), [], |r| {
                r.get::<_, i64>(0)
            })
            .unwrap();
        let big = "x".repeat(64 * 1024);

        let failure = s
            .write_batch(
                mode,
                &[
                    put(HOST_OWNER, "seed", "small", None),
                    put(HOST_OWNER, "big", &big, None),
                    put(HOST_OWNER, "later", "y", None),
                ],
            )
            .unwrap_err();

        assert_eq!(failure.index, Some(1), "{mode:?}");
        assert_eq!(
            failure.error.storage_failure(),
            Some(StorageFailure::DiskFull),
            "{mode:?}: {}",
            failure.error
        );
        let seed = s.get(&Scope::Global, "seed").unwrap().unwrap();
        assert_eq!((seed.version, seed.value), (1, text("x")), "{mode:?}");
        assert!(s.get(&Scope::Global, "later").unwrap().is_none());
        assert_eq!(s.regular_used_bytes, used);
        assert!(s.take_pending_changes().is_empty());
        assert_eq!(gauge.snapshot().commits, commits);
    }
}

/// 본문이 저장 실패를 무시해도 단위는 그 뒤 쓰기를 실행하지 않고 전체를 되돌린다.
#[test]
fn a_unit_that_swallowed_a_storage_failure_still_fails_as_a_whole() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _path) = disk_store(tmp.path());
    let pages: i64 = s
        .conn
        .query_row("PRAGMA page_count", [], |r| r.get(0))
        .unwrap();
    s.conn
        .query_row(&format!("PRAGMA max_page_count={}", pages + 2), [], |r| {
            r.get::<_, i64>(0)
        })
        .unwrap();
    let big = "x".repeat(64 * 1024);
    let mut later = None;

    let err = s
        .run_unit(|s| {
            s.put(
                HOST_OWNER,
                &Scope::Global,
                "small",
                &text("1"),
                &PutOpts::default(),
            )?;
            let full = s.put(
                HOST_OWNER,
                &Scope::Global,
                "big",
                &text(&big),
                &PutOpts::default(),
            );
            assert_eq!(
                full.unwrap_err().storage_failure(),
                Some(StorageFailure::DiskFull)
            );
            later = Some(s.put(
                HOST_OWNER,
                &Scope::Global,
                "later",
                &text("2"),
                &PutOpts::default(),
            ));
            Ok(())
        })
        .unwrap_err();

    assert_eq!(
        err.storage_failure(),
        Some(StorageFailure::DiskFull),
        "{err}"
    );
    let later = later.unwrap().unwrap_err();
    assert_eq!(later.storage_failure(), Some(StorageFailure::DiskFull));
    assert!(s.get(&Scope::Global, "small").unwrap().is_none());
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(s.regular_used_bytes, 0);
}

/// 다른 연결이 쓰기 잠금을 쥐면 첫 요청이 busy로 실패하고 묶음 전체가 확정되지 않는다.
#[test]
fn a_locked_database_fails_the_whole_batch_as_busy() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, path) = disk_store(tmp.path());
    s.conn.busy_timeout(std::time::Duration::ZERO).unwrap();
    let holder = rusqlite::Connection::open(&path).unwrap();
    holder.execute_batch("BEGIN IMMEDIATE").unwrap();
    let ops = [
        put(HOST_OWNER, "a", "1", Some(3)),
        put(HOST_OWNER, "b", "2", None),
    ];

    let failure = s.write_batch(BatchMode::Independent, &ops).unwrap_err();
    assert_eq!(failure.index, Some(1), "거절은 쓰기 잠금을 요구하지 않는다");
    assert_eq!(failure.error.storage_failure(), Some(StorageFailure::Busy));
    assert!(s.take_pending_changes().is_empty());

    holder.execute_batch("ROLLBACK").unwrap();
    let results = s.write_batch(BatchMode::Independent, &ops).unwrap();
    assert_eq!(
        shape(&results),
        vec![Err("cas"), Ok(WriteOutput::Version(1))]
    );
}

/// 파일 크기 상한(RLIMIT_FSIZE)으로 WAL 쓰기를 실제로 실패시켜 commit 단계의 오류를 낸다.
/// 상한은 프로세스 전체에 걸리므로 이 시험 하나만 자식 프로세스로 다시 실행한다.
#[cfg(unix)]
#[test]
fn a_commit_that_hits_a_real_write_error_publishes_nothing() {
    const CHILD_DIR: &str = "TASTY_MEMORY_FSIZE_CHILD_DIR";
    const NAME: &str =
        "batch::batch_tests::a_commit_that_hits_a_real_write_error_publishes_nothing";
    if let Some(dir) = std::env::var_os(CHILD_DIR) {
        commit_fails_under_a_file_size_limit(Path::new(&dir));
        return;
    }

    let tmp = tempfile::tempdir().unwrap();
    let (mut keeper, path) = disk_store(tmp.path());
    keeper
        .put(
            HOST_OWNER,
            &Scope::Global,
            "seed",
            &text("x"),
            &PutOpts::default(),
        )
        .unwrap();
    // WAL을 비워 자식의 commit이 WAL 앞부분부터 쓰게 한다. keeper가 열려 있어 -shm은 유지된다.
    assert!(keeper.checkpoint_truncate().unwrap());

    // SIGXFSZ를 무시해야 write가 EFBIG로 돌아온다. 무시 상태는 exec 뒤에도 유지된다.
    // 상한은 자식이 쓰는 모든 일반 파일에 걸린다. 부모의 stdout·stderr를 물려주면 이미 상한을 넘은
    // 로그 파일에서 자식의 첫 출력부터 실패하므로 파이프로 받는다.
    let output = std::process::Command::new("sh")
        .arg("-c")
        .arg("trap '' XFSZ; ulimit -f 64 && exec \"$0\" \"$@\"")
        .arg(std::env::current_exe().unwrap())
        .args([NAME, "--exact", "--test-threads=1", "--nocapture"])
        .env(CHILD_DIR, tmp.path())
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "자식 시험이 실패했다: {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    assert!(keeper.get(&Scope::Global, "big0").unwrap().is_none());
    drop(keeper);
    let reopened = MemoryStore::open(&path).unwrap();
    assert_eq!(reopened.count(&Scope::Global, None).unwrap(), 1);
}

#[cfg(unix)]
fn commit_fails_under_a_file_size_limit(dir: &Path) {
    let (mut s, _path) = disk_store(dir);
    let gauge = s.db_latency();
    let used = s.regular_used_bytes;
    let chunk = "z".repeat(8 * 1024);
    let ops: Vec<WriteOp> = (0..40)
        .map(|i| put(HOST_OWNER, &format!("big{i}"), &chunk, None))
        .collect();

    let failure = s.write_batch(BatchMode::Independent, &ops).unwrap_err();

    assert_eq!(
        failure.index, None,
        "commit 단계가 아닌 곳에서 실패했다: {failure}"
    );
    assert!(
        matches!(
            failure.error.storage_failure(),
            Some(StorageFailure::Io | StorageFailure::DiskFull)
        ),
        "{failure}"
    );
    assert!(s.get(&Scope::Global, "big0").unwrap().is_none());
    assert!(s.take_pending_changes().is_empty());
    assert_eq!(s.regular_used_bytes, used);
    assert_eq!(gauge.snapshot().commits, 0);
}

// ---- 시험용 저장소 대조 ----

/// 같은 요청 순서를 운영 저장소와 시험용 저장소에 넣고 요청별 결과와 남은 항목을 대조한다.
/// 시험용 저장소가 흉내 내지 않는 quota·TTL·secret·저장 실패는 넣지 않는다.
#[test]
fn the_test_storage_judges_batches_the_way_the_real_store_does() {
    let sequences: Vec<(BatchMode, Vec<WriteOp>)> = vec![
        (
            BatchMode::Independent,
            vec![
                put(PLUGIN_A, "k", "1", None),
                put(PLUGIN_A, "k", "2", Some(1)),
                put(PLUGIN_A, "k", "x", Some(1)),
                put(PLUGIN_B, "k", "x", None),
                put(HOST_OWNER, "k", "host", None),
                delete(PLUGIN_B, "k", None),
                delete(PLUGIN_A, "missing", None),
                delete(PLUGIN_A, "k", Some(3)),
                put(PLUGIN_B, "k", "again", None),
                put(PLUGIN_A, "new", "x", Some(1)),
            ],
        ),
        (
            BatchMode::Atomic,
            vec![put(PLUGIN_A, "a", "1", None), put(PLUGIN_A, "b", "2", None)],
        ),
        (
            BatchMode::Atomic,
            vec![
                put(PLUGIN_A, "c", "1", None),
                put(PLUGIN_B, "a", "steal", None),
            ],
        ),
        (
            BatchMode::Atomic,
            vec![
                delete(PLUGIN_A, "b", Some(1)),
                put(PLUGIN_A, "b", "re", None),
            ],
        ),
    ];
    let mut real = MemoryStore::open_in_memory().unwrap();
    let mut mock = InMemoryStorage::new();
    for (mode, ops) in &sequences {
        let r = MemoryStorage::write_batch(&mut real, *mode, ops);
        let m = MemoryStorage::write_batch(&mut mock, *mode, ops);
        match (r, m) {
            (Ok(r), Ok(m)) => assert_eq!(shape(&r), shape(&m), "{mode:?} {ops:?}"),
            (Err(r), Err(m)) => {
                assert_eq!((r.index, kind(&r.error)), (m.index, kind(&m.error)));
            }
            (r, m) => panic!("결과가 갈렸다: real {r:?}, mock {m:?}"),
        }
    }
    let snapshot = |s: &dyn MemoryStorage| -> Vec<(String, u64, Option<String>, MemoryValue)> {
        s.list(&Scope::Global, &ListOpts::default())
            .unwrap()
            .into_iter()
            .map(|e| (e.key, e.version, e.owner, e.value))
            .collect()
    };
    assert_eq!(snapshot(&real), snapshot(&mock));
    assert!(!snapshot(&real).is_empty());

    // 실행 단위의 부분 반영도 같다.
    let unit = |s: &mut dyn MemoryStorage| {
        with_unit(s, |s| {
            s.put(
                PLUGIN_A,
                &Scope::Global,
                "u1",
                &text("1"),
                &PutOpts::default(),
            )?;
            s.put(
                PLUGIN_B,
                &Scope::Global,
                "a",
                &text("steal"),
                &PutOpts::default(),
            )?;
            s.put(
                PLUGIN_A,
                &Scope::Global,
                "u2",
                &text("2"),
                &PutOpts::default(),
            )
        })
    };
    assert_eq!(kind(&unit(&mut real).unwrap_err()), "owned");
    assert_eq!(kind(&unit(&mut mock).unwrap_err()), "owned");
    assert_eq!(snapshot(&real), snapshot(&mock));
}

/// commit마다 확정된 쓰기 수가 분포에 들어간다. 단독 쓰기·정리는 1, 묶음·단위는 확정된 쓰기
/// 수의 구간이다. VACUUM은 commit이 아니라 따로 센다.
#[test]
fn each_commit_lands_in_the_bucket_of_its_confirmed_writes() {
    let tmp = tempfile::tempdir().unwrap();
    let (mut s, _path) = disk_store(tmp.path());
    let gauge = s.db_latency();
    let expiring = PutOpts {
        expires_at: Some(1),
        cas: None,
    };
    s.put(HOST_OWNER, &Scope::Global, "old.1", &text("1"), &expiring)
        .unwrap();
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "log.1",
        &text("1"),
        &PutOpts::default(),
    )
    .unwrap();
    s.put(
        HOST_OWNER,
        &Scope::Global,
        "log.2",
        &text("2"),
        &PutOpts::default(),
    )
    .unwrap();
    let before = gauge.snapshot();
    assert_eq!(before.writes_per_commit, [3, 0, 0, 0, 0, 0]);

    assert_eq!(s.purge_expired().unwrap().regular, 1);
    assert_eq!(s.prune_prefix_keep_recent("log.", 1).unwrap(), 1);
    assert_eq!(s.purge_scope(&Scope::Surface(7)).unwrap().regular, 0);
    let after = gauge.snapshot();
    assert_eq!(
        after.commits,
        before.commits + 2,
        "a cleanup that removed nothing does not commit"
    );
    assert_eq!(after.writes_per_commit, [5, 0, 0, 0, 0, 0]);

    let ops: Vec<_> = (0..100)
        .map(|i| put(HOST_OWNER, &format!("b.{i}"), "v", None))
        .collect();
    s.write_batch(BatchMode::Independent, &ops).unwrap();
    s.run_unit(|s| {
        s.put(
            HOST_OWNER,
            &Scope::Global,
            "u.1",
            &text("1"),
            &PutOpts::default(),
        )?;
        s.prune_prefix_keep_recent("b.", 10).map(|_| ())
    })
    .unwrap();
    let last = gauge.snapshot();
    assert_eq!(
        last.commits,
        after.commits + 2,
        "a cleanup inside a unit joins its commit"
    );
    assert_eq!(last.writes_per_commit, [5, 1, 0, 0, 1, 0]);

    for i in 0..200 {
        s.put(
            HOST_OWNER,
            &Scope::Global,
            &format!("v.{i}"),
            &text(&"x".repeat(2048)),
            &PutOpts::default(),
        )
        .unwrap();
    }
    s.prune_prefix_keep_recent("v.", 0).unwrap();
    let commits = gauge.snapshot().commits;
    assert!(s.vacuum_if_fragmented(1).unwrap());
    assert_eq!(gauge.snapshot().vacuums, 1);
    assert_eq!(gauge.snapshot().commits, commits);
}
