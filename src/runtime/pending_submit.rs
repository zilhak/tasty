//! Live body-ack → settle → CR continuations owned by the original engine/resource.
//! No worker thread, durable input body or SID-only reinjection is involved.
use super::engine_access::EngineMut;
use tasty_terminal::{ResourceGeneration,WriteAck};
use std::time::{Duration,Instant};
pub(crate) const MAX_PENDING:usize=64;
pub(crate) struct PendingSubmit {
    surface:u32,generation:ResourceGeneration,ack:WriteAck,started:Instant,settle:Option<Instant>,
}
impl PendingSubmit {
    pub(crate) fn new(surface:u32,generation:ResourceGeneration,ack:WriteAck)->Self {Self {surface,generation,ack,started:Instant::now(),settle:None}}
    pub(crate) fn surface(&self)->u32 {self.surface}
    fn next(&self)->Instant {self.settle.unwrap_or_else(||(Instant::now()+Duration::from_millis(10)).min(self.started+Duration::from_secs(5)))}
}
impl super::engine_runtime::EngineRuntime {
    pub(crate) fn input_submit_deadline(&self)->Option<Instant> {self.pending_submits.iter().map(PendingSubmit::next).min()}
}
impl EngineMut<'_> {
    pub(crate) fn poll_input_submissions(&mut self) {
        let now=Instant::now();
        let mut pending=std::mem::take(&mut self.runtime.pending_submits);
        for mut submit in pending.drain(..) {
            if !self.runtime.terminals.matches_generation(submit.surface,submit.generation) || self.live.occupancy.is_hard_occupied(submit.surface) {continue;}
            if submit.settle.is_none() && (submit.ack.is_complete() || now.saturating_duration_since(submit.started)>=Duration::from_secs(5)) {submit.settle=Some(now+Duration::from_millis(20));}
            if submit.settle.is_some_and(|deadline|now>=deadline) {
                if let Some(terminal)=self.runtime.terminals.get_mut(submit.surface) && terminal.try_send_key_with_ack("\r").is_err() {tracing::warn!(surface=submit.surface,"terminal submission queue disconnected");}
            } else {self.runtime.pending_submits.push(submit);}
        }
    }
}
