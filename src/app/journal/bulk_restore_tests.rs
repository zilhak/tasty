use super::stall_budget::StallBudget;
use super::*;

struct NoScrollback;
impl crate::core::layout_persistence::import::ScrollbackSource for NoScrollback {
    fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
        Ok(None)
    }
}

#[test]
fn more_than_one_channel_capacity_of_captures_are_read_without_halting_bootstrap() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
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
        crate::settings::Settings::default(),
    )
    .unwrap();
    let home = tasty_utils::path::tasty_home().unwrap();
    let directory = home.join("structure");
    std::fs::create_dir(&directory).unwrap();
    let journal_id = "journal-1234567890abcdef1234567890abcdef";
    std::fs::write(
        directory.join("journal.json"),
        serde_json::to_vec(
            &serde_json::json!({"version":1,"journal_id":journal_id,"phase":"Preparing"}),
        )
        .unwrap(),
    )
    .unwrap();
    let expected = {
        let mut store =
            tasty_event_store::EventStore::open(&directory.join("journal.db"), journal_id).unwrap();
        let epoch = store.acquire_writer().unwrap();
        let tabs:Vec<_> = (0..100).map(|index|serde_json::json!({"name":format!("tab-{index}"),"explicit_name":null,"surface":{"Leaf":{"Generic":{"kind":"empty","data":{"index":index}}}}})).collect();
        let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"many","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":tabs,"active_tab":0}}}]});
        let imported = crate::core::layout_persistence::import::import_slot(
            &mut store,
            epoch,
            1,
            &layout.to_string(),
            &NoScrollback,
        )
        .unwrap();
        imported.mapping.workspaces[0].panes[0]
            .tabs
            .iter()
            .map(|tab| {
                let sid = tab.surfaces[0];
                let model = crate::runtime::journal::load(
                    &store,
                    &crate::runtime::journal::engine_stream(1),
                )
                .unwrap();
                (sid, model.surfaces[&sid].data.unwrap())
            })
            .collect::<std::collections::BTreeMap<_, _>>()
    };
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
    let mut turns = 0;
    loop {
        turns += 1;
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        assert!(journal.restorations.read_count() <= 8);
        if journal.restorations.ready(session.id).len() == 100 {
            break;
        }
        stall.nap("capture reads");
    }
    assert!(
        turns > 1,
        "the application regains control while the worker reads"
    );
    #[cfg(feature = "gui")]
    check_cancelled_reads(&mut journal, &session);
    let got = journal
        .restorations
        .ready(session.id)
        .iter()
        .map(|item| (item.surface_id, item.reference.unwrap()))
        .collect::<std::collections::BTreeMap<_, _>>();
    assert_eq!(got, expected);
    assert!(journal.restorations.read_count() == 0 && journal.restorations.queued_count() == 0);
    assert!(
        !journal.is_ready(session.id),
        "capture reads alone do not complete materialization"
    );
    assert_eq!(
        session.runtime.terminals.iter().count(),
        0,
        "logical bootstrap and payload reads never spawn a PTY"
    );
}

#[cfg(feature = "gui")]
fn check_cancelled_reads(journal: &mut JournalApplication, session: &EngineSession) {
    let other = EngineSession::new(80, 24, Arc::new(|| {})).unwrap();
    let input = journal.restorations.ready(session.id)[0].clone();
    for forget_before_reply in [false, true] {
        let mut inputs = restorations::Restorations::default();
        inputs.register(session.id);
        inputs.register(other.id);
        inputs.stage(session.id, input.clone());
        let mut ready = input.clone();
        ready.reference = None;
        inputs.stage(other.id, ready);
        let ticket = journal.next_ticket;
        inputs
            .submit(&journal.worker, &mut journal.next_ticket)
            .unwrap();
        inputs.retire(session.id);
        if forget_before_reply {
            inputs.forget(session.id);
        }
        assert!(
            inputs.has_reads(session.id),
            "accepted read retains its completion ticket"
        );
        let mut stall = StallBudget::new(journal);
        loop {
            match journal.worker.try_recv() {
                Ok(Completion::Finished {
                    ticket: done,
                    result,
                }) => {
                    assert_eq!(done, ticket);
                    let result = result.map_err(|error| error.to_string());
                    assert!(inputs.complete(done, &result, &[]).unwrap());
                    break;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    stall.nap("check cancelled reads");
                }
                other => panic!("unexpected read completion: {other:?}"),
            }
        }
        assert!(!inputs.has_reads(session.id));
        assert!(
            inputs.ready(session.id).is_empty(),
            "late payload must not revive a cancelled engine"
        );
        assert_eq!(inputs.ready(other.id).len(), 1);
        assert!(
            !inputs
                .complete(ticket, &Err("late duplicate".into()), &[])
                .unwrap()
        );
    }
}

#[test]
fn selected_terminal_restores_capture_while_other_tabs_remain_resource_free() {
    struct History(Vec<u8>);
    impl crate::core::layout_persistence::import::ScrollbackSource for History {
        fn read_bytes(&self, _: &str) -> std::io::Result<Option<Vec<u8>>> {
            Ok(Some(self.0.clone()))
        }
    }
    let mut old = tasty_terminal::Terminal::new_detached(80, 24);
    old.process_bytes(b"SAVED-IMMUTABLE-CONTENT");
    let history = History(tasty_terminal::disk_scrollback::serialize_lines(
        &old.screen_snapshot_lines(),
    ));
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = String::new();
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
    let ids = {
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
            .collect::<Vec<_>>()
    };
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
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        if journal.is_ready(session.id) {
            break;
        }
        stall.nap("selected restore");
    }
    assert_eq!(session.runtime.terminals.iter().count(), 1);
    let terminal = session.runtime.terminals.get(ids[1]).unwrap();
    while !terminal
        .with_content(|view| view.screen_text(false))
        .contains("RESTORE-READY")
    {
        stall.nap("restore command output");
    }
    assert!(
        terminal
            .with_content(|view| view.screen_text(true))
            .contains("SAVED-IMMUTABLE-CONTENT")
    );
    assert_eq!(
        session.runtime.terminals.scrollback_persist_id(ids[1]),
        Some("saved")
    );
    // The OS reports the resolved cwd; macOS temp dirs sit behind /var -> /private/var.
    assert_eq!(
        session.runtime.terminals.cwd(ids[1]),
        Some(home.canonicalize().unwrap())
    );
    for sid in [ids[0], ids[2]] {
        assert!(session.runtime.terminals.get(sid).is_none());
        assert!(
            session
                .runtime
                .surfaces
                .get(&sid)
                .unwrap()
                .as_any()
                .is::<crate::runtime::surface_restorer::JournalPlaceholder>()
        );
    }
    assert_eq!(journal.restorations.ready(session.id).len(), 2);
    assert!(journal.activate_restored_surface(&session, ids[2]).unwrap());
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        if !journal.has_creation(session.id) {
            break;
        }
        stall.nap("later selection");
    }
    assert_eq!(session.runtime.terminals.iter().count(), 2);
    assert!(session.runtime.terminals.get(ids[0]).is_none());
    assert_eq!(
        session.core_state.local_workspaces()[0].all_surface_ids(),
        ids
    );
}

#[test]
fn large_generic_capture_waits_for_registration_and_reaches_restore_factory_unchanged() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
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
        crate::settings::Settings::default(),
    )
    .unwrap();
    let home = tasty_utils::path::tasty_home().unwrap();
    let directory = home.join("structure");
    std::fs::create_dir(&directory).unwrap();
    let journal_id = "journal-0123456789abcdef0123456789abcdef";
    std::fs::write(
        directory.join("journal.json"),
        serde_json::to_vec(
            &serde_json::json!({"version":1,"journal_id":journal_id,"phase":"Preparing"}),
        )
        .unwrap(),
    )
    .unwrap();
    let data = serde_json::json!({"text":"large capture value\n".repeat(70_000),"tail":731});
    assert!(serde_json::to_vec(&data).unwrap().len() > 1024 * 1024);
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"generic","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"large","explicit_name":null,"surface":{"Leaf":{"Generic":{"kind":"late-restore","data":data}}}}],"active_tab":0}}}]});
    let sid = {
        let mut store =
            tasty_event_store::EventStore::open(&directory.join("journal.db"), journal_id).unwrap();
        let epoch = store.acquire_writer().unwrap();
        crate::core::layout_persistence::import::import_slot(
            &mut store,
            epoch,
            1,
            &layout.to_string(),
            &NoScrollback,
        )
        .unwrap()
        .mapping
        .workspaces[0]
            .panes[0]
            .tabs[0]
            .surfaces[0]
    };
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
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        if journal.is_ready(session.id) {
            break;
        }
        stall.nap("generic capture bootstrap");
    }
    assert_eq!(journal.restorations.ready(session.id).len(), 1);
    assert!(
        journal.restorations.ready(session.id)[0]
            .input
            .restore
            .is_none()
    );
    assert!(
        session
            .runtime
            .surfaces
            .get(&sid)
            .unwrap()
            .as_any()
            .is::<crate::runtime::surface_restorer::JournalPlaceholder>()
    );
    let (sender, receiver) = std::sync::mpsc::channel();
    let declaration=serde_json::from_value(serde_json::json!({"kind":"late-restore","display_name_i18n_key":"surface.kind.markdown","rendering":"remote"})).unwrap();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &session.runtime.surface_registry,
        "com.test.late-restore",
        &declaration,
        sender,
    );
    loop {
        journal.poll_restore_bootstrap(&session).unwrap();
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        if journal.is_ready(session.id) && journal.restorations.ready(session.id).is_empty() {
            break;
        }
        stall.nap("late registration restore");
    }
    match receiver.try_recv().unwrap() {
        crate::plugin_bridge::host_cmd::HostCmd::RemoteSurfaceRestored {
            surface_id,
            data: actual,
            ..
        } => {
            assert_eq!(surface_id, sid);
            assert_eq!(actual, data);
        }
        _ => panic!("factory created instead of restoring the committed capture"),
    }
    assert_eq!(
        session.core_state.find_surface_by_id(sid).unwrap().kind(),
        "late-restore"
    );
    assert!(
        receiver.try_recv().is_err(),
        "one claim publishes exactly one restore request"
    );
    assert_eq!(session.runtime.terminals.iter().count(), 0);
}

#[test]
fn product_slot_import_preserves_null_restore_and_does_not_reimport_modified_legacy_file() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    crate::surface_meta::SurfaceMetaStore::set(
        &mut *memory.lock().unwrap(),
        17,
        "restore.command",
        "old unrelated scope",
    )
    .unwrap();
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
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
        crate::settings::Settings::default(),
    )
    .unwrap();
    let home = tasty_utils::path::tasty_home().unwrap();
    std::fs::create_dir(home.join("layouts")).unwrap();
    let slot_path = home.join("layouts/01.json");
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"imported","subtitle":"","description":"","category":0,"focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"null snapshot","explicit_name":null,"surface":{"Leaf":{"Generic":{"kind":"import-null","data":null}}}}],"active_tab":0}}}]});
    std::fs::write(&slot_path, layout.to_string()).unwrap();
    let (sender, receiver) = std::sync::mpsc::channel();
    let declaration=serde_json::from_value(serde_json::json!({"kind":"import-null","display_name_i18n_key":"surface.kind.markdown","rendering":"remote"})).unwrap();
    crate::plugin_bridge::remote_kind::register_remote_kind(
        &session.runtime.surface_registry,
        "com.test.import-null",
        &declaration,
        sender,
    );
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
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        if journal.is_ready(session.id) {
            break;
        }
        stall.nap("product slot import");
    }
    let ids = session.core_state.local_workspaces()[0].all_surface_ids();
    assert_eq!(
        ids,
        vec![18],
        "legacy import reserves above numeric metadata scopes"
    );
    assert_eq!(session.core_state.local_workspaces()[0].name, "imported");
    match receiver.try_recv().unwrap() {
        crate::plugin_bridge::host_cmd::HostCmd::RemoteSurfaceRestored {
            surface_id, data, ..
        } => {
            assert_eq!(surface_id, ids[0]);
            assert!(data.is_null());
        }
        _ => panic!("JSON null was treated as create rather than a restore snapshot"),
    }
    crate::surface_meta::SurfaceMetaStore::set(
        &mut *session.runtime.memory.lock().unwrap(),
        ids[0],
        "restore.command",
        "keep live metadata",
    )
    .unwrap();
    let binding = session.journal_binding.clone().unwrap();
    drop(journal);
    // A later legacy export is not another import request or a source of positional identities.
    std::fs::write(&slot_path, "{broken legacy after successful import").unwrap();
    // Reset only the test projection; retained runtime owners still belong to this session.
    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
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
    stall.watch(&journal);
    loop {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        journal.poll_restore_bootstrap(&session).unwrap();
        if journal.is_ready(session.id) {
            break;
        }
        stall.nap("journal resume");
    }
    assert_eq!(
        session.core_state.local_workspaces()[0].all_surface_ids(),
        ids
    );
    assert_eq!(
        crate::surface_meta::SurfaceMetaStore::get(
            &mut *session.runtime.memory.lock().unwrap(),
            ids[0],
            "restore.command"
        )
        .as_deref(),
        Some("keep live metadata")
    );
    assert_eq!(
        session.journal_binding.as_ref().unwrap().incarnation,
        binding.incarnation
    );
    assert_eq!(
        session.journal_binding.as_ref().unwrap().stream,
        binding.stream
    );
    assert!(session.journal_binding.as_ref().unwrap().runtime_epoch > binding.runtime_epoch);
    assert_eq!(
        std::fs::read_to_string(slot_path).unwrap(),
        "{broken legacy after successful import"
    );
}
