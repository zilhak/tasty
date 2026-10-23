use super::*;

#[test]
fn actual_sqlite_commit_unknown_keeps_dirty_age_until_a_later_confirmed_snapshot() {
    let mut f = fixture_with(
        |conn| {
            conn.execute_batch("CREATE TABLE age_parent(id INTEGER PRIMARY KEY);
            CREATE TABLE age_child(id INTEGER REFERENCES age_parent(id) DEFERRABLE INITIALLY DEFERRED);
            CREATE TRIGGER age_fault AFTER INSERT ON recent_files BEGIN INSERT INTO age_child VALUES(1); END;").unwrap();
        },
        Limits::default(),
        Arc::new(SystemClock),
    );
    let clock = Arc::new(ManualClock::default());
    let mut recent = RecentFiles::with_clock(
        std::collections::HashMap::new(),
        Box::new(WriterRecentSink {
            queue: f.shared.queue.clone(),
            pending: Vec::new(),
        }),
        clock.clone(),
    );
    recent.add("markdown", "/unknown.md".into());
    let deadline = Instant::now() + WAIT;
    while recent.pending() != 0 {
        recent.settle();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    clock.advance(Duration::from_micros(31));
    let unknown = recent.save_snapshot();
    let unknown_count = f.parts.writer.stats().completed_unknown;
    let disk_before = disk_paths(&f.path, "markdown");
    Connection::open(&f.path)
        .unwrap()
        .execute_batch("DROP TRIGGER age_fault")
        .unwrap();
    recent.add("markdown", "/confirmed.md".into());
    let age_before_confirmation = recent.save_snapshot();
    while recent.pending() != 0 {
        recent.settle();
        assert!(Instant::now() < deadline);
        std::thread::yield_now();
    }
    let confirmed = recent.save_snapshot();
    shutdown(&mut f);
    assert_eq!(
        unknown_count, 1,
        "real COMMIT fault must reach Unknown settlement"
    );
    assert!(disk_before.is_empty());
    assert_eq!(unknown.dirty_age_us, Some(31));
    assert_eq!(age_before_confirmation.dirty_age_us, Some(31));
    assert_eq!(confirmed.dirty_age_us, None);
    assert_eq!(
        recent.revisions("markdown"),
        crate::recent_files::Revisions {
            visible: 2,
            committed: 2
        }
    );
    assert_eq!(
        reopened_paths(&f.path, "markdown"),
        vec!["/confirmed.md", "/unknown.md"]
    );
}
