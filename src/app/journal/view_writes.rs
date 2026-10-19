//! View checkpoints coalesce queued values without replacing an already accepted write.
use super::*;
use crate::runtime::journal_product::view_record::StoredView;
use std::collections::HashSet;

#[derive(Default)]
pub(super) struct ViewWrites {
    queued: HashMap<String, StoredView>,
    inflight: HashMap<u64, StoredView>,
    failed: HashMap<String, (StoredView, String)>,
    cancelled: HashSet<u64>,
    sequence: u64,
    /// stream별 연속 쓰기 실패. 기준에 이른 실패는 `alerts`에 남겨 App 알림으로 옮긴다.
    streaks: super::save_failures::FailureStreaks<String>,
    alerts: Vec<(String, u32, String)>,
}
impl ViewWrites {
    pub(super) fn queue(&mut self, mut view: StoredView) -> Result<(), String> {
        let sequence = self
            .sequence
            .checked_add(1)
            .ok_or("View checkpoint sequence exhausted")?;
        self.sequence = sequence;
        view.sequence = sequence;
        self.failed.remove(&view.binding.stream);
        self.queued.insert(view.binding.stream.clone(), view);
        Ok(())
    }
    pub(super) fn sequence(&self) -> u64 {
        self.sequence
    }
    pub(super) fn failed_for(&self, stream: &str) -> bool {
        self.failed.contains_key(stream)
    }
    pub(super) fn pending_for(&self, stream: &str) -> bool {
        self.queued.contains_key(stream)
            || self
                .inflight
                .values()
                .any(|view| view.binding.stream == stream)
    }
    pub(super) fn pending(&self) -> bool {
        !self.queued.is_empty() || !self.inflight.is_empty()
    }
    pub(super) fn cancel(&mut self, stream: &str) {
        self.queued.remove(stream);
        self.failed.remove(stream);
        self.streaks.succeeded(&stream.to_owned());
        self.alerts.retain(|(failed, _, _)| failed != stream);
        self.cancelled.extend(
            self.inflight
                .iter()
                .filter_map(|(ticket, view)| (view.binding.stream == stream).then_some(*ticket)),
        );
    }
    pub(super) fn submit(
        &mut self,
        worker: &JournalWorker,
        next_ticket: &mut u64,
    ) -> Result<(), String> {
        let ready: Vec<_> = self
            .queued
            .keys()
            .filter(|stream| {
                !self
                    .inflight
                    .values()
                    .any(|view| view.binding.stream == **stream)
            })
            .take(8)
            .cloned()
            .collect();
        for stream in ready {
            let ticket = *next_ticket;
            let next = ticket
                .checked_add(1)
                .ok_or("journal ticket range exhausted")?;
            let view = self.queued.remove(&stream).expect("listed View write");
            match worker.submit(Request {
                ticket,
                work: Work::SaveView(view.clone()),
            }) {
                Ok(()) => {
                    *next_ticket = next;
                    self.inflight.insert(ticket, view);
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => {
                    self.queued.insert(stream, view);
                    break;
                }
                Err(error) => {
                    self.queued.insert(stream, view);
                    return Err(format!("View snapshot submission: {error:?}"));
                }
            }
        }
        Ok(())
    }
    pub(super) fn complete(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
    ) -> Result<bool, String> {
        let Some(view) = self.inflight.remove(&ticket) else {
            return Ok(false);
        };
        if self.cancelled.remove(&ticket) {
            return Ok(true);
        }
        match result {
            Ok(ResultValue::ViewSaved) => {
                self.streaks.succeeded(&view.binding.stream);
                if self
                    .failed
                    .get(&view.binding.stream)
                    .is_some_and(|(failed, _)| failed.sequence <= view.sequence)
                {
                    self.failed.remove(&view.binding.stream);
                }
            }
            // 손상 범위가 남아 있는 동안 선택은 저장하지 않는다. 다시 보내도 같은 응답이므로 실패로 두지 않는다.
            Ok(ResultValue::ViewDeferred) => {}
            Err(error) => {
                tracing::warn!("View snapshot write failed: {error}");
                if let Some(failures) = self.streaks.failed(view.binding.stream.clone()) {
                    self.alerts
                        .push((view.binding.stream.clone(), failures, error.clone()));
                }
                // A later value already queued is the retry; the older failure must not outlive it.
                if self
                    .queued
                    .get(&view.binding.stream)
                    .is_none_or(|queued| queued.sequence <= view.sequence)
                {
                    self.failed
                        .insert(view.binding.stream.clone(), (view, error.clone()));
                }
            }
            _ => return Err("View snapshot returned another completion".into()),
        }
        Ok(true)
    }
    /// 기준에 이른 쓰기 실패: (stream, 연속 실패 횟수, 마지막 원인).
    pub(super) fn take_alerts(&mut self) -> Vec<(String, u32, String)> {
        std::mem::take(&mut self.alerts)
    }
    /// 큐에 있는 값을 worker에 보낸 것처럼 진행 중으로 옮긴다.
    #[cfg(test)]
    pub(super) fn accept_for_test(&mut self, stream: &str, ticket: u64) {
        let view = self.queued.remove(stream).expect("queued View write");
        self.inflight.insert(ticket, view);
    }
    #[cfg(test)]
    pub(super) fn inflight_count(&self) -> usize {
        self.inflight.len()
    }
    #[cfg(test)]
    pub(super) fn has_failures(&self) -> bool {
        !self.failed.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn view(incarnation: u64) -> StoredView {
        StoredView::capture(
            crate::runtime::journal_product::EngineBinding {
                journal_id: "journal-test".into(),
                stream: "structure:slot-1".into(),
                incarnation,
                runtime_epoch: 1,
                published_cut: None,
                revision: None,
            },
            &crate::core::CoreState::new_base(),
            None,
            &crate::model::StructurePresentationSnapshot::default(),
        )
    }
    fn accept(owner: &mut ViewWrites, ticket: u64) {
        owner.accept_for_test("structure:slot-1", ticket);
    }
    /// 실패한 값의 재시도는 다음 저장 주기에 다시 큐에 들어온 값이다.
    fn fail_once(writes: &mut ViewWrites, ticket: u64) {
        writes.queue(view(1)).unwrap();
        accept(writes, ticket);
        writes
            .complete(ticket, &Err("disk I/O error".into()))
            .unwrap();
    }
    #[test]
    fn repeated_write_failures_raise_one_alert_until_a_save_succeeds() {
        let mut writes = ViewWrites::default();
        let threshold = u64::from(super::super::save_failures::REPEATED_FAILURES);
        for ticket in 1..threshold {
            fail_once(&mut writes, ticket);
            assert!(
                writes.take_alerts().is_empty(),
                "failure {ticket} is retried quietly"
            );
        }
        fail_once(&mut writes, threshold);
        let alerts = writes.take_alerts();
        assert_eq!(alerts.len(), 1, "{alerts:?}");
        assert_eq!(alerts[0].0, "structure:slot-1");
        assert_eq!(u64::from(alerts[0].1), threshold);
        assert_eq!(alerts[0].2, "disk I/O error");
        for ticket in threshold + 1..threshold + 5 {
            fail_once(&mut writes, ticket);
        }
        assert!(
            writes.take_alerts().is_empty(),
            "the same cause is not raised again"
        );

        writes.queue(view(1)).unwrap();
        accept(&mut writes, 100);
        writes.complete(100, &Ok(ResultValue::ViewSaved)).unwrap();
        for ticket in 101..100 + threshold {
            fail_once(&mut writes, ticket);
        }
        assert!(writes.take_alerts().is_empty());
        fail_once(&mut writes, 100 + threshold);
        assert_eq!(
            writes.take_alerts().len(),
            1,
            "a new failure run is raised after a save"
        );
    }
    #[test]
    fn a_single_failure_followed_by_a_save_raises_nothing() {
        let mut writes = ViewWrites::default();
        for round in 0..5 {
            fail_once(&mut writes, round * 2 + 1);
            writes.queue(view(1)).unwrap();
            accept(&mut writes, round * 2 + 2);
            writes
                .complete(round * 2 + 2, &Ok(ResultValue::ViewSaved))
                .unwrap();
        }
        assert!(writes.take_alerts().is_empty());
        assert!(!writes.has_failures());
    }
    #[test]
    fn older_failure_cannot_remain_after_a_newer_queued_save_succeeds() {
        let mut writes = ViewWrites::default();
        writes.queue(view(1)).unwrap();
        accept(&mut writes, 10);
        writes.queue(view(1)).unwrap();
        assert!(
            writes
                .complete(10, &Err("old write failed".into()))
                .unwrap()
        );
        assert!(!writes.has_failures());
        assert!(writes.pending());
        accept(&mut writes, 11);
        writes.complete(11, &Ok(ResultValue::ViewSaved)).unwrap();
        assert!(!writes.pending());
        assert!(!writes.has_failures());
        assert_eq!(writes.sequence(), 2);
    }
    #[test]
    fn retirement_consumes_late_replies_without_marking_a_reused_stream_dirty() {
        for result in [Ok(ResultValue::ViewSaved), Err("late write failed".into())] {
            let mut writes = ViewWrites::default();
            writes.queue(view(1)).unwrap();
            accept(&mut writes, 10);
            writes.queue(view(1)).unwrap();
            writes.cancel("structure:slot-1");
            assert_eq!(writes.inflight_count(), 1);
            writes.queue(view(2)).unwrap();
            assert!(writes.complete(10, &result).unwrap());
            assert!(!writes.has_failures());
            assert_eq!(writes.queued["structure:slot-1"].binding.incarnation, 2);
            assert!(!writes.complete(10, &result).unwrap());
            accept(&mut writes, 11);
            writes.complete(11, &Ok(ResultValue::ViewSaved)).unwrap();
            assert!(!writes.pending());
            assert!(writes.cancelled.is_empty());
        }
    }
}
