//! Bounded restore inputs retain their ticket until completion, including after cancellation.
use super::*;
use crate::runtime::surface_restorer::RestoreInput;

#[derive(Default)]
pub(super) struct Restorations {
    reads: HashMap<u64, (EngineId, RestoreInput)>,
    queue: VecDeque<(EngineId, RestoreInput)>,
    engines: HashMap<EngineId, EngineRestoration>,
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
        if let Some(state) = self.engines.get_mut(&engine) {
            if state.phase != RestorePhase::Retiring {
                state.ready.push(input);
            }
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
        if let Some(state) = self.engines.get_mut(&engine) {
            if state.phase != RestorePhase::Retiring {
                state.phase = RestorePhase::Serving;
            }
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
        let ResultValue::Payload { reference, bytes } = result.as_ref().map_err(Clone::clone)?
        else {
            return Err("restore payload request returned another value".into());
        };
        let session = sessions
            .iter()
            .find(|session| session.id == engine)
            .ok_or("restoring engine disappeared")?;
        let current = session
            .as_ref()
            .find_surface_by_id(input.surface_id)
            .and_then(|surface| {
                surface
                    .as_any()
                    .downcast_ref::<crate::runtime::surface_restorer::JournalPlaceholder>()
            })
            .ok_or("restoring surface no longer has its pending capture")?;
        if current.data.or(current.creation_seed) != input.reference {
            return Err("restoring surface capture changed while reading".into());
        }
        crate::runtime::surface_restorer::accept_payload(&mut input, *reference, bytes)?;
        self.return_ready(engine, input);
        Ok(true)
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
