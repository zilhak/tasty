//! schema 버전 표와 durability 설정.

use crate::schema::current_version;
use crate::{EventStore, SCHEMA_VERSION, StoreError};

use super::common::{JOURNAL, db_path, open, raw};

#[test]
fn fresh_journal_records_every_migration_once() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    drop(open(&path));
    drop(open(&path));
    let conn = raw(&path);
    assert_eq!(current_version(&conn).expect("version"), SCHEMA_VERSION);
    let rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM schema_migrations", [], |r| r.get(0))
        .expect("count");
    assert_eq!(rows, i64::from(SCHEMA_VERSION));
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("journal mode");
    assert_eq!(mode, "wal");
}

#[test]
fn newer_schema_is_refused() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    drop(open(&path));
    let newer = SCHEMA_VERSION + 1;
    raw(&path)
        .execute(
            "INSERT INTO schema_migrations (version) VALUES (?1)",
            [newer],
        )
        .expect("simulate a newer build");

    let err = EventStore::open(&path, JOURNAL).err().expect("refused");
    assert!(
        matches!(err, StoreError::SchemaTooNew { found, supported } if found == newer && supported == SCHEMA_VERSION),
        "{err:?}"
    );
}

#[test]
fn foreign_sqlite_file_is_refused_without_changes() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    raw(&path)
        .execute_batch("CREATE TABLE memory (key TEXT PRIMARY KEY, value BLOB)")
        .expect("create a non-journal database");
    let before = std::fs::read(&path).expect("read before");

    let err = EventStore::open(&path, JOURNAL).err().expect("refused");
    assert!(matches!(err, StoreError::NotAJournal), "{err:?}");

    assert_eq!(std::fs::read(&path).expect("read after"), before);
    let wal = dir.path().join("journal.db-wal");
    assert!(!wal.exists(), "the refused open must not switch to WAL");
    let conn = raw(&path);
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |r| r.get(0))
        .expect("journal mode");
    assert_eq!(mode, "delete");
    let tables: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
        .expect("prepare")
        .query_map([], |r| r.get(0))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("names");
    assert_eq!(tables, ["memory"]);
}

/// 여러 프로세스·스레드가 빈 경로를 동시에 처음 열어도 모두 성공하고 migration은 한 번만 적용된다.
#[test]
fn concurrent_first_opens_all_succeed() {
    const OPENERS: usize = 8;
    let dir = tempfile::tempdir().expect("tempdir");
    let path = db_path(&dir);
    let barrier = std::sync::Arc::new(std::sync::Barrier::new(OPENERS));
    let handles: Vec<_> = (0..OPENERS)
        .map(|_| {
            let path = path.clone();
            let barrier = std::sync::Arc::clone(&barrier);
            std::thread::spawn(move || {
                barrier.wait();
                EventStore::open(&path, JOURNAL).map(drop)
            })
        })
        .collect();
    let failures: Vec<String> = handles
        .into_iter()
        .map(|h| h.join().expect("opener thread"))
        .filter_map(|r| r.err().map(|e| e.to_string()))
        .collect();
    assert!(failures.is_empty(), "failed opens: {failures:?}");

    let conn = raw(&path);
    let rows: Vec<(u32, i64)> = conn
        .prepare(
            "SELECT version, COUNT(*) FROM schema_migrations GROUP BY version ORDER BY version",
        )
        .expect("prepare")
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
        .expect("query")
        .collect::<Result<_, _>>()
        .expect("rows");
    let expected: Vec<(u32, i64)> = (1..=SCHEMA_VERSION).map(|v| (v, 1)).collect();
    assert_eq!(rows, expected);
}
