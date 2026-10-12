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
