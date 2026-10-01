use super::*;
use crate::runtime::journal_product::view_record::StoredView;
use std::time::{Duration, Instant};

fn poll_until(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    predicate: impl Fn(&JournalApplication, &EngineSession) -> bool,
) {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        journal.poll_bootstrap(&mut [session], None).unwrap();
        journal.poll_restore_bootstrap(session).unwrap();
        if predicate(journal, session) {
            break;
        }
        assert!(Instant::now() < deadline, "View checkpoint test stalled");
        std::thread::sleep(Duration::from_millis(1));
    }
}

#[test]
fn latest_view_supersedes_pending_tick_and_resume_ignores_legacy_positions() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::MemoryStore::open_in_memory().unwrap(),
    ));
    seed_legacy_scopes(&mut memory.lock().unwrap());
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = "exec sleep 60".into();
    let mut session = EngineSession::new_with_ids_and_settings(
        80,
        24,
        Arc::new(|| {}),
        None,
        Some(1),
        memory.clone(),
        runners,
        settings,
    )
    .unwrap();
    let home = tasty_utils::path::tasty_home().unwrap();
    std::fs::create_dir(home.join("layouts")).unwrap();
    let tabs:Vec<_>=(0..2).map(|i|serde_json::json!({"name":format!("tab-{i}"),"explicit_name":null,"surface":{"Leaf":{"Generic":{"kind":"empty","data":{}}}}})).collect();
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"View checkpoint","subtitle":"","description":"","focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":tabs,"active_tab":0}}}]});
    std::fs::write(home.join("layouts/01.json"), layout.to_string()).unwrap();
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
    poll_until(&mut journal, &mut session, |journal, session| {
        journal.is_ready(session.id)
    });
    assert_legacy_scopes(&memory.lock().unwrap(), false);
    let workspace = &session.core_state.local_workspaces()[0];
    let workspace_id = workspace.id;
    let pane = workspace.pane_layout().first_pane().unwrap();
    let pane_id = pane.id;
    let first = pane.tabs[0].id;
    let second = pane.tabs[1].id;
    let binding = session.journal_binding.clone().unwrap();
    let mut choice = crate::model::StructurePresentationSnapshot::default();
    choice.selected_tabs.insert(pane_id, first);
    journal.queue_view(StoredView::capture(
        binding.clone(),
        &session.core_state,
        Some(workspace_id),
        &choice,
    ));
    journal.submit_view_writes().unwrap();
    assert_eq!(
        journal.view_writes.len(),
        1,
        "the older tick is already in flight"
    );
    choice.selected_tabs.insert(pane_id, second);
    choice.collapsed_categories.insert(0);
    journal.queue_view(StoredView::capture(
        binding.clone(),
        &session.core_state,
        Some(workspace_id),
        &choice,
    ));
    let final_sequence = journal.latest_view_sequence();
    assert_eq!(final_sequence, 2);
    poll_until(&mut journal, &mut session, |journal, _| {
        !journal.has_pending_view_writes()
    });
    assert!(journal.failed_view_writes.is_empty());
    let store = tasty_event_store::EventStore::open(
        &home.join("structure/journal.db"),
        &binding.journal_id,
    )
    .unwrap();
    let bytes = store.restore_manifest("view:slot-1").unwrap().unwrap().view;
    drop(store);
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(saved["sequence"], final_sequence);
    assert_eq!(saved["binding"]["revision"], binding.revision.unwrap());
    assert_eq!(
        saved["binding"]["published_cut"],
        binding.published_cut.unwrap()
    );
    drop(journal);
    seed_legacy_scopes(&mut memory.lock().unwrap());
    std::fs::write(home.join("layouts/01.json"), "invalid legacy positions").unwrap();
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
    poll_until(&mut journal, &mut session, |journal, session| {
        journal.is_ready(session.id)
    });
    assert_legacy_scopes(&memory.lock().unwrap(), false);
    let restored = &journal.restored_views[&session.id];
    assert_eq!(restored.active_workspace, Some(workspace_id));
    assert_eq!(restored.selection.selected_tabs[&pane_id], second);
    assert!(restored.selection.collapsed_categories.contains(&0));
    drop(journal);
    seed_legacy_scopes(&mut memory.lock().unwrap());
    // A reset did not select old View data, including a newer/unreadable sidecar schema.
    std::fs::create_dir_all(home.join("structure/views/slot-1")).unwrap();
    std::fs::write(
        home.join("structure/views/slot-1/incarnation-1.json"),
        "{unreadable old View}",
    )
    .unwrap();
    // Reset only the test projection; retained runtime owners still belong to this session.
    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
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
    poll_until(&mut journal, &mut session, |journal, session| {
        journal.is_ready(session.id)
    });
    assert_legacy_scopes(&memory.lock().unwrap(), true);
    assert_eq!(
        session.journal_binding.as_ref().unwrap().incarnation,
        binding.incarnation + 1
    );
    assert!(
        !journal.restored_views[&session.id]
            .selection
            .collapsed_categories
            .contains(&0)
    );
    assert_eq!(
        std::fs::read_to_string(home.join("structure/views/slot-1/incarnation-1.json")).unwrap(),
        "{unreadable old View}"
    );
    let stale = StoredView::capture(binding, &session.core_state, None, &choice);
    journal
        .worker
        .submit(Request {
            ticket: u64::MAX,
            work: Work::SaveView(stale),
        })
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match journal.worker.try_recv() {
            Ok(Completion::Finished { ticket, result }) => {
                assert_eq!(ticket, u64::MAX);
                assert!(result.unwrap_err().contains("retired engine binding"));
                break;
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => {}
            other => panic!("unexpected stale save result: {other:?}"),
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert_eq!(
        std::fs::read_to_string(home.join("structure/views/slot-1/incarnation-1.json")).unwrap(),
        "{unreadable old View}"
    );
    let current = session.journal_binding.clone().unwrap();
    journal.queue_view(StoredView::capture(
        current.clone(),
        &session.core_state,
        None,
        &choice,
    ));
    poll_until(&mut journal, &mut session, |journal, _| {
        !journal.has_pending_view_writes()
    });
    assert!(journal.failed_view_writes.is_empty());
    let store = tasty_event_store::EventStore::open(
        &home.join("structure/journal.db"),
        &current.journal_id,
    )
    .unwrap();
    let manifest = store.restore_manifest("view:slot-1").unwrap().unwrap();
    assert_eq!(manifest.incarnation, current.incarnation);
    assert_eq!(manifest.sequence, journal.latest_view_sequence());
    let saved: serde_json::Value = serde_json::from_slice(&manifest.view).unwrap();
    assert_eq!(saved["binding"]["incarnation"], current.incarnation);
    assert_eq!(saved["sequence"], manifest.sequence);

    assert_eq!(
        std::fs::read_to_string(home.join("structure/views/slot-1/incarnation-1.json")).unwrap(),
        "{unreadable old View}"
    );
}

#[test]
fn failed_view_write_retains_the_checkpoint_and_marks_its_engine_dirty() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::testing::InMemoryStorage::new(),
    ));
    let runners = Arc::new(tasty_task_runtime::RunnerRegistry::new());
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = "exec sleep 60".into();
    let mut session = EngineSession::new_with_ids_and_settings(
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
    poll_until(&mut journal, &mut session, |journal, session| {
        journal.is_ready(session.id)
    });
    let home = tasty_utils::path::tasty_home().unwrap();
    let binding = session.journal_binding.clone().unwrap();
    let path = home.join("structure/views/slot-1/incarnation-1.json");
    // A fresh slot has no View manifest, so its initial legacy read still fails here.
    std::fs::create_dir_all(&path).unwrap();
    session.persistence.dirty.clear();
    let choice = crate::model::StructurePresentationSnapshot::default();
    journal.queue_view(StoredView::capture(
        binding.clone(),
        &session.core_state,
        None,
        &choice,
    ));
    poll_until(&mut journal, &mut session, |journal, _| {
        !journal.has_pending_view_writes()
    });
    assert!(journal.failed_view_writes.contains_key(&binding.stream));
    assert!(session.persistence.dirty.dirty_since().is_some());
    assert!(
        !journal.is_halted(),
        "failed View checkpoint does not roll back committed structure"
    );
    std::fs::remove_dir(&path).unwrap();
    journal.queue_view(StoredView::capture(
        binding.clone(),
        &session.core_state,
        None,
        &choice,
    ));
    poll_until(&mut journal, &mut session, |journal, _| {
        !journal.has_pending_view_writes()
    });
    assert!(journal.failed_view_writes.is_empty());
    let store = tasty_event_store::EventStore::open(
        &home.join("structure/journal.db"),
        &binding.journal_id,
    )
    .unwrap();
    let manifest = store.restore_manifest("view:slot-1").unwrap().unwrap();
    assert_eq!(manifest.incarnation, binding.incarnation);
    assert_eq!(manifest.sequence, journal.latest_view_sequence());
    let saved: serde_json::Value = serde_json::from_slice(&manifest.view).unwrap();
    assert_eq!(saved["sequence"], manifest.sequence);
    assert_eq!(saved["binding"]["runtime_epoch"], binding.runtime_epoch);
}

// Use the real SQLite store: the generic memory fake does not purge secret rows.
fn seed_legacy_scopes(memory: &mut tasty_memory::MemoryStore) {
    use tasty_memory::{MemoryValue, PutOpts, Scope};
    for id in [17, crate::runtime::terminal_store::PTY_ID_BASE + 4] {
        let scope = Scope::Surface(id);
        let value = MemoryValue::Text("legacy".into());
        memory
            .put(
                "com.test.legacy",
                &scope,
                "regular",
                &value,
                &PutOpts::default(),
            )
            .unwrap();
        memory
            .put_secret(
                "com.test.legacy",
                &scope,
                "secret",
                &value,
                &PutOpts::default(),
            )
            .unwrap();
    }
}

fn assert_legacy_scopes(memory: &tasty_memory::MemoryStore, invalid_present: bool) {
    use tasty_memory::Scope;
    for (id, present) in [
        (17, true),
        (
            crate::runtime::terminal_store::PTY_ID_BASE + 4,
            invalid_present,
        ),
    ] {
        let scope = Scope::Surface(id);
        assert_eq!(memory.get(&scope, "regular").unwrap().is_some(), present);
        assert_eq!(
            memory
                .get_secret("com.test.legacy", &scope, "secret")
                .unwrap()
                .is_some(),
            present
        );
    }
}

#[test]
fn failed_legacy_import_keeps_regular_and_secret_scopes() {
    let memory = Arc::new(std::sync::Mutex::new(
        tasty_memory::MemoryStore::open_in_memory().unwrap(),
    ));
    seed_legacy_scopes(&mut memory.lock().unwrap());
    let mut session = EngineSession::new_with_ids_and_settings(
        80,
        24,
        Arc::new(|| {}),
        None,
        Some(1),
        memory.clone(),
        Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        crate::settings::Settings::default(),
    )
    .unwrap();
    let home = tasty_utils::path::tasty_home().unwrap();
    std::fs::create_dir(home.join("layouts")).unwrap();
    std::fs::write(home.join("layouts/01.json"), "invalid legacy layout").unwrap();
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
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if journal.poll_bootstrap(&mut [&mut session], None).is_err() {
            break;
        }
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(1));
    }
    assert!(session.journal_binding.is_none());
    assert_legacy_scopes(&memory.lock().unwrap(), true);
}
