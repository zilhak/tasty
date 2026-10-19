//! One layout slot's unreadable records stay that slot's loss: the journal opens, other slots
//! restore, the damaged slot is never written and the stored rows are kept.
use super::stall_budget::StallBudget;
use super::*;

const JOURNAL_ID: &str = "journal-fedcba9876543210fedcba9876543210";

struct TempHome {
    dir: tempfile::TempDir,
}
impl TempHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        tasty_utils::path::push_home_override(dir.path().to_path_buf());
        Self { dir }
    }
    fn database(&self) -> std::path::PathBuf {
        self.dir.path().join("structure").join("journal.db")
    }
}
impl Drop for TempHome {
    fn drop(&mut self) {
        tasty_utils::path::pop_home_override();
    }
}

struct NoHistory;
impl crate::core::layout_persistence::import::ScrollbackSource for NoHistory {
    fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

/// Slots 1 and 2 each hold one workspace; the last event of slot 2 is made undecodable.
fn damaged_home() -> TempHome {
    damaged_home_with(&[2])
}

/// Slots 1 to 3 each hold one workspace; the last event of every slot in `damaged` is made
/// undecodable.
fn damaged_home_with(damaged: &[u32]) -> TempHome {
    let home = TempHome::new();
    let directory = home.dir.path().join("structure");
    std::fs::create_dir(&directory).unwrap();
    std::fs::write(
        directory.join("journal.json"),
        serde_json::to_vec(
            &serde_json::json!({"version":1,"journal_id":JOURNAL_ID,"phase":"Preparing"}),
        )
        .unwrap(),
    )
    .unwrap();
    let mut store = tasty_event_store::EventStore::open(&home.database(), JOURNAL_ID).unwrap();
    let epoch = store.acquire_writer().unwrap();
    for slot in [1u32, 2, 3] {
        let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":format!("slot-{slot}"),"subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"t","explicit_name":null,"surface":{"Leaf":{"Terminal":{"cwd":null,"restore_command":null,"scrollback_ref":null}}}}],"active_tab":0}}}]});
        crate::core::layout_persistence::import::import_slot(
            &mut store,
            epoch,
            slot,
            &layout.to_string(),
            &NoHistory,
        )
        .unwrap();
    }
    drop(store);
    let connection = rusqlite::Connection::open(home.database()).unwrap();
    for slot in damaged {
        let stream = format!("structure:slot-{slot}");
        let changed = connection
            .execute(
                "UPDATE events SET payload = X'00' WHERE stream_id = ?1
                 AND stream_revision = (SELECT MAX(stream_revision) FROM events WHERE stream_id = ?1)",
                [&stream],
            )
            .unwrap();
        assert_eq!(changed, 1);
    }
    home
}

/// The damaged slot's stored events and the number of snapshots. Healthy slots keep writing.
fn rows(home: &TempHome) -> (Vec<(i64, Vec<u8>)>, i64) {
    let connection = rusqlite::Connection::open(home.database()).unwrap();
    let mut statement = connection
        .prepare(
            "SELECT stream_revision, payload FROM events WHERE stream_id = 'structure:slot-2'
             ORDER BY stream_revision",
        )
        .unwrap();
    let events = statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let snapshots = connection
        .query_row("SELECT COUNT(*) FROM snapshots", [], |row| row.get(0))
        .unwrap();
    (events, snapshots)
}

fn session(slot: Option<u32>) -> EngineSession {
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = String::new();
    settings.general.restore_layout = true;
    EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: slot,
            memory: Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            )),
            runner_registry: Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        },
        settings,
    )
    .unwrap()
}

/// The journal starts before the session: building a session in tests can move this thread's
/// home override, and the worker must open the damaged home.
fn boot(selection: EngineSelection, slot: Option<u32>) -> (JournalApplication, EngineSession) {
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    let mut session = session(slot);
    journal.begin_engine(&session, selection).unwrap();
    let mut stall = StallBudget::new(&journal);
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        if journal.is_ready(session.id) {
            return (journal, session);
        }
        stall.nap("bootstrap with a damaged slot");
    }
}

fn workspace_names(session: &EngineSession) -> Vec<String> {
    session
        .core_state
        .local_workspaces()
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect()
}

#[test]
fn a_damaged_slot_is_reported_and_the_healthy_slot_restores() {
    let home = damaged_home();
    let before = rows(&home);
    let (mut journal, first) = boot(
        EngineSelection::Slot {
            slot: 1,
            resume: true,
        },
        Some(1),
    );
    assert!(!journal.is_halted());
    assert_eq!(workspace_names(&first), vec!["slot-1".to_owned()]);
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].level,
        crate::notification::NotificationLevel::Warning
    );
    assert!(notices[0].body.contains(" 2 "), "{}", notices[0].body);
    assert_eq!(journal.damaged_layout_slots().collect::<Vec<_>>(), vec![2]);
    drop(journal);
    assert_eq!(
        rows(&home),
        before,
        "no event, snapshot or compaction touches the damaged records"
    );
}

#[test]
fn opening_the_damaged_slot_starts_a_fresh_slot_instead() {
    let home = damaged_home();
    let before = rows(&home);
    let (journal, engine) = boot(
        EngineSelection::Slot {
            slot: 2,
            resume: true,
        },
        Some(2),
    );
    assert!(!journal.is_halted());
    assert_eq!(
        engine.persistence.slot,
        Some(4),
        "slot 3 is a healthy existing slot"
    );
    assert!(
        !workspace_names(&engine).contains(&"slot-2".to_owned()),
        "the damaged layout is not shown as if it were whole"
    );
    drop(journal);
    assert_eq!(rows(&home), before, "the damaged slot gets no new records");
}

#[test]
fn the_worker_refuses_to_open_a_damaged_slot() {
    let home = damaged_home();
    let (mut journal, _engine) = boot(
        EngineSelection::Slot {
            slot: 1,
            resume: true,
        },
        Some(1),
    );
    let ticket = journal.next_ticket;
    journal.next_ticket += 1;
    journal
        .worker
        .submit(Request {
            ticket,
            work: Work::OpenEngine {
                selection: EngineSelection::Slot {
                    slot: 2,
                    resume: true,
                },
                normal_category_name: String::new(),
                surface_floor: 0,
            },
        })
        .unwrap();
    let mut stall = StallBudget::new(&journal);
    let result = loop {
        match journal.worker.try_recv() {
            Ok(Completion::Finished {
                ticket: got,
                result,
            }) if got == ticket => break result,
            Ok(_) | Err(std::sync::mpsc::TryRecvError::Empty) => stall.nap("refused opening"),
            Err(error) => panic!("{error:?}"),
        }
    };
    let error = result
        .expect_err("a damaged slot is never opened")
        .to_string();
    assert!(error.contains("damaged"), "{error}");
    drop(journal);
    drop(home);
}

#[test]
fn headless_starts_a_fresh_stream_despite_a_damaged_slot() {
    let home = damaged_home();
    let (journal, engine) = boot(EngineSelection::FreshHeadless, None);
    assert!(!journal.is_halted());
    assert!(engine.journal_binding.is_some());
    drop(journal);
    drop(home);
}

#[test]
fn two_damaged_openings_in_one_bootstrap_get_different_fresh_slots() {
    let home = damaged_home_with(&[2, 3]);
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    let mut first = session(Some(2));
    let mut second = session(Some(3));
    for engine in [&first, &second] {
        let slot = engine.persistence.slot.unwrap();
        journal
            .begin_engine(engine, EngineSelection::Slot { slot, resume: true })
            .unwrap();
    }
    let mut stall = StallBudget::new(&journal);
    while !(journal.is_ready(first.id) && journal.is_ready(second.id)) {
        journal
            .poll_bootstrap(&mut [&mut first, &mut second], None)
            .unwrap();
        journal.poll_restore_bootstrap(&first).unwrap();
        journal.poll_restore_bootstrap(&second).unwrap();
        stall.nap("two damaged openings");
    }
    assert!(!journal.is_halted());
    let slots = [first.persistence.slot, second.persistence.slot];
    assert_eq!(
        slots,
        [Some(4), Some(5)],
        "each opening gets its own fresh slot"
    );
    drop(journal);
    drop(home);
}

/// A View save writes a domain snapshot of the loaded models and prunes cache snapshots. While a
/// slot is damaged that snapshot would leave the slot out, so the next start would no longer find
/// the damage; the save waits instead.
#[cfg(feature = "gui")]
#[test]
fn a_view_save_waits_while_a_slot_is_damaged() {
    let home = damaged_home();
    let before = rows(&home);
    let (mut journal, mut first) = boot(
        EngineSelection::Slot {
            slot: 1,
            resume: true,
        },
        Some(1),
    );
    let workspace = first.core_state.local_workspaces()[0].id;
    journal.queue_view(
        crate::runtime::journal_product::view_record::StoredView::capture(
            first.journal_binding.clone().unwrap(),
            &first.core_state,
            Some(workspace),
            &crate::model::StructurePresentationSnapshot::default(),
        ),
    );
    let mut stall = StallBudget::new(&journal);
    while journal.has_pending_view_writes() {
        journal.poll_bootstrap(&mut [&mut first], None).unwrap();
        stall.nap("deferred View save");
    }
    assert!(
        !journal.view_writes.has_failures(),
        "waiting is not a failure"
    );
    drop(journal);
    assert_eq!(rows(&home), before, "no snapshot is written or pruned");
    let manifests: i64 = rusqlite::Connection::open(home.database())
        .unwrap()
        .query_row("SELECT COUNT(*) FROM restore_manifests", [], |row| {
            row.get(0)
        })
        .unwrap();
    assert_eq!(manifests, 0);

    // Building the first session moved this thread's home; point the restart at the damaged home.
    tasty_utils::path::push_home_override(home.dir.path().to_path_buf());
    let (mut journal, _first) = boot(
        EngineSelection::Slot {
            slot: 1,
            resume: true,
        },
        Some(1),
    );
    assert_eq!(
        journal.take_notices().len(),
        1,
        "the next start still finds the damaged slot"
    );
    assert_eq!(journal.damaged_layout_slots().collect::<Vec<_>>(), vec![2]);
    drop(journal);
    tasty_utils::path::pop_home_override();
}

/// A snapshot whose checksum holds but whose body cannot be decoded is skipped when loading. Its
/// row stays: View saves wait instead of pruning it, and the next start warns again.
#[cfg(feature = "gui")]
#[test]
fn a_skipped_snapshot_row_survives_view_saves_and_restarts() {
    use sha2::Digest;
    let home = damaged_home_with(&[]);
    let body = b"checksum holds but this is no snapshot".to_vec();
    let snapshot = {
        let mut store = tasty_event_store::EventStore::open(&home.database(), JOURNAL_ID).unwrap();
        let epoch = store.acquire_writer().unwrap();
        let models = crate::runtime::journal::load_all(&store).unwrap();
        crate::runtime::journal::save_snapshot(&mut store, epoch, &models).unwrap()
    };
    rusqlite::Connection::open(home.database())
        .unwrap()
        .execute(
            "UPDATE payloads SET bytes = ?1, sha256 = ?2 WHERE payload_id =
             (SELECT payload_id FROM snapshots WHERE snapshot_id = ?3)",
            rusqlite::params![body, sha2::Sha256::digest(&body).to_vec(), snapshot as i64],
        )
        .unwrap();
    let kept = || -> Vec<u8> {
        rusqlite::Connection::open(home.database())
            .unwrap()
            .query_row(
                "SELECT p.bytes FROM snapshots s JOIN payloads p ON p.payload_id = s.payload_id
                 WHERE s.snapshot_id = ?1",
                [snapshot as i64],
                |row| row.get(0),
            )
            .unwrap()
    };
    for _ in 0..2 {
        tasty_utils::path::push_home_override(home.dir.path().to_path_buf());
        let (mut journal, mut first) = boot(
            EngineSelection::Slot {
                slot: 1,
                resume: true,
            },
            Some(1),
        );
        assert_eq!(workspace_names(&first), vec!["slot-1".to_owned()]);
        assert_eq!(journal.take_notices().len(), 1, "every start warns");
        let workspace = first.core_state.local_workspaces()[0].id;
        journal.queue_view(
            crate::runtime::journal_product::view_record::StoredView::capture(
                first.journal_binding.clone().unwrap(),
                &first.core_state,
                Some(workspace),
                &crate::model::StructurePresentationSnapshot::default(),
            ),
        );
        let mut stall = StallBudget::new(&journal);
        while journal.has_pending_view_writes() {
            journal.poll_bootstrap(&mut [&mut first], None).unwrap();
            stall.nap("deferred View save");
        }
        drop(journal);
        tasty_utils::path::pop_home_override();
        assert_eq!(kept(), body, "the skipped snapshot row is kept");
    }
}
