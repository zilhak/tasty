use super::*;
impl ClientDocument {
    /// Per-response encoded size consent; does not grant document cumulative Continue.
    pub fn allow(&mut self, id: TransferId) -> bool {
        if !self.active() || self.stopped {
            return false;
        }
        let Some(a) = self
            .attempts
            .iter_mut()
            .find(|a| a.id() == id && a.state == ReceiveState::SizeNotice)
        else {
            return false;
        };
        a.allowed = true;
        a.state = ReceiveState::Ready;
        true
    }
    pub fn continue_receiving(&mut self) -> bool {
        if !self.active() || self.stopped || self.continued {
            return false;
        }
        self.continued = true;
        self.paused = false;
        true
    }
    /// Ends presentation immediately; outstanding encoded authorization remains charged until
    /// the exact late chunk/terminal settles it, or this document/connection is retired.
    pub fn decline(&mut self, id: TransferId) -> Option<HttpRequest> {
        if !self.active() {
            return None;
        }
        let a = self.attempts.iter_mut().find(|a| a.id() == id)?;
        if a.terminal()
            || matches!(
                a.state,
                ReceiveState::Verified | ReceiveState::Skipped | ReceiveState::Reusing
            )
        {
            return None;
        }
        a.cancel.store(true, Ordering::Release);
        a.owner = None;
        a.result = None;
        a.job = None;
        a.upload = None;
        a.body = Vec::new();
        a.body_lease = None;
        if a.credit.is_none() {
            a.discard();
        }
        a.state = ReceiveState::Skipped;
        self.skipped += 1;
        Some(HttpRequest::Cancel {
            id,
            source_generation: self.key.source_generation,
        })
    }
    pub fn stop(&mut self) -> Option<HttpRequest> {
        if !self.active() || self.stopped {
            return None;
        }
        // Stop is one lifecycle command; no unbounded per-resource cancel command queue.
        for i in 0..self.attempts.len() {
            if self.attempts[i].state == ReceiveState::Reusing {
                self.attempts[i].state = ReceiveState::Verified;
            } else if !self.attempts[i].terminal()
                && self.attempts[i].state != ReceiveState::Verified
            {
                let id = self.attempts[i].id();
                self.decline(id);
            }
        }
        self.stopped = true;
        self.paused = false;
        Some(HttpRequest::Stop {
            entry: self.key.entry,
            source_generation: self.key.source_generation,
        })
    }
    pub fn close(&mut self) -> Option<HttpRequest> {
        if self.closed {
            return None;
        }
        let send = self.epoch.is_active();
        self.send_failed();
        send.then_some(HttpRequest::Close {
            entry: self.key.entry,
            source_generation: self.key.source_generation,
        })
    }
    /// A sender failure retires this document only. The transport owner retires the connection.
    pub fn send_failed(&mut self) {
        self.closed = true;
        self.stopped = true;
        self.paused = false;
        for a in &mut self.attempts {
            a.settle();
            a.discard();
            a.state = ReceiveState::Cancelled;
        }
        self.reserved = 0;
    }
    /// Exact retained response handshake; never emits Begin, uploads or a network fallback.
    pub fn reuse(&mut self, response: ResponseId) -> Option<HttpRequest> {
        if !self.active() {
            return None;
        }
        let a = self
            .attempts
            .iter_mut()
            .find(|a| a.response() == Some(response) && a.state == ReceiveState::Verified)?;
        let HttpRequest::Begin { method, upload, .. } = &a.request.as_ref()?.begin else {
            return None;
        };
        if !a
            .verified
            .as_ref()?
            .matches_native(response, method, *upload)
        {
            return None;
        }
        a.state = ReceiveState::Reusing;
        a.replay_head = false;
        Some(HttpRequest::Reuse { response })
    }
    pub fn release(&mut self, response: ResponseId) -> Option<HttpRequest> {
        if !self.active() {
            return None;
        }
        let a = self.attempts.iter_mut().find(|a| {
            a.response() == Some(response)
                && matches!(a.state, ReceiveState::Verified | ReceiveState::Reusing)
        })?;
        a.discard();
        a.state = ReceiveState::Released;
        Some(HttpRequest::Release { response })
    }
}
