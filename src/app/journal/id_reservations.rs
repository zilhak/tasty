//! Refill execution reservations through the same home-wide journal authority as local commands.
use super::*;
impl JournalApplication {
    pub(super) fn refill_execution_ids(
        &mut self,
        sessions: &mut [&mut EngineSession],
    ) -> Result<(), String> {
        for session in sessions {
            if self
                .execution_id_requests
                .values()
                .any(|id| *id == session.id)
            {
                continue;
            }
            let Some(binding) = session.journal_binding.clone() else {
                continue;
            };
            let needed = session.runtime.ids.needed()?;
            if needed.is_empty() {
                continue;
            }
            let ticket = self.next_ticket;
            match self.worker.submit(Request {
                ticket,
                work: Work::ReserveExecutionIds {
                    binding,
                    kinds: needed,
                },
            }) {
                Ok(()) => {
                    self.next_ticket = ticket
                        .checked_add(1)
                        .ok_or("execution ID ticket exhausted")?;
                    self.execution_id_requests.insert(ticket, session.id);
                }
                Err(crate::runtime::journal_product::SubmitError::Busy) => break,
                Err(error) => {
                    return Err(format!("execution ID reservation unavailable: {error:?}"));
                }
            }
        }
        Ok(())
    }
    pub(super) fn answer_execution_ids(
        &mut self,
        ticket: u64,
        result: &Result<ResultValue, String>,
        sessions: &mut [&mut EngineSession],
    ) -> Result<bool, String> {
        let Some(id) = self.execution_id_requests.remove(&ticket) else {
            return Ok(false);
        };
        let Some(session) = sessions.iter_mut().find(|session| session.id == id) else {
            return Ok(true);
        };
        match result {
            Ok(ResultValue::ExecutionIds { binding, ranges })
                if session.journal_binding.as_ref().is_some_and(|current| {
                    current.journal_id == binding.journal_id
                        && current.stream == binding.stream
                        && current.incarnation == binding.incarnation
                        && current.runtime_epoch == binding.runtime_epoch
                }) =>
            {
                session.runtime.ids.supply(ranges.clone())?
            }
            Ok(ResultValue::ExecutionIds { .. }) => {}
            Err(error) => {
                session.runtime.ids.refuse_refill(error.clone());
                tracing::warn!("execution identity reservation failed: {error}");
            }
            _ => return Err("execution ID reservation returned another result".into()),
        }
        Ok(true)
    }
}
