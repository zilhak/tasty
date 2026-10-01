//! Bounded live capture scheduling. Persistence belongs to the worker, not the View or Core.
use super::*;
pub(super) struct PendingCapture {
    engine:EngineId,
    dirty_generation:std::sync::Weak<()>,
    work:Option<Work>,
}
impl JournalApplication {
    pub(crate) fn queue_surface_capture(&mut self,session:&EngineSession,force:bool) {
        if self.is_halted() || !session.runtime.settings.general.restore_layout {return;}
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
            if !session.persistence.dirty.is_dirty() && !(force && session.runtime.settings.general.restore_surface_content) {
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
            self.captures.insert(ticket,PendingCapture {engine:id,dirty_generation:session.persistence.dirty.generation(),work:Some(work)});
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
                Ok(ResultValue::Executed(_)|ResultValue::Stored(_))=>session.persistence.dirty.clear_if_generation(&capture.dirty_generation),
                Err(error)=>tracing::warn!("surface capture failed; dirty state retained: {error}"),
                _=>tracing::error!("capture returned an unrelated result; dirty state retained"),
            }
        }
        if !self.capture_requests.is_empty() {(self.wake)();}
        true
    }
}

pub(crate) enum PresetCaptureReply {
    Ipc {
        id: serde_json::Value,
        sender: std::sync::mpsc::SyncSender<crate::ipc::protocol::JsonRpcResponse>,
        name: Option<String>,
    },
    Intent {
        origin: crate::intent::IntentOrigin,
        view: std::sync::Weak<()>,
        name: Option<String>,
    },
}
pub(crate) struct PresetCaptureCompletion {
    pub(crate) engine: EngineId,
    pub(crate) binding: crate::runtime::journal_product::EngineBinding,
    pub(crate) reply: PresetCaptureReply,
    pub(crate) result: Result<(crate::intent::ClonedPreset, String), String>,
}
struct PendingPresetCapture {
    engine: EngineId,
    binding: crate::runtime::journal_product::EngineBinding,
    reply: PresetCaptureReply,
    work: Option<Work>,
    weight: usize,
    _lease: crate::runtime::journal_payload::PayloadReadLease,
}
#[derive(Default)]
pub(super) struct PresetCaptures {
    pending: std::collections::BTreeMap<u64, PendingPresetCapture>,
    completed: Vec<PresetCaptureCompletion>,
    bytes: usize,
}
impl JournalApplication {
    pub(crate) fn queue_preset_capture(
        &mut self,
        session: &EngineSession,
        presentation: &dyn crate::model::StructurePresentation,
        kind: tasty_presets::PresetKind,
        source: u32,
        reply: PresetCaptureReply,
    ) -> Result<u64, String> {
        if self.is_halted() { return Err("journal is halted".into()); }
        let binding = session.journal_binding.clone().ok_or("preset capture engine is unbound")?;
        let draft = crate::intent::preset_capture::capture_draft(presentation, &session.as_ref(), kind, source)?;
        let reply_bytes = match &reply {
            PresetCaptureReply::Ipc {id, name, ..} => id.to_string().len().saturating_add(name.as_ref().map_or(0, String::len)),
            PresetCaptureReply::Intent {name, ..} => name.as_ref().map_or(0, String::len),
        };
        let weight = draft.weight().saturating_add(reply_bytes).saturating_add(256);
        if self.preset_captures.pending.len() + self.preset_captures.completed.len() >= 64
            || self.preset_captures.bytes.saturating_add(weight) > tasty_ipc::admission::QUEUED_BYTES_LIMIT {
            return Err("preset capture queue is full".into());
        }
        let lease = self.worker.retain_payloads(draft.references())?;
        let ticket = self.next_ticket;
        self.next_ticket = ticket.checked_add(1).ok_or("preset capture ticket range exhausted")?;
        self.preset_captures.bytes += weight;
        self.preset_captures.pending.insert(ticket, PendingPresetCapture {
            engine: session.id, binding, reply, work: Some(Work::CapturePreset {draft}), weight, _lease: lease,
        });
        (self.wake)();
        Ok(ticket)
    }

    pub(super) fn submit_preset_captures(&mut self) -> Result<(), String> {
        if self.pauses_observation() { return Ok(()); }
        if self.preset_captures.pending.values().any(|pending| pending.work.is_none()) { return Ok(()); }
        let mut rejected = Vec::new();
        for (ticket, pending) in &mut self.preset_captures.pending {
            let Some(work) = pending.work.take() else { continue; };
            match self.worker.submit_owned(Request {ticket: *ticket, work}) {
                Ok(()) => break,
                Err((crate::runtime::journal_product::SubmitError::Busy, request)) => {
                    pending.work = Some(request.work); break;
                },
                Err((crate::runtime::journal_product::SubmitError::TooLarge, _request)) => {
                    rejected.push(*ticket);
                },
                Err((error, request)) => {
                    pending.work = Some(request.work);
                    return Err(format!("preset capture transport unavailable: {error:?}"));
                },
            }
        }
        for ticket in rejected {
            self.answer_preset_capture(ticket, &Err("preset capture exceeds request byte limit".into()));
        }
        Ok(())
    }

    pub(super) fn answer_preset_capture(&mut self, ticket: u64, result: &Result<ResultValue, String>) -> bool {
        let Some(pending) = self.preset_captures.pending.remove(&ticket) else { return false; };
        let mut completed_weight = 256;
        let captured = match result {
            Ok(ResultValue::CapturedPreset {preset, base_name}) => {
                let bytes = match preset {
                    crate::intent::ClonedPreset::Workspace(value) => serde_json::to_vec(value),
                    crate::intent::ClonedPreset::Pane(value) => serde_json::to_vec(value),
                    crate::intent::ClonedPreset::Tab(value) => serde_json::to_vec(value),
                };
                completed_weight = bytes.map_or(usize::MAX, |bytes| bytes.len().saturating_add(base_name.len()).saturating_add(256));
                if self.preset_captures.bytes.saturating_sub(pending.weight).saturating_add(completed_weight) > tasty_ipc::admission::QUEUED_BYTES_LIMIT {
                    completed_weight = 256;
                    Err("resolved preset exceeds available queue bytes".into())
                } else { Ok((preset.clone(), base_name.clone())) }
            },
            Err(error) => Err(error.clone()),
            _ => Err("preset capture returned an unrelated result".into()),
        };
        self.preset_captures.bytes = self.preset_captures.bytes.saturating_sub(pending.weight).saturating_add(completed_weight);
        self.preset_captures.completed.push(PresetCaptureCompletion {
            engine: pending.engine, binding: pending.binding, reply: pending.reply, result: captured,
        });
        (self.wake)();
        true
    }

    pub(crate) fn has_pending_preset_capture(&self, engine: EngineId) -> bool {
        self.preset_captures.pending.values().any(|pending| pending.engine == engine)
            || self.preset_captures.completed.iter().any(|result| result.engine == engine)
    }

    pub(crate) fn has_pending_preset_captures(&self) -> bool {
        !self.preset_captures.pending.is_empty() || !self.preset_captures.completed.is_empty()
    }

    pub(crate) fn take_preset_captures(&mut self) -> Vec<PresetCaptureCompletion> {
        self.preset_captures.bytes = self.preset_captures.pending.values().map(|pending| pending.weight).sum();
        std::mem::take(&mut self.preset_captures.completed)
    }
}

/// Save completion belongs to the original View. The caller validates its weak identity before
/// emitting toast/dialog changes; an absent View never prevents an already accepted preset save.
pub(crate) struct PresetCaptureNotice {
    pub(crate) engine: EngineId,
    pub(crate) binding: crate::runtime::journal_product::EngineBinding,
    pub(crate) origin: crate::intent::IntentOrigin,
    pub(crate) view: std::sync::Weak<()>,
    pub(crate) result: Result<(tasty_presets::PresetKind, String), String>,
}
impl PresetCaptureCompletion {
    pub(crate) fn save(self, services: &crate::app::services::AppServices) -> Option<PresetCaptureNotice> {
        let name = match &self.reply {
            PresetCaptureReply::Ipc {name, ..} | PresetCaptureReply::Intent {name, ..} => name.as_deref(),
        };
        let result = self.result.map_err(PresetSaveError::Capture).and_then(|(preset, base)| {
            let kind = preset.kind();
            match crate::intent::preset::save_inner(services, &base, name, false, preset).map_err(PresetSaveError::Store)? {
                crate::intent::preset::SaveOutcome::Saved(name) => Ok((kind, name)),
                crate::intent::preset::SaveOutcome::SkippedExists => Err(PresetSaveError::Capture("preset name already exists (overwrite=false)".into())),
            }
        });
        match self.reply {
            PresetCaptureReply::Ipc {id, sender, ..} => {
                let response = match result {
                    Ok((_, name)) => crate::ipc::protocol::JsonRpcResponse::success(id, serde_json::json!({"name": name})),
                    Err(PresetSaveError::Capture(error)) => crate::ipc::protocol::JsonRpcResponse::invalid_params(id, error),
                    Err(PresetSaveError::Store(error)) => crate::adapters::ipc::handler::preset::mutation_error(id, error),
                };
                crate::ipc::server::send_response(&sender, response);
                None
            },
            PresetCaptureReply::Intent {origin, view, ..} => Some(PresetCaptureNotice {
                engine: self.engine, binding: self.binding, origin, view,
                result: result.map_err(|error| match error {PresetSaveError::Capture(error) => error, PresetSaveError::Store(error) => error.to_string()}),
            }),
        }
    }
}

enum PresetSaveError {
    Capture(String),
    Store(crate::intent::preset::PresetMutationError),
}
