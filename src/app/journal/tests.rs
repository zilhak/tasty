use super::stall_budget::StallBudget;
use super::*;

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
        crate::runtime::terminal_store::PTY_ID_BASE + 4,
        "restore.command",
        "PTY-SPACE",
    )
    .unwrap();
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = "printf 'BOOTSTRAP-%s\\n' JOURNAL; exec sleep 60".into();
    let mut session = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(1),
            memory,
            runner_registry: runners,
        },
        settings,
    )
    .unwrap();
    assert!(session.core_state.local_workspaces().is_empty());
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
    let mut stall = StallBudget::new(&journal);
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        if session.journal_binding.is_some() && journal.is_ready(session.id) {
            break;
        }
        stall.nap("bootstrap");
    }
    let binding = session.journal_binding.as_ref().unwrap();
    assert_eq!(binding.stream, "structure:slot-1");
    assert_eq!(binding.incarnation, 1);
    assert!(binding.journal_id.starts_with("journal-"));
    assert_eq!(session.core_state.local_workspaces().len(), 1);
    assert!(session.pending_materializations.is_empty());
    let sid = session.core_state.local_workspaces()[0].all_surface_ids()[0];
    assert!(
        sid > 17 && sid < crate::runtime::terminal_store::PTY_ID_BASE,
        "new durable IDs exceed metadata scopes without entering the standalone PTY namespace: {sid}"
    );
    let terminal = session.runtime.terminals.get(sid).unwrap();
    while !terminal
        .with_content(|view| view.screen_text(false))
        .contains("BOOTSTRAP-JOURNAL")
    {
        stall.nap("bootstrap terminal output");
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
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
    let session = EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(1),
            memory,
            runner_registry: runners,
        },
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
    assert!(session.core_state.local_workspaces().is_empty());
    assert_eq!(session.runtime.terminals.iter().count(), 0);
}

/// 시작 복원이 끝나면 열린 슬롯과 다시 열 수 있는 슬롯 밖의 자식 관계를 지우고, 슬롯 표시가 없는
/// 이전 형식의 관계는 복원한 창의 것으로 남긴다.
#[cfg(feature = "gui")]
#[test]
fn resumed_bootstrap_forgets_child_relations_of_slots_that_cannot_reopen() {
    let session = EngineSession::new_with_ids_and_settings(
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
        crate::settings::Settings::default(),
    );
    let mut session = session.unwrap();
    let path = tasty_utils::path::tasty_home()
        .unwrap()
        .join("child-terminals.json");
    std::fs::write(
        &path,
        r#"{"children":{"7":[{"child_surface_id":8,"index":0}],
            "513":[{"child_surface_id":514,"index":0}],
            "1026":[{"child_surface_id":1028,"index":0}]},
            "parent_of":{"8":7,"514":513,"1028":1026},
            "next_index":{},"idle":{},"needs_input":{},
            "slot_of":{"513":1,"514":1,"1026":2,"1028":2}}"#,
    )
    .unwrap();
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
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
    while !(session.journal_binding.is_some() && journal.is_ready(session.id)) {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        stall.nap("bootstrap");
    }
    let on_disk = crate::runtime::child_terminal::ChildTerminalRegistry::load();
    let children = |parent| on_disk.list_children(parent).len();
    assert_eq!([children(7), children(513), children(1026)], [1, 1, 0]);
}
