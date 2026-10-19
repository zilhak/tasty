//! A terminal whose history alone exceeds the capture limit must be rejected before its history
//! is copied, and the rejection must not repeat until the layout or the history changes.
use super::super::stall_budget::StallBudget;
use super::*;

fn bootstrapped() -> (JournalApplication, EngineSession, u32) {
    let mut settings = crate::settings::Settings::default();
    super::super::test_shell::use_quiet_shell(&mut settings);
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
    while !(session.journal_binding.is_some() && journal.is_ready(session.id)) {
        journal.poll_bootstrap(&mut [&mut session], None).unwrap();
        stall.nap("bootstrap");
    }
    let sid = session.core_state.local_workspaces()[0].all_surface_ids()[0];
    (journal, session, sid)
}

/// Two history rows whose serialized form alone is larger than the queue byte limit.
fn oversized_history() -> Vec<tasty_terminal::ScrollbackLine> {
    let cell = "x".repeat(60_000);
    let attrs = termwiz::cell::CellAttributes::default();
    let row = tasty_terminal::ScrollbackLine::from_cells(
        std::iter::repeat_n((cell.as_str(), &attrs), 600),
        false,
    );
    vec![row.clone(), row]
}

/// Queue and submit one capture; returns the warnings it logged.
fn submit(journal: &mut JournalApplication, session: &mut EngineSession) -> String {
    #[derive(Clone)]
    struct Buf(Arc<std::sync::Mutex<Vec<u8>>>);
    impl std::io::Write for Buf {
        fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(b);
            Ok(b.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let buf = Buf(Arc::new(std::sync::Mutex::new(Vec::new())));
    let sink = buf.clone();
    let subscriber = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::WARN)
        .with_ansi(false)
        .with_writer(move || sink.clone())
        .finish();
    tracing::subscriber::with_default(subscriber, || {
        tracing::callsite::rebuild_interest_cache();
        journal.queue_surface_capture(session, true);
        journal.submit_captures(&mut [session]).unwrap();
    });
    String::from_utf8_lossy(&buf.0.lock().unwrap()).into_owned()
}

fn limit_warnings(logs: &str) -> usize {
    logs.matches("byte capture limit").count()
}

#[test]
fn oversized_history_is_rejected_once_per_layout_generation_and_retried_after_it_shrinks() {
    let (mut journal, mut session, sid) = bootstrapped();
    let terminal = session.runtime.terminals.get_mut(sid).unwrap();
    terminal.inject_scrollback(oversized_history());
    assert!(terminal.scrollback_serialized_len() > tasty_ipc::admission::QUEUED_BYTES_LIMIT as u64);

    let logs = submit(&mut journal, &mut session);
    assert_eq!(limit_warnings(&logs), 1, "warned once");
    assert!(
        logs.contains("terminal history of"),
        "rejected by the lower bound before copying the history: {logs}"
    );
    assert!(
        journal.captures.is_empty(),
        "oversized capture is not queued"
    );
    let first = journal
        .capture_rejections
        .get(&session.id)
        .expect("recorded");
    assert!(
        first
            .dirty_generation
            .ptr_eq(&session.persistence.dirty.generation())
    );

    assert_eq!(
        limit_warnings(&submit(&mut journal, &mut session)),
        0,
        "same generation is not retried"
    );
    assert!(journal.captures.is_empty());
    assert!(journal.capture_requests.is_empty(), "request is consumed");

    session.persistence.dirty.mark_dirty();
    assert_eq!(limit_warnings(&submit(&mut journal, &mut session)), 1);
    assert!(journal.captures.is_empty());
    let renewed = journal
        .capture_rejections
        .get(&session.id)
        .expect("recorded");
    assert!(
        renewed
            .dirty_generation
            .ptr_eq(&session.persistence.dirty.generation()),
        "a new layout generation is evaluated again"
    );

    // ED3 drops the history; the same generation is retried because the lower bound shrank.
    session
        .runtime
        .terminals
        .get_mut(sid)
        .unwrap()
        .feed_bytes(b"\x1b[3J");
    assert_eq!(limit_warnings(&submit(&mut journal, &mut session)), 0);
    assert_eq!(journal.captures.len(), 1, "capture is queued after ED3");
    assert!(!journal.capture_rejections.contains_key(&session.id));
}

fn warnings(journal: &mut JournalApplication) -> Vec<super::notices::RecoveryNotice> {
    let notices = journal.take_notices();
    assert!(
        notices
            .iter()
            .all(|notice| notice.level == crate::notification::NotificationLevel::Warning),
        "{notices:?}"
    );
    notices
}

#[test]
fn an_oversized_rejection_warns_at_once_and_not_again_for_the_same_cause() {
    let (mut journal, mut session, sid) = bootstrapped();
    session
        .runtime
        .terminals
        .get_mut(sid)
        .unwrap()
        .inject_scrollback(oversized_history());
    let workspace = session.core_state.local_workspaces()[0].name.clone();

    assert_eq!(limit_warnings(&submit(&mut journal, &mut session)), 1);
    let notices = warnings(&mut journal);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].engine, Some(session.id));
    assert!(
        notices[0].body.contains(&format!("\"{workspace}\"")),
        "names the workspace: {}",
        notices[0].body
    );
    assert!(
        notices[0].body.contains("64.0 MiB"),
        "names the limit: {}",
        notices[0].body
    );

    submit(&mut journal, &mut session);
    session.persistence.dirty.mark_dirty();
    assert_eq!(
        limit_warnings(&submit(&mut journal, &mut session)),
        1,
        "a new layout generation is rejected again"
    );
    assert!(
        warnings(&mut journal).is_empty(),
        "the same cause is not reported again"
    );
}

/// Answer a capture the worker never ran with `result`, as a worker failure would.
fn answer(
    journal: &mut JournalApplication,
    session: &mut EngineSession,
    ticket: u64,
    result: Result<ResultValue, String>,
) {
    journal.captures.insert(
        ticket,
        PendingCapture {
            engine: session.id,
            dirty_generation: session.persistence.dirty.generation(),
            work: None,
        },
    );
    assert!(journal.answer_capture(ticket, &result, &mut [session]));
}

/// Run one real capture through the worker so the engine has a successful save.
fn save(journal: &mut JournalApplication, session: &mut EngineSession) {
    journal.queue_surface_capture(session, true);
    let mut stall = StallBudget::new(journal);
    loop {
        journal.poll_bootstrap(&mut [&mut *session], None).unwrap();
        if !journal.has_pending_capture(session.id) {
            break;
        }
        stall.nap("capture");
    }
}

#[test]
fn repeated_capture_failures_warn_once_and_again_only_after_a_save() {
    let (mut journal, mut session, _) = bootstrapped();
    let threshold = u64::from(super::save_failures::REPEATED_FAILURES);
    for ticket in 1..threshold {
        answer(
            &mut journal,
            &mut session,
            u64::MAX - ticket,
            Err("disk full".into()),
        );
        assert!(
            warnings(&mut journal).is_empty(),
            "failure {ticket} is retried quietly"
        );
    }
    answer(
        &mut journal,
        &mut session,
        u64::MAX - threshold,
        Err("disk full".into()),
    );
    let notices = warnings(&mut journal);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert!(notices[0].body.contains("disk full"), "{}", notices[0].body);
    for ticket in threshold + 1..threshold + 5 {
        answer(
            &mut journal,
            &mut session,
            u64::MAX - ticket,
            Err("disk full".into()),
        );
    }
    assert!(
        warnings(&mut journal).is_empty(),
        "the same cause is not reported again"
    );

    save(&mut journal, &mut session);
    assert!(warnings(&mut journal).is_empty());
    for round in 0..threshold {
        answer(
            &mut journal,
            &mut session,
            u64::MAX - 100 - round,
            Err("busy".into()),
        );
        save(&mut journal, &mut session);
    }
    assert!(
        warnings(&mut journal).is_empty(),
        "a failure followed by a successful save is not reported"
    );
    for ticket in 0..threshold {
        answer(
            &mut journal,
            &mut session,
            u64::MAX - 200 - ticket,
            Err("busy".into()),
        );
    }
    assert_eq!(
        warnings(&mut journal).len(),
        1,
        "a new failure run is reported after a save"
    );
}

#[test]
fn view_write_failures_reach_the_engine_as_one_warning() {
    let (mut journal, mut session, _) = bootstrapped();
    let binding = session.journal_binding.clone().unwrap();
    let workspace = session.core_state.local_workspaces()[0].name.clone();
    let threshold = u64::from(super::save_failures::REPEATED_FAILURES);
    for ticket in 0..threshold + 2 {
        let view = crate::runtime::journal_product::view_record::StoredView::capture(
            binding.clone(),
            &session.core_state,
            None,
            &crate::model::StructurePresentationSnapshot::default(),
        );
        journal.view_writes.queue(view).unwrap();
        journal
            .view_writes
            .accept_for_test(&binding.stream, u64::MAX - ticket);
        journal
            .view_writes
            .complete(u64::MAX - ticket, &Err("disk I/O error".into()))
            .unwrap();
    }
    journal.poll_bootstrap(&mut [&mut session], None).unwrap();
    let notices = warnings(&mut journal);
    assert_eq!(notices.len(), 1, "{notices:?}");
    assert_eq!(notices[0].engine, Some(session.id));
    assert!(
        notices[0].body.contains(&format!("\"{workspace}\""))
            && notices[0].body.contains("disk I/O error"),
        "{}",
        notices[0].body
    );
}
