use super::*;
use std::time::{Duration, Instant};

#[test]
fn application_bootstrap_commits_default_structure_before_installing_its_real_pty() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    crate::surface_meta::SurfaceMetaStore::set(
        &mut *memory.lock().unwrap(),
        17,
        "restore.command",
        "SHOULD-NOT-RUN",
    )
    .unwrap();
    crate::surface_meta::SurfaceMetaStore::set(
        &mut *memory.lock().unwrap(),
        crate::core::terminal_store::PTY_ID_BASE + 4,
        "restore.command",
        "PTY-SPACE",
    )
    .unwrap();
    let runners = Arc::new(crate::core::agent::runner_thread::RunnerRegistry::new());
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = "printf 'BOOTSTRAP-%s\\n' JOURNAL; exec sleep 60".into();
    let mut session = EngineSession::for_journal(
        80,
        24,
        Arc::new(|| {}),
        None,
        Some(1),
        memory,
        runners,
        settings,
    )
    .unwrap();
    assert!(session.core_state.local_workspaces.is_empty());
    assert_eq!(session.runtime.terminals.iter().count(), 0);
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal
        .begin_engine(
            &session,
            EngineSelection::Slot {
                slot: 1,
                resume: false,
            },
        )
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        journal.poll_bootstrap(&mut [&mut session]).unwrap();
        if session.journal_binding.is_some() && journal.is_ready(session.id) {
            break;
        }
        assert!(Instant::now() < deadline, "bootstrap stalled");
        std::thread::sleep(Duration::from_millis(2));
    }
    let binding = session.journal_binding.as_ref().unwrap();
    assert_eq!(binding.stream, "structure:slot-1");
    assert_eq!(binding.incarnation, 1);
    assert!(binding.journal_id.starts_with("journal-"));
    assert_eq!(session.core_state.local_workspaces.len(), 1);
    assert!(session.pending_materializations.is_empty());
    let sid = session.core_state.local_workspaces[0].all_surface_ids()[0];
    assert_eq!(
        sid, 18,
        "new durable IDs are above the existing numeric metadata scopes"
    );
    let terminal = session.runtime.terminals.get(sid).unwrap();
    while !terminal
        .with_content(|view| view.screen_text(false))
        .contains("BOOTSTRAP-JOURNAL")
    {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(2));
    }
    assert!(
        session
            .runtime
            .terminals
            .pty(sid)
            .unwrap()
            .process_id()
            .is_some()
    );
}

#[test]
fn failed_metadata_scope_read_rejects_bootstrap_instead_of_reserving_from_zero() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("memory.db");
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::MemoryStore::open(&path).unwrap(),
    ));
    let runners = Arc::new(crate::core::agent::runner_thread::RunnerRegistry::new());
    let session = EngineSession::for_journal(
        80,
        24,
        Arc::new(|| {}),
        None,
        Some(1),
        memory,
        runners,
        crate::settings::Settings::default(),
    )
    .unwrap();
    let connection = rusqlite::Connection::open(path).unwrap();
    connection.execute("DROP TABLE memory", []).unwrap();
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    let error = journal
        .begin_engine(
            &session,
            EngineSelection::Slot {
                slot: 1,
                resume: false,
            },
        )
        .unwrap_err();
    assert!(error.contains("cannot establish existing surface ID floor"));
    assert!(journal.opening.is_empty());
    assert!(session.core_state.local_workspaces.is_empty());
    assert_eq!(session.runtime.terminals.iter().count(), 0);
}
