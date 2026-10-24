//! A data home whose journal cannot be opened continues in a temporary journal: the first engine
//! gets a default workspace, the original files keep their bytes, and the cause is reported once.
use super::stall_budget::StallBudget;
use super::*;

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
            memory: tasty_memory::writer::MemoryAccess::inline(Arc::new(std::sync::Mutex::new(
                tasty_memory::MemoryStore::open_in_memory().unwrap(),
            ))),
            runner_registry: Arc::new(tasty_task_runtime::RunnerRegistry::new()),
        },
        settings,
    )
    .unwrap()
}

const SELECTION: EngineSelection = EngineSelection::Slot {
    slot: 1,
    resume: true,
};

/// Polls until the engine is ready, or returns the bootstrap failure.
fn bootstrap(journal: &mut JournalApplication, session: &mut EngineSession) -> Result<(), String> {
    let mut stall = StallBudget::new(journal);
    loop {
        journal.poll_bootstrap(&mut [&mut *session], None)?;
        journal.poll_restore_bootstrap(session)?;
        if journal.is_ready(session.id) {
            return Ok(());
        }
        stall.nap("journal bootstrap");
    }
}

fn digest(dir: &std::path::Path) -> Vec<(String, Vec<u8>)> {
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.is_file())
        .map(|path| {
            let bytes = std::fs::read(&path).unwrap_or_default();
            (
                path.file_name().unwrap().to_string_lossy().into_owned(),
                bytes,
            )
        })
        .collect();
    files.sort();
    files
}

#[test]
fn a_journal_that_is_not_a_database_continues_in_a_temporary_journal() {
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let structure = home.join("structure");
    // A first run publishes the journal identity; the database is then replaced with garbage.
    {
        session.core_state = crate::core::CoreState::new_base();
        let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
        journal.begin_engine(&session, SELECTION).unwrap();
        bootstrap(&mut journal, &mut session).unwrap();
    }
    std::fs::write(
        structure.join("journal.db"),
        b"this is not a sqlite database",
    )
    .unwrap();
    for side in ["journal.db-wal", "journal.db-shm"] {
        let path = structure.join(side);
        if path.exists() {
            std::fs::remove_file(path).unwrap();
        }
    }
    let before = digest(&structure);

    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal.begin_engine(&session, SELECTION).unwrap();
    let failure = bootstrap(&mut journal, &mut session).expect_err("the original cannot open");
    assert!(!journal.home_in_use());
    assert!(session.journal_binding.is_none());

    journal
        .fall_back_to_ephemeral(session.id, &failure)
        .unwrap();
    assert!(journal.is_ephemeral());
    journal.begin_engine(&session, SELECTION).unwrap();
    bootstrap(&mut journal, &mut session).unwrap();
    assert!(!journal.is_halted());
    assert_eq!(session.core_state.local_workspaces().len(), 1);

    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].level,
        crate::notification::NotificationLevel::Error
    );
    assert_eq!(notices[0].engine, Some(session.id));
    let popup = notices[0].popup.as_ref().expect("a visible notice");
    assert_eq!(popup.cause, notices::NoticeCause::Corrupt);
    let database = structure.join("journal.db");
    assert_eq!(popup.path, database.display().to_string());
    assert!(notices[0].body.contains(&failure), "{}", notices[0].body);
    assert!(journal.take_notices().is_empty(), "reported once");

    drop(journal);
    assert_eq!(
        digest(&structure),
        before,
        "the original files are untouched"
    );
}

#[cfg(unix)]
#[test]
fn an_unreadable_journal_is_reported_as_denied_and_read_again_once_allowed() {
    use std::os::unix::fs::PermissionsExt;
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let database = home.join("structure/journal.db");
    {
        session.core_state = crate::core::CoreState::new_base();
        let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
        journal.begin_engine(&session, SELECTION).unwrap();
        bootstrap(&mut journal, &mut session).unwrap();
        let workspace = session.core_state.local_workspaces()[0].id;
        assert!(workspace > 0);
    }
    let original = session.core_state.local_workspaces()[0].id;
    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::File::open(&database).is_ok() {
        // Running as root ignores file modes; the case cannot be produced here.
        return;
    }

    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal.begin_engine(&session, SELECTION).unwrap();
    let failure = bootstrap(&mut journal, &mut session).expect_err("the original cannot open");
    journal
        .fall_back_to_ephemeral(session.id, &failure)
        .unwrap();
    journal.begin_engine(&session, SELECTION).unwrap();
    bootstrap(&mut journal, &mut session).unwrap();
    let notices = journal.take_notices();
    assert_eq!(
        notices[0].popup.as_ref().unwrap().cause,
        notices::NoticeCause::ReadDenied
    );
    drop(journal);

    std::fs::set_permissions(&database, std::fs::Permissions::from_mode(0o644)).unwrap();
    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal.begin_engine(&session, SELECTION).unwrap();
    bootstrap(&mut journal, &mut session).unwrap();
    assert!(!journal.is_ephemeral());
    assert_eq!(
        session.core_state.local_workspaces()[0].id,
        original,
        "the original workspace is restored"
    );
}

#[test]
fn only_a_file_that_is_not_a_database_is_called_damaged() {
    let directory = tempfile::tempdir().unwrap();
    let garbage = directory.path().join("garbage.db");
    std::fs::write(&garbage, b"this is not a sqlite database").unwrap();
    assert_eq!(journal_read_cause(&garbage), notices::NoticeCause::Corrupt);
    let database = directory.path().join("real.db");
    rusqlite::Connection::open(&database)
        .unwrap()
        .execute_batch("CREATE TABLE t (x INTEGER);")
        .unwrap();
    assert_eq!(
        journal_read_cause(&database),
        notices::NoticeCause::OpenFailed,
        "a readable database that failed to open is not called damaged"
    );
    assert_eq!(
        journal_read_cause(&directory.path().join("missing.db")),
        notices::NoticeCause::OpenFailed
    );
}

#[cfg(unix)]
#[test]
fn a_read_only_journal_falls_back_without_being_called_damaged() {
    use std::os::unix::fs::PermissionsExt;
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let structure = home.join("structure");
    {
        session.core_state = crate::core::CoreState::new_base();
        let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
        journal.begin_engine(&session, SELECTION).unwrap();
        bootstrap(&mut journal, &mut session).unwrap();
    }
    let modes: Vec<_> = std::fs::read_dir(&structure)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    for path in &modes {
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o444)).unwrap();
    }
    std::fs::set_permissions(&structure, std::fs::Permissions::from_mode(0o555)).unwrap();
    let restore = || {
        std::fs::set_permissions(&structure, std::fs::Permissions::from_mode(0o755)).unwrap();
        for path in &modes {
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o644)).unwrap();
        }
    };
    if std::fs::write(structure.join("probe"), b"").is_ok() {
        // Running as root ignores file modes; the case cannot be produced here.
        restore();
        return;
    }

    session.core_state = crate::core::CoreState::new_base();
    session.journal_binding = None;
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal.begin_engine(&session, SELECTION).unwrap();
    let result = bootstrap(&mut journal, &mut session);
    let Err(failure) = result else {
        drop(journal);
        restore();
        panic!("a read-only journal was expected to refuse the writer");
    };
    journal
        .fall_back_to_ephemeral(session.id, &failure)
        .unwrap();
    let notices = journal.take_notices();
    drop(journal);
    restore();
    assert_eq!(
        notices[0].popup.as_ref().unwrap().cause,
        notices::NoticeCause::OpenFailed,
        "{failure}"
    );
}

#[test]
fn post_binding_boot_failure_retains_detached_pty_until_reaped_after_ephemeral_start() {
    let mut session = new_session();
    let mut journal = JournalApplication::new(Arc::new(|| {})).unwrap();
    journal.begin_engine(&session, SELECTION).unwrap();
    bootstrap(&mut journal, &mut session).unwrap();
    let original_binding = session.journal_binding.clone().unwrap();
    assert!(session.runtime.terminals.iter().next().is_some());
    let original = session.runtime.terminals.iter().next().unwrap().0;
    let generation = session
        .runtime
        .terminals
        .pty(original)
        .unwrap()
        .generation();
    let original_pid = session
        .runtime
        .terminals
        .pty(original)
        .unwrap()
        .process_id();
    journal.halted = Some("injected post-binding publication failure".into());
    journal
        .poll_boot_recovery(
            &mut session,
            None,
            "injected post-binding publication failure",
        )
        .unwrap();
    assert!(journal.is_ephemeral());
    assert!(session.journal_binding.is_none());
    assert!(session.runtime.terminals.iter().next().is_none());
    assert!(session.pending_materializations.is_empty());
    assert!(session.pending_resource_retirements.is_empty());
    bootstrap(&mut journal, &mut session).unwrap();
    assert_eq!(session.core_state.local_workspaces().len(), 1);
    let replacement = session.journal_binding.as_ref().unwrap();
    assert_ne!(original_binding.journal_id, replacement.journal_id);
    let new_surface = session.runtime.terminals.iter().next().unwrap().0;
    assert_ne!(
        session
            .runtime
            .terminals
            .pty(new_surface)
            .unwrap()
            .generation(),
        generation
    );
    let notice = journal.take_notices();
    assert_eq!(notice.len(), 1);
    assert_eq!(
        notice[0].popup.as_ref().unwrap().cause,
        notices::NoticeCause::SaveStopped
    );
    let mut stall = StallBudget::new(&journal);
    while journal.boot_release.is_some() {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        stall.nap("exact detached boot resource disposal");
    }
    #[cfg(target_os = "linux")]
    if let Some(pid) = original_pid {
        assert!(
            !std::path::Path::new(&format!("/proc/{pid}")).exists(),
            "old exact PTY process was reaped"
        );
    }
    #[cfg(not(target_os = "linux"))]
    let _original_pid = original_pid;
    assert!(!journal.is_halted());
}
