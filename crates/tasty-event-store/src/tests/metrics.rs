use super::common::{append, command, fresh, key};
use crate::{CommitRequest, ExpectedRevision};

#[test]
fn physical_commits_count_sql_success_not_calls_duplicates_or_rolled_back_writes() {
    let (_dir, mut store, epoch) = fresh();
    let initial = store.commit_statistics();
    assert_eq!(initial.physical_commits, 2); // opening migration/binding and writer epoch
    assert_eq!(initial.structural_commits, 0);
    let mut request = CommitRequest::new(epoch);
    request.command = Some(command("cmd-metrics", Some(key("metrics", "one")), b"one"));
    request
        .appends
        .push(append("s", ExpectedRevision::NoStream, &["event-metrics"]));
    store.commit(&request).unwrap();
    let first = store.commit_statistics();
    assert_eq!(first.physical_commits, initial.physical_commits + 1);
    assert_eq!(first.structural_commits, 1);
    assert_eq!(first.logical_requests, 1);
    store.commit(&request).unwrap();
    assert_eq!(store.commit_statistics(), first); // duplicate returns without COMMIT
    request.command = None;
    assert!(store.commit(&request).is_err());
    assert_eq!(store.commit_statistics(), first); // failed revision check rolls back
    store.put_payload(epoch, &[1]).unwrap();
    let auxiliary = store.commit_statistics();
    assert_eq!(auxiliary.physical_commits, first.physical_commits + 1);
    assert_eq!(auxiliary.structural_commits, 1);
}

#[test]
fn sqlite_commit_failure_is_not_counted_as_a_success() {
    let (_dir, mut store, epoch) = fresh();
    let tx = store.write_tx(epoch).unwrap();
    tx.execute_batch(
        "CREATE TABLE metric_parent(id INTEGER PRIMARY KEY);
        CREATE TABLE metric_child(parent INTEGER REFERENCES metric_parent(id)
        DEFERRABLE INITIALLY DEFERRED);",
    )
    .unwrap();
    tx.commit().unwrap();
    let before = store.commit_statistics();
    let tx = store.write_tx(epoch).unwrap();
    tx.execute("INSERT INTO metric_child VALUES(123)", [])
        .unwrap();
    assert!(tx.commit().is_err());
    let after = store.commit_statistics();
    assert_eq!(after.physical_commits, before.physical_commits);
    assert_eq!(after.failed_commits, before.failed_commits + 1);
    assert_eq!(
        store
            .conn
            .query_row("SELECT count(*) FROM metric_child", [], |r| r
                .get::<_, u64>(0))
            .unwrap(),
        0
    );
}
