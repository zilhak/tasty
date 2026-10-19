//! A first import of an old slot file that cannot be read never fails opening the engine: the
//! slot starts in its initial state, the file is kept, and the loss is reported.
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
    let mut stall = StallBudget::new(&journal);
    loop {
        journal.poll_bootstrap(&mut [&mut *session], None).unwrap();
        journal.poll_restore_bootstrap(session).unwrap();
        if journal.is_ready(session.id) {
            return journal;
        }
        stall.nap("legacy import");
    }
}

fn names(session: &EngineSession) -> Vec<String> {
    session
        .core_state
        .local_workspaces()
        .iter()
        .map(|workspace| workspace.name.clone())
        .collect()
}

fn layout(scrollback_ref: Option<&str>) -> String {
    serde_json::json!({"version":2,"active_workspace":0,"workspaces":[{"name":"legacy","subtitle":"","description":"","focused_pane_index":0,"pane_layout":{"Leaf":{"tabs":[{"name":"t","explicit_name":null,"surface":{"Leaf":{"Terminal":{"cwd":null,"restore_command":null,"scrollback_ref":scrollback_ref}}}}],"active_tab":0}}}]}).to_string()
}

#[test]
fn an_unparsable_slot_file_starts_fresh_and_is_not_imported_later() {
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let file = home.join("layouts/01.json");
    std::fs::create_dir(home.join("layouts")).unwrap();
    std::fs::write(&file, "{ not a layout").unwrap();

    let journal = open(&mut session);
    assert!(!journal.is_halted());
    assert!(!names(&session).contains(&"legacy".to_owned()));
    drop(journal);
    // The initial state now has records, so a repaired file is not imported again.
    std::fs::write(&file, layout(None)).unwrap();
    let mut journal = open(&mut session);
    assert!(!names(&session).contains(&"legacy".to_owned()));
    assert!(journal.take_notices().is_empty());
    assert_eq!(std::fs::read_to_string(&file).unwrap(), layout(None));
}

#[cfg(unix)]
#[test]
fn an_unreadable_slot_file_reports_an_error_and_starts_fresh() {
    use std::os::unix::fs::PermissionsExt;
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    let file = home.join("layouts/01.json");
    std::fs::create_dir(home.join("layouts")).unwrap();
    std::fs::write(&file, layout(None)).unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o000)).unwrap();
    if std::fs::read(&file).is_ok() {
        // Running as root ignores file modes; the case cannot be produced here.
        return;
    }

    let mut journal = open(&mut session);
    assert!(!journal.is_halted());
    assert!(session.journal_binding.is_some());
    assert!(!names(&session).contains(&"legacy".to_owned()));
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].level,
        crate::notification::NotificationLevel::Error
    );
    let popup = notices[0].popup.as_ref().expect("a visible notice");
    assert_eq!(popup.cause, notices::NoticeCause::ReadDenied);
    assert_eq!(popup.path, file.display().to_string());
    assert!(
        notices[0]
            .body
            .contains(crate::i18n::t("recovery.legacy_not_reimported")),
        "{}",
        notices[0].body
    );
    drop(journal);
    let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o000, "the file's permissions are unchanged");
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(std::fs::read_to_string(&file).unwrap(), layout(None));
}

#[test]
fn missing_scrollback_restores_the_layout_with_a_warning() {
    let mut session = new_session();
    let home = tasty_utils::path::tasty_home().unwrap();
    std::fs::create_dir(home.join("layouts")).unwrap();
    std::fs::write(home.join("layouts/01.json"), layout(Some("gone"))).unwrap();

    let mut journal = open(&mut session);
    assert_eq!(names(&session), vec!["legacy".to_owned()]);
    let notices = journal.take_notices();
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(
        notices[0].level,
        crate::notification::NotificationLevel::Warning
    );
    assert!(notices[0].body.contains('1'), "{}", notices[0].body);
}
