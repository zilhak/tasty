//! A restore failure payload whose preservation pin could not be written must not be collected:
//! the worker remembers it, retries the pin before each checkpoint and skips the checkpoint while
//! the pin is still missing.
use super::*;

const JOURNAL_ID: &str = "journal-00112233445566778899aabbccddeeff";

struct History;
impl crate::core::layout_persistence::import::ScrollbackSource for History {
    fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
        let mut terminal = tasty_terminal::Terminal::new_detached(80, 24);
        terminal.process_bytes(b"saved");
        Ok(Some(tasty_terminal::disk_scrollback::serialize_lines(
            &terminal.screen_snapshot_lines(),
        )))
    }
}

/// One terminal tab whose capture is a stored payload. Returns that payload.
fn home_with_capture(home: &std::path::Path) -> u64 {
    let directory = home.join("structure");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("journal.json"),
        serde_json::to_vec(
            &serde_json::json!({"version":1,"journal_id":JOURNAL_ID,"phase":"Preparing"}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut store =
        tasty_event_store::EventStore::open(&directory.join("journal.db"), JOURNAL_ID).unwrap();
    let epoch = store.acquire_writer().unwrap();
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"w","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"t","explicit_name":null,"surface":{"Leaf":{"Terminal":{"cwd":null,"restore_command":null,"scrollback_ref":"saved"}}}}],"active_tab":0}}}]});
    crate::core::layout_persistence::import::import_slot(
        &mut store,
        epoch,
        1,
        &layout.to_string(),
        &History,
    )
    .unwrap();
    let model =
        crate::runtime::journal::load(&store, &crate::runtime::journal::engine_stream(1)).unwrap();
    model
        .surfaces
        .values()
        .find_map(|surface| surface.data)
        .unwrap()
        .0
}

fn sql(home: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(home.join("structure/journal.db")).unwrap()
}

fn snapshots(home: &std::path::Path) -> i64 {
    sql(home)
        .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
        .unwrap()
}

fn holders(home: &std::path::Path, payload: u64) -> Vec<String> {
    let connection = sql(home);
    let mut statement = connection
        .prepare("SELECT holder FROM payload_pins WHERE payload_id = ?1")
        .unwrap();
    statement
        .query_map([payload as i64], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

#[test]
fn a_failed_preservation_pin_blocks_checkpoints_until_it_is_retried() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    let payload = home_with_capture(home);
    let executor = Executor::open(
        StructureDecider::default(),
        identity::open(home, None).unwrap(),
    )
    .unwrap();
    let readers = crate::runtime::journal_payload::PayloadReaders::default();
    // The store refuses this one write, as a full disk or I/O error would.
    sql(home)
        .execute_batch(
            "CREATE TRIGGER block_restore_pin BEFORE INSERT ON payload_pins
             WHEN NEW.holder = 'restore-failure' BEGIN SELECT RAISE(ABORT, 'blocked'); END;",
        )
        .unwrap();

    let error = preserve_failed_payload(&executor, tasty_core::DataRef(payload)).unwrap_err();
    assert!(error.contains("retrying"), "{error}");
    assert!(executor.decider().unpreserved().contains(&payload));
    let before = snapshots(home);
    capture::checkpoint(&executor, &readers).unwrap();
    assert_eq!(
        snapshots(home),
        before,
        "no snapshot, compaction or GC while the original is unpinned"
    );

    sql(home)
        .execute_batch("DROP TRIGGER block_restore_pin;")
        .unwrap();
    capture::checkpoint(&executor, &readers).unwrap();
    assert!(
        holders(home, payload)
            .iter()
            .any(|holder| holder == RESTORE_FAILURE_HOLDER)
    );
    assert!(executor.decider().unpreserved().is_empty());
    assert_eq!(
        snapshots(home),
        before + 1,
        "checkpoints resume once it is kept"
    );
}

#[test]
fn a_missing_payload_has_nothing_to_preserve_and_does_not_block_checkpoints() {
    let dir = tempfile::tempdir().unwrap();
    let home = dir.path();
    home_with_capture(home);
    let executor = Executor::open(
        StructureDecider::default(),
        identity::open(home, None).unwrap(),
    )
    .unwrap();
    let error = preserve_failed_payload(&executor, tasty_core::DataRef(9_999)).unwrap_err();
    assert!(!error.contains("retrying"), "{error}");
    assert!(executor.decider().unpreserved().is_empty());
}
