//! An unreadable View checkpoint costs only the saved selection: the engine opens with every
//! workspace, the default selection is used, a warning is left, and the unreadable bytes stay
//! available after later View saves and restarts.
use super::stall_budget::StallBudget;
use super::*;
use crate::runtime::journal_product::view_record::StoredView;

fn poll_until(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    predicate: impl Fn(&JournalApplication, &EngineSession) -> bool,
) {
    let mut stall = StallBudget::new(journal);
    loop {
        journal.poll_bootstrap(&mut [session], None).unwrap();
        journal.poll_restore_bootstrap(session).unwrap();
        if predicate(journal, session) {
            break;
        }
        stall.nap("View checkpoint recovery");
    }
}

/// Builds the session first: in tests it moves this thread's home to a temporary directory.
fn new_session() -> EngineSession {
    let mut settings = crate::settings::Settings::default();
    settings.general.shell = "/bin/sh".into();
    settings.general.startup_command = String::new();
    EngineSession::new_with_ids_and_settings(
        crate::runtime::engine_session::EngineSessionSpec {
            cols: 80,
            rows: 24,
            waker: Arc::new(|| {}),
            shared_ids: None,
            layout_slot: Some(1),
            memory: Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().unwrap(),
            )),
            runner_registry: Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        },
        settings,
    )
    .unwrap()
}

fn open(session: &mut EngineSession) -> JournalApplication {
    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal
        .begin_engine(
            session,
            EngineSelection::Slot {
                slot: 1,
                resume: true,
            },
        )
        .unwrap();
    poll_until(&mut journal, session, |journal, session| {
        journal.is_ready(session.id)
    });
    journal
}

fn save_view(journal: &mut JournalApplication, session: &mut EngineSession, active: u32) {
    let binding = session.journal_binding.clone().unwrap();
    journal.queue_view(StoredView::capture(
        binding,
        &session.core_state,
        Some(active),
        &crate::model::StructurePresentationSnapshot::default(),
    ));
    poll_until(journal, session, |journal, _| {
        !journal.has_pending_view_writes()
    });
    assert!(!journal.view_writes.has_failures());
}

/// Two workspaces with the second selected, saved as a DB View manifest. Returns the session and
/// both workspace IDs.
fn saved_selection() -> (EngineSession, std::path::PathBuf, [u32; 2]) {
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    std::fs::create_dir(home.join("layouts")).unwrap();
    let workspace = |name: &str| serde_json::json!({"name":name,"subtitle":"","description":"","focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"t","explicit_name":null,"surface":{"Leaf":{"Generic":{"kind":"empty","data":{}}}}}],"active_tab":0}}});
    let layout = serde_json::json!({"version":2,"active_workspace":0,"workspaces":[workspace("first"),workspace("second")]});
    std::fs::write(home.join("layouts/01.json"), layout.to_string()).unwrap();
    let mut journal = open(&mut session);
    let ids: Vec<u32> = session
        .core_state
        .local_workspaces()
        .iter()
        .map(|workspace| workspace.id)
        .collect();
    save_view(&mut journal, &mut session, ids[1]);
    drop(journal);
    (session, home, [ids[0], ids[1]])
}

fn database(home: &std::path::Path) -> rusqlite::Connection {
    rusqlite::Connection::open(home.join("structure/journal.db")).unwrap()
}

/// Replace the manifest's View payload, keeping its checksum valid. Returns the new bytes.
fn rewrite_view(home: &std::path::Path, change: impl FnOnce(Vec<u8>) -> Vec<u8>) -> Vec<u8> {
    use sha2::Digest;
    let connection = database(home);
    let (payload, bytes): (i64, Vec<u8>) = connection
        .query_row(
            "SELECT p.payload_id, p.bytes FROM restore_manifests m JOIN payloads p
             ON p.payload_id = m.payload_id WHERE m.restore_key = 'view:slot-1'",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .unwrap();
    let bytes = change(bytes);
    connection
        .execute(
            "UPDATE payloads SET bytes = ?1, sha256 = ?2 WHERE payload_id = ?3",
            rusqlite::params![bytes, sha2::Sha256::digest(&bytes).to_vec(), payload],
        )
        .unwrap();
    bytes
}

fn current_view(home: &std::path::Path) -> Vec<u8> {
    database(home)
        .query_row(
            "SELECT p.bytes FROM restore_manifests m JOIN payloads p
             ON p.payload_id = m.payload_id WHERE m.restore_key = 'view:slot-1'",
            [],
            |row| row.get(0),
        )
        .unwrap()
}

fn preserved_views(home: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let directory = crate::runtime::journal_product::view_record::damaged_directory(home);
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut files: Vec<_> = entries
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "view"))
        .map(|path| {
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                std::fs::read(&path).unwrap(),
            )
        })
        .collect();
    files.sort();
    files
}

/// Opens the damaged checkpoint, checks the default selection and the warning, saves a new
/// selection and restarts. The unreadable bytes stay preserved throughout.
fn recovers_from(damage: impl FnOnce(&std::path::Path) -> Vec<u8>) {
    let (mut session, home, ids) = saved_selection();
    let damaged = damage(&home);

    let mut journal = open(&mut session);
    assert!(!journal.is_halted());
    let names: Vec<_> = session
        .core_state
        .local_workspaces()
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect();
    assert_eq!(names, vec!["first", "second"], "the structure is restored");
    let restored = &journal.restored_views[&session.id];
    assert_ne!(
        restored.active_workspace,
        Some(ids[1]),
        "the saved choice is not used"
    );
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].level,
        crate::notification::NotificationLevel::Warning
    );
    let preserved = preserved_views(&home);
    assert_eq!(preserved.len(), 1, "{preserved:?}");
    assert_eq!(
        preserved[0].1, damaged,
        "the unreadable bytes are kept as they were"
    );
    assert!(
        notices[0].body.contains(&preserved[0].0),
        "{}",
        notices[0].body
    );

    save_view(&mut journal, &mut session, ids[0]);
    assert_ne!(
        current_view(&home),
        damaged,
        "the next save replaces the manifest"
    );
    drop(journal);
    let journal = open(&mut session);
    assert_eq!(
        journal.restored_views[&session.id].active_workspace,
        Some(ids[0]),
        "the new selection is the source after a successful save"
    );
    assert_eq!(
        preserved_views(&home),
        preserved,
        "the preserved copy survives"
    );
}

#[test]
fn a_non_json_view_manifest_falls_back_to_the_default_selection() {
    recovers_from(|home| rewrite_view(home, |_| b"not json".to_vec()));
}

#[test]
fn an_unsupported_view_version_falls_back_to_the_default_selection() {
    recovers_from(|home| {
        rewrite_view(home, |bytes| {
            let mut view: serde_json::Value = serde_json::from_slice(&bytes).unwrap();
            view["version"] = 2.into();
            serde_json::to_vec(&view).unwrap()
        })
    });
}

#[test]
fn a_view_manifest_with_a_foreign_sequence_falls_back_to_the_default_selection() {
    recovers_from(|home| {
        database(home)
            .execute(
                "UPDATE restore_manifests SET sequence = sequence + 5 WHERE restore_key = 'view:slot-1'",
                [],
            )
            .unwrap();
        current_view(home)
    });
}

#[cfg(unix)]
#[test]
fn an_unreadable_legacy_sidecar_is_left_in_place() {
    use std::os::unix::fs::PermissionsExt;
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let mut journal = open(&mut session);
    let workspace = session.core_state.local_workspaces()[0].id;
    let binding = session.journal_binding.clone().unwrap();
    drop(journal);
    // The DB has no View manifest yet, so the sidecar is the source this start reads.
    let sidecar = home.join("structure/views/slot-1/incarnation-1.json");
    std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
    let original = serde_json::to_vec(&StoredView::capture(
        binding,
        &session.core_state,
        Some(workspace),
        &crate::model::StructurePresentationSnapshot::default(),
    ))
    .unwrap();
    std::fs::write(&sidecar, &original).unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&sidecar).is_ok() {
        // Running as root ignores file modes; the case cannot be produced here.
        std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o600)).unwrap();
        return;
    }

    journal = open(&mut session);
    assert!(!journal.is_halted());
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(
        notices[0].body.contains(&sidecar.display().to_string()),
        "{}",
        notices[0].body
    );
    save_view(&mut journal, &mut session, workspace);
    drop(journal);
    let journal = open(&mut session);
    assert!(
        journal.notices.is_empty(),
        "the DB manifest is the source now"
    );
    drop(journal);
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(
        std::fs::read(&sidecar).unwrap(),
        original,
        "the sidecar is unchanged"
    );
}

#[test]
fn a_view_payload_with_a_bad_checksum_falls_back_to_the_default_selection() {
    recovers_from(|home| {
        let bytes = b"bytes that no longer match the checksum".to_vec();
        database(home)
            .execute(
                "UPDATE payloads SET bytes = ?1 WHERE payload_id =
                 (SELECT payload_id FROM restore_manifests WHERE restore_key = 'view:slot-1')",
                [&bytes],
            )
            .unwrap();
        bytes
    });
}

#[test]
fn a_rejected_sidecar_of_another_version_does_not_block_later_saves() {
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let mut journal = open(&mut session);
    let workspace = session.core_state.local_workspaces()[0].id;
    let binding = session.journal_binding.clone().unwrap();
    drop(journal);
    let sidecar = home.join("structure/views/slot-1/incarnation-1.json");
    std::fs::create_dir_all(sidecar.parent().unwrap()).unwrap();
    let mut newer = serde_json::to_value(StoredView::capture(
        binding,
        &session.core_state,
        Some(workspace),
        &crate::model::StructurePresentationSnapshot::default(),
    ))
    .unwrap();
    newer["version"] = 2.into();
    newer["sequence"] = 1_000.into();
    let original = serde_json::to_vec(&newer).unwrap();
    std::fs::write(&sidecar, &original).unwrap();

    journal = open(&mut session);
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    save_view(&mut journal, &mut session, workspace);
    drop(journal);
    let journal = open(&mut session);
    assert!(
        journal.notices.is_empty(),
        "the DB manifest is the source now"
    );
    assert_eq!(
        journal.restored_views[&session.id].active_workspace,
        Some(workspace)
    );
    drop(journal);
    assert_eq!(
        std::fs::read(&sidecar).unwrap(),
        original,
        "the sidecar is unchanged"
    );
}

#[test]
fn a_view_manifest_without_its_snapshot_row_falls_back_to_the_default_selection() {
    recovers_from(|home| {
        let connection = database(home);
        connection
            .execute_batch(
                "PRAGMA foreign_keys = OFF;
                 DELETE FROM snapshots WHERE snapshot_id =
                 (SELECT snapshot_id FROM restore_manifests WHERE restore_key = 'view:slot-1');",
            )
            .unwrap();
        current_view(home)
    });
}
