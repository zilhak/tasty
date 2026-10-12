//! Live body-ack → settle → CR continuations owned by the original engine/resource.
//! No worker thread, durable input body or SID-only reinjection is involved.
use super::engine_access::EngineMut;
use std::time::{Duration, Instant};
use tasty_terminal::{ResourceGeneration, WriteAck};
pub(crate) const MAX_PENDING: usize = 64;
pub(crate) struct PendingSubmit {
    surface: u32,
    generation: ResourceGeneration,
    ack: WriteAck,
    started: Instant,
    settle: Option<Instant>,
}
impl PendingSubmit {
    pub(crate) fn new(surface: u32, generation: ResourceGeneration, ack: WriteAck) -> Self {
        Self {
            surface,
            generation,
            ack,
            started: Instant::now(),
            settle: None,
        }
    }
    /// 제한 시간 경계를 시험하려고 시작 시각만 지정한다. 제품 경로의 시간 소스는 `new`다.
    #[cfg(test)]
    pub(crate) fn started_at(
        surface: u32,
        generation: ResourceGeneration,
        ack: WriteAck,
        started: Instant,
    ) -> Self {
        Self {
            started,
            ..Self::new(surface, generation, ack)
        }
    }
    pub(crate) fn surface(&self) -> u32 {
        self.surface
    }
    fn next(&self) -> Instant {
        self.settle.unwrap_or_else(|| {
            (Instant::now() + Duration::from_millis(10)).min(self.started + Duration::from_secs(5))
        })
    }
}
impl super::engine_runtime::EngineRuntime {
    pub(crate) fn input_submit_deadline(&self) -> Option<Instant> {
        self.pending_submits.iter().map(PendingSubmit::next).min()
    }
}
impl EngineMut<'_> {
    pub(crate) fn poll_input_submissions(&mut self) {
        self.poll_input_submissions_at(Instant::now());
    }
    /// `now`를 받아 ack 제한 시간과 settle 경계를 시험할 수 있게 한다.
    fn poll_input_submissions_at(&mut self, now: Instant) {
        let mut pending = std::mem::take(&mut self.runtime.pending_submits);
        for mut submit in pending.drain(..) {
            if !self
                .runtime
                .terminals
                .matches_generation(submit.surface, submit.generation)
                || self.live.occupancy.is_hard_occupied(submit.surface)
            {
                continue;
            }
            if submit.settle.is_none()
                && (submit.ack.is_complete()
                    || now.saturating_duration_since(submit.started) >= Duration::from_secs(5))
            {
                submit.settle = Some(now + Duration::from_millis(20));
            }
            if submit.settle.is_some_and(|deadline| now >= deadline) {
                if let Some(terminal) = self.runtime.terminals.get_mut(submit.surface)
                    && terminal.try_send_key_with_ack("\r").is_err()
                {
                    tracing::warn!(
                        surface = submit.surface,
                        "terminal submission queue disconnected"
                    );
                }
            } else {
                self.runtime.pending_submits.push(submit);
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    const SURFACE: u32 = 1;

    fn engine() -> crate::runtime::engine_session::EngineSession {
        use tasty_core::{DomainEvent as E, SurfaceSpec};
        let events = vec![
            E::CategoryCreated {
                id: 0,
                name: "normal".into(),
                index: 0,
            },
            E::WorkspaceCreated {
                id: 1,
                name: "workspace".into(),
                category: 0,
                index: 0,
                pane: 1,
            },
            E::TabCreated {
                id: SURFACE,
                pane: 1,
                index: 0,
                name: "terminal".into(),
                surface: SurfaceSpec {
                    id: SURFACE,
                    kind: "terminal".into(),
                    data: None,
                },
            },
        ];
        crate::state::tests::test_state_from_model(crate::state::tests::test_model(events)).1
    }

    /// 본문을 보내고 그 ack로 대기 항목 하나를 만든다. sink가 있으면 ack는 완료되지 않는다.
    fn enqueue_body(
        e: &mut EngineMut<'_>,
        sink: Option<mpsc::Sender<Vec<u8>>>,
        started: Instant,
    ) -> ResourceGeneration {
        let terminal = e.runtime.terminals.get_mut(SURFACE).expect("terminal");
        if let Some(sink) = sink {
            terminal.set_input_sink(sink);
        }
        let ack = terminal.send_key_with_ack("hi");
        let generation = e.runtime.terminals.generation(SURFACE).expect("generation");
        e.runtime
            .pending_submits
            .push(PendingSubmit::started_at(SURFACE, generation, ack, started));
        generation
    }

    fn received(rx: &mpsc::Receiver<Vec<u8>>) -> Vec<Vec<u8>> {
        rx.try_iter().collect()
    }

    #[test]
    fn a_replaced_generation_drops_the_submit_without_a_cr() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        let t0 = Instant::now();
        let (old_tx, old_rx) = mpsc::channel();
        enqueue_body(&mut e, Some(old_tx), t0);
        let (new_tx, new_rx) = mpsc::channel();
        e.runtime
            .terminals
            .get_mut(SURFACE)
            .expect("terminal")
            .replace_external_connection(new_tx);

        e.poll_input_submissions_at(t0 + Duration::from_secs(6));

        assert!(e.runtime.pending_submits.is_empty());
        assert_eq!(received(&old_rx), vec![b"hi".to_vec()]);
        assert!(
            received(&new_rx).is_empty(),
            "교체된 연결에 CR을 보내면 안 된다"
        );
    }

    #[test]
    fn a_hard_occupancy_drops_the_submit_without_a_cr() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        let t0 = Instant::now();
        let (tx, rx) = mpsc::channel();
        enqueue_body(&mut e, Some(tx), t0);
        e.live.occupancy.acquire(SURFACE, 1).expect("hard occupy");

        e.poll_input_submissions_at(t0 + Duration::from_secs(6));

        assert!(e.runtime.pending_submits.is_empty());
        assert_eq!(received(&rx), vec![b"hi".to_vec()]);
    }

    #[test]
    fn an_unacked_body_waits_until_five_seconds() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        let t0 = Instant::now();
        let (tx, rx) = mpsc::channel();
        enqueue_body(&mut e, Some(tx), t0);

        e.poll_input_submissions_at(t0 + Duration::from_millis(4_999));

        assert_eq!(e.runtime.pending_submits.len(), 1);
        assert!(e.runtime.pending_submits[0].settle.is_none());
        assert_eq!(received(&rx), vec![b"hi".to_vec()]);
    }

    #[test]
    fn an_unacked_body_settles_at_five_seconds_then_sends_cr_after_twenty_ms() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        let t0 = Instant::now();
        let (tx, rx) = mpsc::channel();
        enqueue_body(&mut e, Some(tx), t0);
        let timeout = t0 + Duration::from_secs(5);

        e.poll_input_submissions_at(timeout);
        assert_eq!(e.runtime.pending_submits.len(), 1);
        assert_eq!(
            e.runtime.pending_submits[0].settle,
            Some(timeout + Duration::from_millis(20))
        );
        assert_eq!(received(&rx), vec![b"hi".to_vec()]);

        e.poll_input_submissions_at(timeout + Duration::from_millis(19));
        assert_eq!(e.runtime.pending_submits.len(), 1);
        assert!(received(&rx).is_empty());

        e.poll_input_submissions_at(timeout + Duration::from_millis(20));
        assert!(e.runtime.pending_submits.is_empty());
        assert_eq!(received(&rx), vec![b"\r".to_vec()]);
    }

    // sink 없이 보낸 본문은 대상 write 수가 0이라 ack가 바로 완료된다. CR 관측용 sink는 그 뒤에 연결한다.
    #[test]
    fn an_acked_body_sends_cr_twenty_ms_after_the_ack_is_seen() {
        let mut session = engine();
        let mut e = session.borrow_mut();
        let t0 = Instant::now();
        let generation = enqueue_body(&mut e, None, t0);
        assert!(e.runtime.pending_submits[0].ack.is_complete());
        let (tx, rx) = mpsc::channel();
        e.runtime
            .terminals
            .get_mut(SURFACE)
            .expect("terminal")
            .set_input_sink(tx);
        assert!(e.runtime.terminals.matches_generation(SURFACE, generation));
        let seen = t0 + Duration::from_millis(1);

        e.poll_input_submissions_at(seen);
        assert_eq!(
            e.runtime.pending_submits[0].settle,
            Some(seen + Duration::from_millis(20))
        );
        assert!(received(&rx).is_empty());

        e.poll_input_submissions_at(seen + Duration::from_millis(20));
        assert!(e.runtime.pending_submits.is_empty());
        assert_eq!(received(&rx), vec![b"\r".to_vec()]);
    }
}
