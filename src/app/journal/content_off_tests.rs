//! Turning content restore off must stop earlier terminal history from coming back, including
//! history held by a lazy tab that was never activated while the setting was off.
use super::stall_budget::StallBudget;
use super::*;

const MARKER: &str = "MARKER-OLD";

/// Keeps this test's journal and slot files in its own temporary home, never the real one.
struct TempHome {
    _dir: tempfile::TempDir,
}
impl TempHome {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        tasty_utils::path::push_home_override(dir.path().to_path_buf());
        Self { _dir: dir }
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

/// Three terminal tabs whose immutable captures all hold `MARKER`. Tab 1 is selected.
fn import_marked_slot() -> Vec<u32> {
    let mut old = tasty_terminal::Terminal::new_detached(80, 24);
    old.process_bytes(MARKER.as_bytes());
    let history = History(tasty_terminal::disk_scrollback::serialize_lines(
        &old.screen_snapshot_lines(),
    ));
    let home = tasty_utils::path::tasty_home().unwrap();
    let directory = home.join("structure");
    std::fs::create_dir(&directory).unwrap();
    let journal_id = "journal-abcdef1234567890abcdef1234567890";
    std::fs::write(
        directory.join("journal.json"),
        serde_json::to_vec(
            &serde_json::json!({"version":1,"journal_id":journal_id,"phase":"Preparing"}),
        )
        .unwrap(),
    )
    .unwrap();
    let tabs:Vec<_> = (0..3).map(|index|serde_json::json!({"name":format!("terminal-{index}"),"explicit_name":null,"surface":{"Leaf":{"Terminal":{"cwd":home.to_string_lossy(),"restore_command":"printf 'RESTORE-%s\\n' READY; exec sleep 60","scrollback_ref":"saved"}}}})).collect();
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"restore","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":tabs,"active_tab":1}}}]});
    let mut store =
        tasty_event_store::EventStore::open(&directory.join("journal.db"), journal_id).unwrap();
    let epoch = store.acquire_writer().unwrap();
    crate::core::layout_persistence::import::import_slot(
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
        .collect()
}

/// One application run on slot 1 with the given content-restore setting. The journal opens the
/// test's home before the session installs its own isolated home for settings and metadata.
fn boot(restore_content: bool) -> (JournalApplication, EngineSession, StallBudget) {
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = String::new();
    settings.general.restore_layout = true;
    settings.general.restore_surface_content = restore_content;
    let mut session = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(1),
            memory: tasty_memory::writer::MemoryAccess::inline(Arc::new(std::sync::Mutex::new(
                tasty_memory::testing::InMemoryStorage::new(),
            ))),
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

/// Wait until the restore command ran, so the restored history (if any) is already injected.
fn restored_text(session: &EngineSession, stall: &mut StallBudget, surface: u32) -> String {
    let terminal = session.runtime.terminals.get(surface).unwrap();
    while !terminal
        .with_content(|view| view.screen_text(false))
        .contains("RESTORE-READY")
    {
        stall.nap("restore command output");
    }
    terminal.with_content(|view| view.screen_text(true))
}

fn activate(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    stall: &mut StallBudget,
    surface: u32,
) {
    assert!(journal.activate_restored_surface(session, surface).unwrap());
    loop {
        journal.poll_bootstrap(&mut [&mut *session], None).unwrap();
        if !journal.has_creation(session.id) {
            break;
        }
        stall.nap("lazy activation");
    }
}

fn is_placeholder(session: &EngineSession, surface: u32) -> bool {
    session.runtime.surfaces.get(&surface).is_some_and(|owner| {
        owner
            .as_any()
            .is::<crate::runtime::surface_restorer::JournalPlaceholder>()
    })
}

#[test]
fn content_off_does_not_restore_old_history_and_drops_it_from_lazy_tabs_too() {
    let home = TempHome::new();
    let ids = import_marked_slot();

    // A run with content restore off, as after the user turned it off: the selected tab must not
    // receive the history even though the journal still holds it.
    let (mut journal, mut session, mut stall) = boot(false);
    assert!(is_placeholder(&session, ids[0]) && is_placeholder(&session, ids[2]));
    let selected = restored_text(&session, &mut stall, ids[1]);
    assert!(
        !selected.contains(MARKER),
        "selected tab got old history: {selected}"
    );

    // The settings change marks the layout dirty; the next capture records the current sources.
    session.persistence.dirty.mark_dirty();
    journal.queue_surface_capture(&session, true);
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        if !journal.has_pending_capture(session.id) {
            break;
        }
        stall.nap("capture");
    }
    assert!(
        is_placeholder(&session, ids[0]) && is_placeholder(&session, ids[2]),
        "lazy tabs stay inactive while content restore is off"
    );
    drop(journal);
    drop(session);

    // Turn content restore back on. If any current source still held the old history it would
    // come back now, so its absence shows the journal no longer restores from those payloads.
    let (mut journal, mut session, mut stall) = boot(true);
    // History is injected before the terminal is published, so its text is final here. The live
    // capture had no restore.command metadata, so there is no restore output to wait for.
    let selected = session
        .runtime
        .terminals
        .get(ids[1])
        .unwrap()
        .with_content(|view| view.screen_text(true));
    assert!(
        !selected.contains(MARKER),
        "captured tab kept old history: {selected}"
    );
    activate(&mut journal, &mut session, &mut stall, ids[2]);
    // Waiting for the restore command also shows the stripped payload kept it.
    let lazy = restored_text(&session, &mut stall, ids[2]);
    assert!(
        !lazy.contains(MARKER),
        "lazy tab first activated after the change kept old history: {lazy}"
    );
    assert_eq!(
        session.core_state.local_workspaces()[0].all_surface_ids(),
        ids,
        "layout itself is restored"
    );
    drop(journal);
    drop(home);
}

#[test]
fn a_lazy_tab_activated_while_content_is_off_does_not_receive_old_history() {
    let home = TempHome::new();
    let ids = import_marked_slot();
    let (mut journal, mut session, mut stall) = boot(false);
    activate(&mut journal, &mut session, &mut stall, ids[2]);
    let lazy = restored_text(&session, &mut stall, ids[2]);
    assert!(!lazy.contains(MARKER), "lazy tab got old history: {lazy}");
    drop(journal);
    drop(home);
}
