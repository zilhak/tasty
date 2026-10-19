//! Bounded restore inputs retain their ticket until completion, including after cancellation.
use super::*;
use crate::runtime::surface_restorer::RestoreInput;

#[derive(Default)]
pub(super) struct Restorations {
    reads: HashMap<u64, (EngineId, RestoreInput)>,
    queue: VecDeque<(EngineId, RestoreInput)>,
    engines: HashMap<EngineId, EngineRestoration>,
    /// 읽거나 해석하지 못한 capture. 알림으로 옮긴 뒤 비운다.
    failures: Vec<PayloadFailure>,
    /// 보존을 요청할 원본 payload와 응답을 기다리는 ticket.
    preserve: VecDeque<tasty_core::DataRef>,
    preserving: std::collections::HashSet<u64>,
}

/// surface 하나의 복원 자료를 잃은 사실. 그 surface는 빈 상태로 다시 시작한다.
pub(super) struct PayloadFailure {
    pub(super) engine: EngineId,
    pub(super) surface: u32,
    pub(super) workspace: String,
    pub(super) cause: String,
}
#[derive(Default)]
struct EngineRestoration {
    ready: Vec<RestoreInput>,
    phase: RestorePhase,
}
#[derive(Default, PartialEq, Eq)]
enum RestorePhase {
    #[default]
    Bootstrapping,
    Serving,
    Retiring,
}

impl Restorations {
    pub(super) fn register(&mut self, engine: EngineId) {
        self.engines.entry(engine).or_default();
    }
    pub(super) fn stage(&mut self, engine: EngineId, input: RestoreInput) {
        if !self.accepting(engine) {
            return;
        }
        if input.reference.is_some() {
            self.queue.push_back((engine, input));
        } else {
            self.return_ready(engine, input);
        }
    }
    fn accepting(&self, engine: EngineId) -> bool {
        self.engines
            .get(&engine)
            .is_some_and(|state| state.phase != RestorePhase::Retiring)
    }
    pub(super) fn return_ready(&mut self, engine: EngineId, input: RestoreInput) {
        if let Some(state) = self.engines.get_mut(&engine)
            && state.phase != RestorePhase::Retiring
        {
            state.ready.push(input);
        }
    }
    pub(super) fn ready(&self, engine: EngineId) -> &[RestoreInput] {
        self.engines
            .get(&engine)
            .map(|state| state.ready.as_slice())
            .unwrap_or_default()
    }
    pub(super) fn take_ready(&mut self, engine: EngineId, index: usize) -> RestoreInput {
        self.engines
            .get_mut(&engine)
            .expect("selected restore engine")
            .ready
            .remove(index)
    }
    pub(super) fn activated(&mut self, engine: EngineId, surface: u32) {
        if let Some(state) = self.engines.get_mut(&engine) {
            state.ready.retain(|input| input.surface_id != surface);
        }
    }
    pub(super) fn finish_bootstrap(&mut self, engine: EngineId) {
        if let Some(state) = self.engines.get_mut(&engine)
            && state.phase != RestorePhase::Retiring
        {
            state.phase = RestorePhase::Serving;
        }
    }
    pub(super) fn boot_done(&self, engine: EngineId) -> bool {
        self.engines
            .get(&engine)
            .is_some_and(|state| state.phase == RestorePhase::Serving)
    }
    pub(super) fn has_reads(&self, engine: EngineId) -> bool {
        self.reads.values().any(|(id, _)| *id == engine)
    }
    pub(super) fn has_pending(&self, engine: EngineId) -> bool {
        self.has_reads(engine) || self.queue.iter().any(|(id, _)| *id == engine)
    }
    pub(super) fn is_reading(&self, engine: EngineId, surface: u32) -> bool {
        self.queue
            .iter()
            .chain(self.reads.values())
            .any(|(id, input)| *id == engine && input.surface_id == surface)
    }
    pub(super) fn is_ready(&self, engine: EngineId) -> bool {
        self.accepting(engine)
            && !self.has_pending(engine)
            && (self.boot_done(engine) || self.ready(engine).is_empty())
    }
    #[cfg(feature = "gui")]
    pub(super) fn retire(&mut self, engine: EngineId) {
        self.queue.retain(|(id, _)| *id != engine);
        if let Some(state) = self.engines.get_mut(&engine) {
            state.ready.clear();
            state.phase = RestorePhase::Retiring;
        }
    }
    #[cfg(feature = "gui")]
    pub(super) fn forget(&mut self, engine: EngineId) {
        self.queue.retain(|(id, _)| *id != engine);
        self.engines.remove(&engine);
        // Accepted reads keep their tickets so a late response is consumed, never re-routed.
    }
    pub(super) fn submit(
        &mut self,
        worker: &JournalWorker,
        next_ticket: &mut u64,
    ) -> Result<(), String> {
        const MAX_READS: usize = 8;
        while self.reads.len() < MAX_READS {
            let Some((engine, input)) = self.queue.pop_front() else {
                break;
            };
            let ticket = *next_ticket;
            let next = ticket
                .checked_add(1)
                .ok_or("journal ticket range exhausted")?;
            let request = Request {
                ticket,
                work: Work::ReadPayload(input.reference.expect("referenced capture")),
            };
            match worker.submit(request) {
                Ok(()) => {
                    *next_ticket = next;
                    self.reads.insert(ticket, (engine, input));
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => {
                    self.queue.push_front((engine, input));
                    break;
                }
                Err(error) => {
                    self.queue.push_front((engine, input));
                    return Err(format!("restore capture read: {error:?}"));
                }
            }
        }
        Ok(())
    }
    /// 읽기 결과를 복원 입력에 반영한다. capture를 읽거나 해석하지 못하면 그 surface만 빈 상태로
    /// 다시 시작하게 하고 원본 보존을 요청한다. 읽는 사이 engine이 닫혔거나 surface가 바뀐 것은 손상이
    /// 아니므로 결과를 버리고, 바뀐 capture는 다시 읽는다. 저장소를 더 쓸 수 없는 오류만 `Err`다.
    pub(super) fn complete(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
        sessions: &[&mut EngineSession],
    ) -> Result<bool, String> {
        let Some((engine, mut input)) = self.reads.remove(&ticket) else {
            return Ok(false);
        };
        if !self.accepting(engine) {
            return Ok(true);
        }
        let outcome = match result.as_ref().map_err(Clone::clone)? {
            ResultValue::Payload { reference, bytes } => {
                crate::runtime::surface_restorer::accept_payload(&mut input, *reference, bytes)
            }
            ResultValue::PayloadUnavailable { reason, .. } => Err(reason.clone()),
            _ => return Err("restore payload request returned another value".into()),
        };
        let Some(session) = sessions.iter().find(|session| session.id == engine) else {
            tracing::warn!(?engine, "restore read finished after its engine closed");
            return Ok(true);
        };
        if !self.still_waiting(engine, session, &input) {
            return Ok(true);
        }
        if let Err(cause) = outcome {
            self.record_failure(engine, session, &mut input, cause);
        }
        self.return_ready(engine, input);
        Ok(true)
    }

    /// surface가 아직 이 capture를 기다리는지 본다. 바뀐 capture는 다시 읽도록 큐에 넣는다.
    fn still_waiting(
        &mut self,
        engine: EngineId,
        session: &EngineSession,
        input: &RestoreInput,
    ) -> bool {
        let current = session
            .as_ref()
            .find_surface_by_id(input.surface_id)
            .and_then(|surface| {
                surface
                    .as_any()
                    .downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()
            })
            .map(|current| current.data.or(current.creation_seed));
        let Some(current) = current else {
            tracing::warn!(
                surface = input.surface_id,
                "restore read finished after its surface stopped waiting for it"
            );
            return false;
        };
        if current == input.reference {
            return true;
        }
        tracing::warn!(
            surface = input.surface_id,
            "restoring surface capture changed while reading; reading the new capture"
        );
        if let Some(fresh) = crate::runtime::surface_restorer::describe(&session.as_ref())
            .into_iter()
            .find(|fresh| fresh.surface_id == input.surface_id)
        {
            self.stage(engine, fresh);
        }
        false
    }

    fn record_failure(
        &mut self,
        engine: EngineId,
        session: &EngineSession,
        input: &mut RestoreInput,
        cause: String,
    ) {
        let workspace = session
            .core_state
            .local_workspaces()
            .iter()
            .find(|workspace| workspace.all_surface_ids().contains(&input.surface_id))
            .map(|workspace| workspace.name.clone())
            .unwrap_or_default();
        tracing::warn!(
            surface = input.surface_id,
            workspace,
            "surface restore data could not be read; starting it empty: {cause}"
        );
        crate::runtime::surface_restorer::discard_payload(input);
        if let Some(reference) = input.reference {
            self.preserve.push_back(reference);
        }
        self.failures.push(PayloadFailure {
            engine,
            surface: input.surface_id,
            workspace,
            cause,
        });
    }

    pub(super) fn take_failures(&mut self) -> Vec<PayloadFailure> {
        std::mem::take(&mut self.failures)
    }

    /// 보존 요청을 보낸다. 큐가 차 있으면 다음 차례에 다시 보낸다.
    pub(super) fn submit_preservations(
        &mut self,
        worker: &JournalWorker,
        next_ticket: &mut u64,
    ) -> Result<(), String> {
        while let Some(reference) = self.preserve.pop_front() {
            let ticket = *next_ticket;
            let next = ticket
                .checked_add(1)
                .ok_or("journal ticket range exhausted")?;
            match worker.submit(Request {
                ticket,
                work: Work::PreserveFailedPayload(reference),
            }) {
                Ok(()) => {
                    *next_ticket = next;
                    self.preserving.insert(ticket);
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => {
                    self.preserve.push_front(reference);
                    break;
                }
                Err(error) => {
                    self.preserve.push_front(reference);
                    return Err(format!("restore failure preservation: {error:?}"));
                }
            }
        }
        Ok(())
    }

    /// 보존 응답을 받는다. 실패해도 저장소를 계속 쓸 수 있으므로 로그만 남긴다.
    pub(super) fn complete_preservation(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
    ) -> bool {
        if !self.preserving.remove(&ticket) {
            return false;
        }
        if let Err(error) = result {
            tracing::warn!("failed to keep the unreadable restore payload: {error}");
        }
        true
    }

    #[cfg(test)]
    pub(super) fn read_count(&self) -> usize {
        self.reads.len()
    }
    #[cfg(test)]
    pub(super) fn queued_count(&self) -> usize {
        self.queue.len()
    }
}
