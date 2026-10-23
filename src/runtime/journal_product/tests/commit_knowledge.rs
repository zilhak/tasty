//! Real worker + SQLite failures distinguish uncertain COMMIT from deterministic rollback.
use super::*;

fn sql(home: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(home.join("structure/journal.db")).unwrap()
}

fn rename() -> Work {
    Work::Resolve {
        changes: vec![StreamCommand {
            stream: "structure:slot-1".into(),
            command: StructuralCommand::RenameCategory {
                id: 1,
                name: "must-not-commit".into(),
            },
        }],
        response: None,
    }
}

fn ready(home: &std::path::Path) -> JournalWorker {
    let worker = start(home);
    seed_category(&worker);
    submit(&worker, 200, Work::Admit(header("fault")));
    finished(&worker, 200).unwrap();
    assert!(worker.metrics_snapshot().publication_known);
    worker
}

fn inject_commit_failure(home: &std::path::Path, table: &str, operation: &str) {
    assert!(matches!(
        (table, operation),
        ("commands", "INSERT") | ("id_reservations", "UPDATE")
    ));
    sql(home).execute_batch(&format!("CREATE TABLE metrics_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE metrics_child(parent INTEGER REFERENCES metrics_parent(id) DEFERRABLE INITIALLY DEFERRED);
        CREATE TRIGGER metrics_commit_failure AFTER {operation} ON {table}
        BEGIN INSERT INTO metrics_child VALUES(999); END;")).unwrap();
}

fn assert_unknown(worker: &JournalWorker) {
    let snapshot = worker.metrics_snapshot();
    let store = snapshot.store.unwrap();
    assert_eq!(store.commits.failed_commits, 1);
    assert_eq!(store.commit_failure.samples, 1);
    assert!(
        !snapshot.publication_known,
        "uncertain physical COMMIT must invalidate the cached canonical cut"
    );
    assert_eq!(snapshot.unpublished_age_us, None);
    let public = crate::adapters::ipc::handler::journal_pressure::snapshot_json(Some(&snapshot));
    assert_eq!(public["publication_known"], false);
    assert!(public["unpublished_age_us"].is_null());
}

#[test]
fn group_sql_commit_failure_invalidates_cached_cut_until_reopen() {
    let home = tempfile::tempdir().unwrap();
    let worker = ready(home.path());
    inject_commit_failure(home.path(), "commands", "INSERT");
    submit(&worker, 200, rename());
    assert!(
        matches!(receive(&worker), Completion::Halted(reason) if reason.contains("durable outcome"))
    );
    assert!(finished(&worker, 200).is_err());
    assert_unknown(&worker);
    drop(worker);
    sql(home.path())
        .execute_batch("DROP TRIGGER metrics_commit_failure")
        .unwrap();
    let reopened = start(home.path());
    submit(&reopened, 201, Work::ReadEngine("structure:slot-1".into()));
    finished(&reopened, 201).unwrap();
    let verified = reopened.metrics_snapshot();
    assert!(verified.publication_known);
    assert_eq!(verified.store.unwrap().commits.failed_commits, 0);
}

#[test]
fn scalar_sql_commit_failure_invalidates_cached_cut_without_changing_reply_behavior() {
    let home = tempfile::tempdir().unwrap();
    let worker = ready(home.path());
    inject_commit_failure(home.path(), "id_reservations", "UPDATE");
    submit(&worker, 200, Work::Reserve(vec![(IdKind::Category, 1)]));
    assert!(finished(&worker, 200).is_err());
    assert_unknown(&worker);
    // Existing scalar processing can still answer reads. It must not relabel an old cut known.
    submit(&worker, 201, Work::ReadEngine("structure:slot-1".into()));
    finished(&worker, 201).unwrap();
    assert_unknown(&worker);
}

#[test]
fn deterministic_sql_write_rollback_keeps_the_acknowledged_cut_known() {
    let home = tempfile::tempdir().unwrap();
    let worker = ready(home.path());
    sql(home.path())
        .execute_batch(
            "CREATE TRIGGER metrics_rollback BEFORE INSERT ON commands
        BEGIN SELECT RAISE(ABORT, 'deterministic rollback'); END;",
        )
        .unwrap();
    submit(&worker, 200, rename());
    assert!(matches!(receive(&worker), Completion::Halted(_)));
    assert!(finished(&worker, 200).is_err());
    let snapshot = worker.metrics_snapshot();
    assert!(snapshot.publication_known);
    assert_eq!(snapshot.unpublished_age_us, None);
    assert_eq!(snapshot.store.unwrap().commits.failed_commits, 0);
    assert_eq!(snapshot.store.unwrap().commit_failure.samples, 0);
}

#[test]
fn deterministic_scalar_validation_rejection_keeps_the_cut_known() {
    let home = tempfile::tempdir().unwrap();
    let worker = ready(home.path());
    submit(&worker, 200, Work::Reserve(vec![(IdKind::Category, 0)]));
    assert!(finished(&worker, 200).is_err());
    let snapshot = worker.metrics_snapshot();
    assert!(snapshot.publication_known);
    assert_eq!(snapshot.unpublished_age_us, None);
    assert_eq!(snapshot.store.unwrap().commits.failed_commits, 0);
}
