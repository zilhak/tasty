use super::*;
use base64::Engine;
use tasty_ipc::html_http::{AUTO_BYTES, CHUNK_BYTES, DOCUMENT_BYTES};
impl ClientDocument {
    /// Advances local state before returning one command. Never retry a returned command.
    pub fn next_command(&mut self) -> Option<HttpRequest> {
        if !self.active() || self.stopped {
            return None;
        }
        if let Some(command) = self
            .attempts
            .iter_mut()
            .find_map(|a| upload_command(a, self.key.source_generation))
        {
            return Some(command);
        }
        self.next_credit()
    }
    fn next_credit(&mut self) -> Option<HttpRequest> {
        for a in &mut self.attempts {
            if a.state != ReceiveState::Ready || a.credit.is_some() {
                continue;
            }
            let head = a.head.as_ref()?;
            let (max, reservation) = credit_window(a, head);
            if self.paused && reservation != 0 {
                continue;
            }
            let Some(total) = self
                .received
                .checked_add(self.reserved)
                .and_then(|n| n.checked_add(reservation))
            else {
                continue;
            };
            if !self.continued && total > DOCUMENT_BYTES {
                self.paused = true;
                continue;
            }
            self.reserved += reservation;
            a.credit = Some(Credit {
                max,
                reserved: reservation,
            });
            a.state = ReceiveState::Receiving;
            return Some(HttpRequest::Credit {
                response: head.id,
                offset: a.offset,
                max_bytes: max as u32,
                allow_large: a.allowed,
            });
        }
        None
    }
    pub fn take_verify_job(&mut self) -> Option<VerifyJob> {
        if !self.active() {
            self.send_failed();
            return None;
        }
        self.attempts.iter_mut().find_map(|a| a.job.take())
    }
    pub fn poll_verification(&mut self) -> bool {
        if !self.active() {
            let changed = !self.closed;
            self.send_failed();
            return changed;
        }
        let mut changed = false;
        for i in 0..self.attempts.len() {
            let a = &mut self.attempts[i];
            let Some(result) = a.result.as_ref().and_then(|r| r.try_recv().ok()) else {
                continue;
            };
            if a.state != ReceiveState::Verifying
                || !a
                    .owner
                    .as_ref()
                    .is_some_and(|owner| result.owner.ptr_eq(&Arc::downgrade(owner)))
            {
                continue;
            }
            a.result = None;
            match result.value {
                Ok(body) => {
                    a.verified = Some(body);
                    a.state = ReceiveState::Verified;
                }
                Err(_) => self.fail(i),
            }
            changed = true;
        }
        changed
    }
    pub fn snapshot(&self) -> Snapshot {
        Snapshot {
            received: self.received,
            reserved: self.reserved,
            paused: self.paused,
            receiving: self.active()
                && self.attempts.iter().any(|a| {
                    matches!(
                        a.state,
                        ReceiveState::Uploading
                            | ReceiveState::WaitingHead
                            | ReceiveState::SizeNotice
                            | ReceiveState::Ready
                            | ReceiveState::Receiving
                            | ReceiveState::Verifying
                            | ReceiveState::Reusing
                            | ReceiveState::Cancelling
                    )
                }),
            skipped: self.skipped,
            notices: self
                .attempts
                .iter()
                .filter(|a| a.state == ReceiveState::SizeNotice)
                .map(Attempt::id)
                .collect(),
        }
    }
}

fn upload_command(a: &mut Attempt, source_generation: u64) -> Option<HttpRequest> {
    if a.state != ReceiveState::Uploading {
        return None;
    }
    let upload = a.upload.as_mut()?;
    // Empty uploads need their initial Ready. Nonempty uploads finish once their final
    // authorized bytes have been handed to the ordered sender; the server sends no final ACK.
    if upload.offset == upload.bytes.len() && (upload.offset != 0 || upload.window.is_some()) {
        a.committed = true;
        a.state = ReceiveState::WaitingHead;
        a.upload = None;
        return Some(HttpRequest::Commit {
            id: a.id(),
            source_generation,
        });
    }
    let max = upload.window.take()?;
    let offset = upload.offset;
    let end = offset + max.min(upload.bytes.len() - offset);
    let data_b64 = base64::engine::general_purpose::STANDARD.encode(&upload.bytes[offset..end]);
    upload.offset = end;
    Some(HttpRequest::Upload {
        id: a.id(),
        source_generation,
        offset: offset as u64,
        data_b64,
    })
}
fn credit_window(a: &Attempt, head: &ResponseHead) -> (usize, u64) {
    if !head.has_body || head.encoded_length == Some(a.offset) {
        return (1, 0);
    }
    let remaining = head
        .encoded_length
        .map_or(CHUNK_BYTES as u64, |n| n.saturating_sub(a.offset));
    let automatic = if a.allowed {
        CHUNK_BYTES as u64
    } else if a.offset < AUTO_BYTES {
        AUTO_BYTES - a.offset
    } else {
        1
    };
    let max = remaining.min(automatic).min(CHUNK_BYTES as u64);
    (max as usize, max)
}
