//! One surface's unreadable restore payload must stay that surface's loss: the rest of the
//! engine restores, the application never halts, the surface starts empty and the original row
//! is kept.
use super::stall_budget::StallBudget;
use super::*;

const MARKER: &str = "MARKER-OLD";
const JOURNAL_ID: &str = "journal-0123456789abcdef0123456789abcdef";

/// Keeps this test's journal in its own temporary home, never the real one.
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

struct History(Vec<u8>);
impl crate::core::layout_persistence::import::ScrollbackSource for History {
    fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(Some(self.0.clone()))
    }
}

/// Three terminal tabs with captures holding `MARKER`; tab 1 is selected. Returns each tab's
/// surface and capture reference.
fn import_slot(home: &TempHome) -> Vec<(u32, tasty_core::DataRef)> {
    let mut old = tasty_terminal::Terminal::new_detached(80, 24);
    old.process_bytes(MARKER.as_bytes());
    let history = History(tasty_terminal::disk_scrollback::serialize_lines(
        &old.screen_snapshot_lines(),
    ));
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
    let cwd = home.dir.path().to_string_lossy();
    let tabs:Vec<_> = (0..3).map(|index|serde_json::json!({"name":format!("terminal-{index}"),"explicit_name":null,"surface":{"Leaf":{"Terminal":{"cwd":cwd,"restore_command":"printf 'RESTORE-%s\\n' READY; exec sleep 60","scrollback_ref":"saved"}}}})).collect();
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"restore","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":tabs,"active_tab":1}}}]});
    let mut store = tasty_event_store::EventStore::open(&home.database(), JOURNAL_ID).unwrap();
    let epoch = store.acquire_writer().unwrap();
    let ids: Vec<u32> = crate::core::layout_persistence::import::import_slot(
        &mut store,
        epoch,
        1,
        &layout.to_string(),
        &history,
    )
    .unwrap()
    .mapping
    .workspaces[0]
        .panes[0]
        .tabs
        .iter()
        .map(|tab| tab.surfaces[0])
        .collect();
    let model =
        crate::runtime::journal::load(&store, &crate::runtime::journal::engine_stream(1)).unwrap();
    ids.into_iter()
        .map(|id| (id, model.surfaces[&id].data.unwrap()))
        .collect()
}

/// Replace a payload's bytes. `keep_checksum` also rewrites the stored digest, so the store reads
/// the row and only the surface decoder rejects it.
fn damage(home: &TempHome, reference: tasty_core::DataRef, keep_checksum: bool) -> Vec<u8> {
    use sha2::Digest;
    let bytes = b"not a surface capture".to_vec();
    let connection = rusqlite::Connection::open(home.database()).unwrap();
    let digest = if keep_checksum {
        sha2::Sha256::digest(&bytes).to_vec()
    } else {
        connection
            .query_row(
                "SELECT sha256 FROM payloads WHERE payload_id = ?1",
                [reference.0 as i64],
                |row| row.get::<_, Vec<u8>>(0),
            )
            .unwrap()
    };
    connection
        .execute(
            "UPDATE payloads SET bytes = ?1, sha256 = ?2 WHERE payload_id = ?3",
            rusqlite::params![bytes, digest, reference.0 as i64],
        )
        .unwrap();
    bytes
}

fn stored(home: &TempHome, reference: tasty_core::DataRef) -> (Vec<u8>, Vec<String>) {
    let connection = rusqlite::Connection::open(home.database()).unwrap();
    let bytes = connection
        .query_row(
            "SELECT bytes FROM payloads WHERE payload_id = ?1",
            [reference.0 as i64],
            |row| row.get::<_, Vec<u8>>(0),
        )
        .unwrap();
    let mut statement = connection
        .prepare("SELECT holder FROM payload_pins WHERE payload_id = ?1")
        .unwrap();
    let holders = statement
        .query_map([reference.0 as i64], |row| row.get::<_, String>(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    (bytes, holders)
}

fn boot() -> (JournalApplication, EngineSession, StallBudget) {
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = String::new();
    settings.general.restore_layout = true;
    settings.general.restore_surface_content = true;
    let mut session = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(1),
            memory: Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            )),
            runner_registry: Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        },
        settings,
    )
    .unwrap();
    journal
        .begin_engine(
            &session,
            EngineSelection::Slot {
                slot: 1,
                resume: true,
            },
        )
        .unwrap();
    let mut stall = StallBudget::new(&journal);
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        if journal.is_ready(session.id) {
            break;
        }
        stall.nap("restore bootstrap");
    }
    (journal, session, stall)
}

fn screen(session: &EngineSession, surface: u32) -> String {
    session
        .runtime
        .terminals
        .get(surface)
        .unwrap()
        .with_content(|view| view.screen_text(true))
}

fn is_placeholder(session: &EngineSession, surface: u32) -> bool {
    session.runtime.surfaces.get(&surface).is_some_and(|owner| {
        owner
            .as_any()
            .is::<crate::runtime::surface_restorer::JournalPlaceholder>()
    })
}

/// Poll until the preservation request for `reference` has been answered.
fn wait_preserved(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    stall: &mut StallBudget,
    home: &TempHome,
    reference: tasty_core::DataRef,
) -> (Vec<u8>, Vec<String>) {
    loop {
        journal.poll_bootstrap(&mut [&mut *session], None).unwrap();
        let (bytes, holders) = stored(home, reference);
        if holders.iter().any(|holder| holder == "restore-failure") {
            return (bytes, holders);
        }
        stall.nap("restore failure preservation");
    }
}

#[test]
fn an_unreadable_selected_payload_starts_that_tab_empty_and_keeps_the_rest() {
    let home = TempHome::new();
    let tabs = import_slot(&home);
    let damaged = damage(&home, tabs[1].1, false);

    let (mut journal, mut session, mut stall) = boot();
    assert!(!journal.is_halted());
    assert!(
        is_placeholder(&session, tabs[0].0) && is_placeholder(&session, tabs[2].0),
        "the other tabs stay restorable"
    );
    let selected = screen(&session, tabs[1].0);
    assert!(!selected.contains(MARKER), "{selected}");
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].level,
        crate::notification::NotificationLevel::Warning
    );
    assert!(notices[0].body.contains("restore"), "{}", notices[0].body);
    let (bytes, _) = wait_preserved(&mut journal, &mut session, &mut stall, &home, tabs[1].1);
    assert_eq!(bytes, damaged, "the original row is kept as it was");
    assert_eq!(
        session.core_state.local_workspaces()[0].all_surface_ids(),
        tabs.iter().map(|tab| tab.0).collect::<Vec<_>>()
    );
    drop(journal);
    drop(home);
}

#[test]
fn an_undecodable_lazy_payload_fails_only_that_tab_when_it_is_activated() {
    let home = TempHome::new();
    let tabs = import_slot(&home);
    let damaged = damage(&home, tabs[2].1, true);

    // Every pending capture is read at bootstrap, so the loss is reported before activation.
    let (mut journal, mut session, mut stall) = boot();
    assert!(!journal.is_halted());
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        is_placeholder(&session, tabs[2].0),
        "the lazy tab waits for activation"
    );
    assert!(
        journal
            .activate_restored_surface(&session, tabs[2].0)
            .unwrap()
    );
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        if !journal.has_creation(session.id) {
            break;
        }
        stall.nap("lazy activation");
    }
    assert!(!journal.is_halted(), "{:?}", journal.halted);
    assert!(!is_placeholder(&session, tabs[2].0), "the tab is usable");
    let lazy = screen(&session, tabs[2].0);
    assert!(!lazy.contains(MARKER), "{lazy}");
    assert!(
        is_placeholder(&session, tabs[0].0),
        "an untouched tab is still pending"
    );
    assert!(
        journal.take_notices().is_empty(),
        "one loss is reported once"
    );
    let (bytes, _) = wait_preserved(&mut journal, &mut session, &mut stall, &home, tabs[2].1);
    assert_eq!(bytes, damaged);
    drop(journal);
    drop(home);
}
