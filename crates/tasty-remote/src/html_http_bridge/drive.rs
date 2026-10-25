//! Engine passes allocate common IDs at enqueue, dispatch finite verification and settle waiters.
use super::*;
use crate::html_http_client::ReceiveState;
use tasty_ipc::html_http::HttpRequest;
struct Driving<'a>(&'a AtomicBool);
impl Drop for Driving<'_> {
    fn drop(&mut self) {
        self.0.store(false, Ordering::Release);
    }
}
impl HttpBridge {
    pub fn drive(
        &self,
        mut next_id: impl FnMut() -> Option<TransferId>,
        mut spawn_verify: impl FnMut(VerifyJob) -> Result<(), VerifyJob>,
    ) -> bool {
        if self
            .driving
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return false;
        }
        let _driving = Driving(&self.driving);
        if !self.current() {
            self.close();
            return false;
        }
        let slots = {
            let Ok(inner) = self.lock() else {
                return false;
            };
            inner.slots.clone()
        };
        let mut changed = false;
        for slot in &slots {
            let Some((id, started)) = slot.assignment() else {
                slot.finish(Err(BridgeError::Failed));
                continue;
            };
            if slot.abandoned.load(Ordering::Acquire) {
                if let (Some(id), Some(document)) = (id, self.document())
                    && document
                        .apply(&self.sender, HttpAction::Decline(id))
                        .is_err()
                {
                    self.close();
                    return true;
                }
                continue;
            }
            if !started {
                changed = true;
                self.admit(slot, &mut next_id);
            }
        }
        if let Some(document) = self.document() {
            changed |= document.pump_commands(&self.sender);
            let mut jobs = {
                let Ok(mut inner) = self.lock() else {
                    return changed;
                };
                std::mem::take(&mut inner.jobs)
            };
            while jobs.len() < self.config.limits.verify_jobs {
                let Some(job) = document
                    .with_current(|model| model.take_verify_job())
                    .flatten()
                else {
                    break;
                };
                jobs.push(job);
            }
            let mut pending = Vec::new();
            for job in jobs {
                if !document
                    .with_current(|model| {
                        model.state(job.request_id()) == Some(ReceiveState::Verifying)
                    })
                    .unwrap_or(false)
                {
                    drop(job);
                    changed = true;
                    continue;
                }
                if let Err(job) = spawn_verify(job) {
                    pending.push(job);
                } else {
                    changed = true;
                }
            }
            if !self.current() {
                drop(pending);
                self.close();
                return true;
            }
            if !self.retain_pending(pending) {
                self.close();
                return true;
            }
            changed |= document
                .with_current(|model| model.poll_verification())
                .unwrap_or(false);
            for slot in &slots {
                if !slot.abandoned.load(Ordering::Acquire) {
                    self.settle(&document, slot);
                }
            }
            if !document.is_current() {
                self.close();
                return true;
            }
        }
        {
            let Ok(mut inner) = self.lock() else {
                return changed;
            };
            inner
                .slots
                .retain(|slot| !slot.abandoned.load(Ordering::Acquire));
        }
        changed
    }
    pub(super) fn retain_pending(&self, pending: Vec<VerifyJob>) -> bool {
        let Ok(mut inner) = self.lock() else {
            return false;
        };
        // Close may have drained the queue after the earlier currentness check.
        if !self.current() {
            drop(inner);
            // Job Drop can wake its owner, so it must run outside the bridge lock.
            drop(pending);
            return false;
        }
        inner.jobs = pending;
        true
    }
    fn admit(&self, slot: &Arc<wait::Slot>, next_id: &mut impl FnMut() -> Option<TransferId>) {
        if !self.current() {
            slot.finish(Err(BridgeError::Stale));
            return;
        }
        if let Some(response) = slot.reuse {
            let accepted = self.document().is_some_and(|document| {
                document
                    .apply(&self.sender, HttpAction::Reuse(response))
                    .unwrap_or(false)
            });
            slot.assign(accepted.then_some(response.request));
            if !accepted {
                slot.finish(Err(BridgeError::Failed));
            }
            return;
        }
        let native_http: &NativeRequest = &slot.request;
        if self.document().is_none()
            && !self.reload.as_ref().map_or_else(
                || native_http.logical_url == self.config.scope.entry_url,
                |r| {
                    r.valid()
                        && native_http.logical_url == r.request.logical_url
                        && native_http.method == r.request.method
                        && native_http.identity == r.request.identity
                },
            )
        {
            slot.assign(None);
            slot.finish(Err(BridgeError::Invalid));
            return;
        }
        let Some(id) = next_id().filter(|id| self.matches(*id)) else {
            slot.assign(None);
            slot.finish(Err(BridgeError::Invalid));
            return;
        };
        let document = match self.document() {
            Some(document) => Some(document),
            None => self.create_document(id),
        };
        let Some(document) = document else {
            slot.assign(None);
            slot.finish(Err(BridgeError::Failed));
            return;
        };
        slot.assign(Some(id));
        let first = document.entry() == id;
        let begin = HttpRequest::Begin {
            id,
            source_generation: self.config.scope.source_generation,
            method: slot.request.method.clone(),
            logical_url: slot.request.logical_url.clone(),
            headers: slot.request.headers.clone(),
            upload: slot.request.identity,
            allow_remote_content: self.config.scope.allow_remote_content,
        };
        let begin = if first && let Some(reload) = &self.reload {
            match begin.with_predecessor(reload.previous) {
                Ok(begin) => begin,
                Err(_) => {
                    slot.finish(Err(BridgeError::Invalid));
                    return;
                }
            }
        } else {
            begin
        };
        if document
            .admit_and_send(&self.sender, begin, slot.request.upload.clone())
            .is_err()
        {
            slot.finish(Err(BridgeError::Failed));
        }
    }
    fn create_document(&self, id: TransferId) -> Option<Arc<CapturedDocument>> {
        let live = self.live.clone();
        let source = self.source.clone();
        let scope = &self.config.scope;
        let document = CapturedDocument::new(
            DocumentKey {
                entry: id,
                source_generation: scope.source_generation,
                entry_url: self.reload.as_ref().map_or_else(
                    || scope.entry_url.clone(),
                    |r| r.request.logical_url.clone(),
                ),
                allow_remote_content: scope.allow_remote_content,
            },
            self.sender.epoch(),
            self.config.limits,
            self.config.pending.clone(),
            self.config.bodies.clone(),
            Arc::new(move || live.load(Ordering::Acquire) && source()),
            self.wake.clone(),
        )
        .ok()?;
        {
            let mut inner = self.lock().ok()?;
            if !self.current() {
                document.retire();
                return None;
            }
            inner.document = Some(document.clone());
        }
        Some(document)
    }
    fn matches(&self, id: TransferId) -> bool {
        let scope = &self.config.scope;
        (id.epoch, id.surface_id, id.binding, id.document)
            == (scope.epoch, scope.surface_id, scope.binding, scope.document)
            && id.request > 0
            && id.transfer > 0
    }
    fn settle(&self, document: &Arc<CapturedDocument>, slot: &wait::Slot) {
        let Some((Some(id), _)) = slot.assignment() else {
            return;
        };
        let state = document.with_current(|model| (model.state(id), model.verified(id)));
        match state {
            Some((Some(ReceiveState::Verified), Some(body))) => {
                let expected = slot.reuse.unwrap_or(body.head().id);
                if body.head().logical_url == slot.request.logical_url
                    && body.matches_native(expected, &slot.request.method, slot.request.identity)
                {
                    slot.finish(Ok(body));
                } else {
                    slot.finish(Err(BridgeError::Invalid));
                }
            }
            Some((Some(ReceiveState::Skipped), _)) => slot.finish(Err(BridgeError::Declined)),
            Some((
                Some(ReceiveState::Failed | ReceiveState::Cancelled | ReceiveState::Released),
                _,
            )) => slot.finish(Err(BridgeError::Failed)),
            None => slot.finish(Err(BridgeError::Stale)),
            _ => {}
        }
    }
}
