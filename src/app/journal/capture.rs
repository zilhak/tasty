//! Bounded live capture scheduling. Persistence belongs to the worker, not the View or Core.
use super::*;
pub(super) struct PendingCapture {
    engine:EngineId,
    dirty_generation:std::sync::Weak<()>,
    work:Option<Work>,
}
impl JournalApplication {
    pub(crate) fn queue_surface_capture(&mut self,session:&EngineSession,force:bool) {
        if self.is_halted() || !session.core_state.settings.general.restore_layout {return;}
        self.capture_requests.entry(session.id).and_modify(|queued|*queued|=force).or_insert(force);
        (self.wake)();
    }
    pub(crate) fn has_pending_capture(&self,engine:EngineId)->bool {
        self.capture_requests.contains_key(&engine) || self.captures.values().any(|capture|capture.engine==engine)
    }
    pub(super) fn submit_captures(&mut self,sessions:&mut [&mut EngineSession])->Result<(),String> {
        if self.pauses_observation() {return Ok(());}
        let ready:Vec<_>=self.capture_requests.keys().copied().filter(|engine|!self.captures.values().any(|capture|capture.engine==*engine)).take(8).collect();
        for id in ready {
            let Some(session)=sessions.iter().find(|session|session.id==id) else {return Err("capture engine owner disappeared".into());};
            let force=self.capture_requests[&id];
            if !session.core_state.layout_dirty.is_dirty() && !(force && session.core_state.settings.general.restore_surface_content) {
                self.capture_requests.remove(&id);continue;
            }
            let binding=session.journal_binding.clone().ok_or("capture engine is unbound")?;
            let surfaces=match crate::runtime::surface_capture::capture(session) {
                Ok(surfaces)=>surfaces,
                Err(error)=>{tracing::warn!("surface capture failed; dirty state retained: {error}");self.capture_requests.remove(&id);continue;},
            };
            let work=Work::Capture {binding,surfaces};
            let held=self.captures.values().filter_map(|capture|capture.work.as_ref()).map(crate::runtime::journal_product::request_size).fold(0usize,usize::saturating_add);
            if self.captures.len()>=64 || held.saturating_add(crate::runtime::journal_product::request_size(&work))>tasty_ipc::admission::QUEUED_BYTES_LIMIT {
                tracing::warn!("capture exceeds available queue bytes; previous immutable data retained");self.capture_requests.remove(&id);continue;
            }
            let ticket=self.next_ticket;
            self.next_ticket=ticket.checked_add(1).ok_or("capture ticket range exhausted")?;
            self.captures.insert(ticket,PendingCapture {engine:id,dirty_generation:session.core_state.layout_dirty.generation(),work:Some(work)});
            self.capture_requests.remove(&id);
        }
        for (ticket,capture) in &mut self.captures {
            let Some(work)=capture.work.take() else {continue;};
            match self.worker.submit_owned(Request {ticket:*ticket,work}) {
                Ok(())=>{},
                Err((crate::runtime::journal_product::SubmitError::Busy,request))=>{capture.work=Some(request.work);break;},
                Err((error,request))=>{capture.work=Some(request.work);return Err(format!("capture transport unavailable: {error:?}"));},
            }
        }
        Ok(())
    }
    pub(super) fn answer_capture(&mut self,ticket:u64,result:&Result<ResultValue,String>,sessions:&mut [&mut EngineSession])->bool {
        let Some(capture)=self.captures.remove(&ticket) else {return false;};
        if let Some(session)=sessions.iter_mut().find(|session|session.id==capture.engine) {
            match result {
                Ok(ResultValue::Executed(_)|ResultValue::Stored(_))=>session.core_state.layout_dirty.clear_if_generation(&capture.dirty_generation),
                Err(error)=>tracing::warn!("surface capture failed; dirty state retained: {error}"),
                _=>tracing::error!("capture returned an unrelated result; dirty state retained"),
            }
        }
        if !self.capture_requests.is_empty() {(self.wake)();}
        true
    }
}
