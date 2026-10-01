//! Logical write limits, independent of disk pressure and SQLite storage amplification.
use crate::{CommitRequest, EffectClaim, EffectTransition, StoreError, StoreResult};

pub(crate) const MAX_WRITE_BYTES: usize = 64 * 1024 * 1024;

pub(crate) fn blob(bytes: &[u8]) -> StoreResult<()> {
    if bytes.len() > MAX_WRITE_BYTES {
        return Err(StoreError::WriteSizeExceeded {kind:"payload",bytes:bytes.len(),limit:MAX_WRITE_BYTES});
    }
    Ok(())
}

/// Accounts for every variable input field plus 256 bytes per record/envelope and 8 per ref.
/// This is a bounded logical request, not a bound on SQLite pages, indexes or WAL bytes.
pub(crate) struct Budget(usize);
impl Budget {
    pub(crate) fn new() -> Self {Self(0)}
    pub(crate) fn add(&mut self, bytes:usize) -> StoreResult<()> {
        self.0=self.0.saturating_add(bytes);
        if self.0>MAX_WRITE_BYTES {return Err(StoreError::WriteSizeExceeded {kind:"batch",bytes:self.0,limit:MAX_WRITE_BYTES});}
        Ok(())
    }
    fn record(&mut self) -> StoreResult<()> {self.add(256)}
    fn bytes(&mut self, bytes:&[u8]) -> StoreResult<()> {blob(bytes)?;self.add(bytes.len())}
    fn optional(&mut self, bytes:Option<&[u8]>) -> StoreResult<()> {if let Some(bytes)=bytes {self.bytes(bytes)?;} Ok(())}
    pub(crate) fn snapshot(&mut self, snapshot:&crate::NewSnapshot)->StoreResult<()> {
        self.record()?;self.bytes(&snapshot.bytes)?;self.add(snapshot.referenced_payloads.len().saturating_mul(8))
    }
    pub(crate) fn manifest(&mut self, manifest:&crate::NewRestoreManifest)->StoreResult<()> {
        self.record()?;self.bytes(&manifest.view)?;self.add(manifest.restore_key.len())?;
        self.add(manifest.referenced_payloads.len().saturating_mul(8))
    }
    pub(crate) fn transition(&mut self, change:&EffectTransition) -> StoreResult<()> {
        self.record()?;self.add(change.effect_id.len())?;self.optional(change.result.as_deref())?;
        match &change.claim {
            Some(EffectClaim::Activation(claim))=>{self.add(claim.engine_id.len())?;self.add(claim.surface_id.as_ref().map_or(0,String::len))?;},
            Some(EffectClaim::Obligation(claim))=>{self.add(claim.engine_id.len())?;self.add(claim.operation_id.len())?;},
            None=>{},
        }
        Ok(())
    }
}

pub(crate) fn commit(request:&CommitRequest)->StoreResult<Budget> {
    let mut size=Budget::new();size.record()?;
    if let Some(command)=&request.command {
        size.record()?;size.add(command.command_id.len())?;
        if let Some(key)=&command.key {size.add(key.caller_scope.len())?;size.add(key.idempotency_key.len())?;}
        size.bytes(&command.request_digest)?;size.bytes(&command.resolved)?;size.optional(command.response.as_deref())?;
    }
    for append in &request.appends {
        size.record()?;size.add(append.stream_id.0.len())?;
        for event in &append.events {
            size.record()?;size.add(event.event_id.len())?;size.add(event.payload.type_tag.len())?;
            size.bytes(&event.payload.bytes)?;size.add(event.actor.len())?;size.add(event.origin.len())?;
            size.add(event.causation_id.as_ref().map_or(0,String::len))?;
            size.add(event.payload_refs.len().saturating_mul(8))?;
        }
    }
    for update in &request.command_updates {
        size.record()?;size.add(update.command_id.len())?;size.optional(update.response.as_deref())?;
    }
    for effect in &request.effects {
        size.record()?;size.add(effect.effect_id.len())?;size.add(effect.operation_id.len())?;
        size.add(effect.payload.type_tag.len())?;size.bytes(&effect.payload.bytes)?;
    }
    for transition in &request.effect_transitions {size.transition(transition)?;}
    Ok(size)
}
