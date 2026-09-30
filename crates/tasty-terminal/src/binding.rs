//! Process-local connection identity. Independent of VT content epochs and durable journal IDs.
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

/// Identifies one physical PTY connection (or one detached terminal connection).
/// Adopt preserves it; replacing the connection allocates another value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ResourceGeneration(u64);

impl ResourceGeneration {
    pub(crate) fn fresh() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(1);
        Self(
            NEXT.fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_add(1))
                .expect("terminal resource generation space exhausted"),
        )
    }
}

/// Revokes callbacks and queued writes without retaining an OS handle or Terminal state.
#[derive(Clone)]
pub(crate) struct ConnectionLease {
    generation: ResourceGeneration,
    active: Arc<AtomicBool>,
}

impl ConnectionLease {
    pub(crate) fn new() -> Self {
        Self {
            generation: ResourceGeneration::fresh(),
            active: Arc::new(AtomicBool::new(true)),
        }
    }
    pub(crate) fn generation(&self) -> ResourceGeneration {
        self.generation
    }
    pub(crate) fn is_active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }
    pub(crate) fn revoke(&self) {
        self.active.store(false, Ordering::Release);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;
    use crate::pty::PtyOutput;
    use crate::{Pty, Terminal, TerminalConfig, TerminalIngest};
    use std::sync::{Mutex, mpsc};
    use std::time::Duration;

    fn pair() -> (Terminal, Pty) {
        crate::spawn_terminal(
            TerminalConfig {
                cols: 40,
                rows: 12,
                shell: Some("/bin/sh"),
                args: &["-c", "exec sleep 60"],
                surface_id: 0,
                working_dir: None,
                initial_input: None,
                extra_env: &[],
            },
            Arc::new(|| {}),
        )
        .expect("actual physical resource")
    }

    fn receiver(terminal: &Terminal) -> TerminalIngest {
        TerminalIngest {
            generation: terminal.resource_generation(),
            state: Arc::downgrade(&terminal.state),
            dirty: Arc::clone(&terminal.dirty),
            waker: Arc::clone(&terminal.waker),
        }
    }

    /// Retirement does not acquire the held content lock, and a late production callback is rejected.
    #[test]
    fn retirement_does_not_wait_for_content_lock_and_fences_a_late_reader() {
        let (mut old, pty) = pair();
        let generation = pty.generation();
        let output = old.add_output_tap();
        let (protocol_tx, protocol_rx) = mpsc::channel();
        old.set_input_sink(protocol_tx);
        let state = Arc::clone(&old.state);
        let callback = receiver(&old);
        let held = state.lock().unwrap();
        let (entered_tx, entered_rx) = mpsc::channel();
        let for_worker = Arc::clone(&state);
        let worker = std::thread::spawn(move || {
            assert!(matches!(
                for_worker.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            ));
            entered_tx.send(()).unwrap();
            callback.on_bytes(b"late-output\x1b[6n")
        });
        entered_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("reader contention ACK");
        // Must not acquire TerminalState's lock: child retirement and rendering cannot deadlock.
        drop(pty);
        assert!(!held.connection.is_active());
        drop(held);
        assert!(!worker.join().unwrap());
        assert!(old.screen_text(false).trim().is_empty());
        assert!(output.try_recv().is_err());
        assert!(protocol_rx.try_recv().is_err());

        let (mut replacement, _new_pty) = pair();
        assert_ne!(generation, replacement.resource_generation());
        assert!(!replacement.record_exit(generation));
        assert!(replacement.take_events().is_empty());
        let (tx, rx) = mpsc::channel();
        replacement.set_input_sink(tx);
        assert!(!replacement.send_response_for(generation, b"old clipboard reply"));
        assert!(rx.try_recv().is_err());
        replacement.process_bytes(b"\x1b[6n");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            b"\x1b[1;1R"
        );
    }

    #[test]
    fn adopt_wake_retargeting_preserves_the_physical_generation_and_content() {
        let (mut terminal, pty) = pair();
        let generation = pty.generation();
        let callback = receiver(&terminal);
        let wakes = Arc::new(Mutex::new(Vec::new()));
        let received = Arc::clone(&wakes);
        terminal.rewire_waker(Arc::new(move || received.lock().unwrap().push(19u32)));
        assert!(callback.on_bytes(b"same-resource"));
        assert_eq!(terminal.resource_generation(), generation);
        assert_eq!(terminal.screen_text(false).trim(), "same-resource");
        assert_eq!(
            *wakes.lock().unwrap(),
            vec![19, 19],
            "retarget wake and subsequent ingest use the new ID"
        );
        terminal.process_bytes(b"\x1b[3J\x1b[?1049h\x1b[?1049l");
        assert_eq!(
            terminal.resource_generation(),
            generation,
            "content epoch changes do not respawn the resource"
        );
    }
}
