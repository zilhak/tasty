use super::*;
use tasty_ipc::html_http::{AUTO_BYTES, CHUNK_BYTES, CommandKind, decode_chunk};
impl ClientDocument {
    pub(super) fn receive_inner(&mut self, reply: HttpReply) -> bool {
        if !self.active() {
            return false;
        }
        let Some(i) = self.attempts.iter().position(|a| a.id() == reply.id()) else {
            return false;
        };
        let (generation, response) = match &reply {
            HttpReply::Head { head } => (head.id.source_generation, Some(head.id)),
            HttpReply::AllowanceRequired { response, .. }
            | HttpReply::Chunk { response, .. }
            | HttpReply::Complete { response, .. } => (response.source_generation, Some(*response)),
            HttpReply::UploadReady {
                source_generation, ..
            } => (*source_generation, None),
            HttpReply::Failure {
                source_generation,
                response,
                ..
            }
            | HttpReply::Cancelled {
                source_generation,
                response,
                ..
            }
            | HttpReply::CommandRejected {
                source_generation,
                response,
                ..
            } => (*source_generation, *response),
        };
        if generation != self.key.source_generation {
            return false;
        }
        let a = &self.attempts[i];
        if a.terminal()
            || a.response()
                .zip(response)
                .is_some_and(|(known, got)| known != got)
        {
            return false;
        }
        if a.state == ReceiveState::Verified
            && !matches!(
                reply,
                HttpReply::CommandRejected { .. }
                    | HttpReply::Failure { .. }
                    | HttpReply::Cancelled { .. }
            )
        {
            return false;
        }
        if reply.validate().is_err() {
            self.fail(i);
            return true;
        }
        if a.state == ReceiveState::Verifying
            && let HttpReply::Complete {
                encoded, decoded, ..
            } = &reply
            && a.completion == Some((*encoded, *decoded))
        {
            return false;
        }
        if let HttpReply::AllowanceRequired {
            encoded_at_least,
            known_length: Some(length),
            ..
        } = &reply
            && *encoded_at_least == *length
            && a.head
                .as_ref()
                .is_some_and(|h| h.encoded_length == Some(*length))
            && (a.allowed || a.state == ReceiveState::SizeNotice)
        {
            return false;
        }
        // Rejections correlate one command, not the accepted operation or credit. Only an
        // initial unaccepted Begin waiter may end here; destructive Commit has a later Failure.
        if let HttpReply::CommandRejected { command, .. } = reply {
            if command == CommandKind::Begin
                && a.state == ReceiveState::Uploading
                && a.upload
                    .as_ref()
                    .is_some_and(|u| u.window.is_none() && u.offset == 0)
            {
                self.fail(i);
                return true;
            }
            return false;
        }
        let valid = self.apply(i, reply);
        if !valid {
            self.fail(i);
        }
        true
    }
    fn apply(&mut self, i: usize, reply: HttpReply) -> bool {
        match reply {
            HttpReply::Failure { .. } => {
                self.fail(i);
                true
            }
            HttpReply::Cancelled { .. } => {
                let a = &mut self.attempts[i];
                self.reserved -= a.settle();
                let skipped = a.state == ReceiveState::Skipped;
                a.discard();
                a.state = if skipped {
                    ReceiveState::Skipped
                } else {
                    ReceiveState::Cancelled
                };
                true
            }
            HttpReply::UploadReady {
                offset, max_bytes, ..
            } => self.upload_ready(i, offset, max_bytes),
            HttpReply::Head { head } => self.head_received(i, head),
            HttpReply::AllowanceRequired {
                encoded_at_least,
                known_length,
                ..
            } => self.allowance(i, encoded_at_least, known_length),
            HttpReply::Chunk {
                offset, data_b64, ..
            } => self.chunk_received(i, offset, data_b64),
            HttpReply::Complete {
                encoded, decoded, ..
            } => self.complete_received(i, encoded, decoded),
            HttpReply::CommandRejected { .. } => unreachable!("handled before terminal replies"),
        }
    }

    fn upload_ready(&mut self, i: usize, offset: u64, max_bytes: u32) -> bool {
        let a = &mut self.attempts[i];
        if a.state != ReceiveState::Uploading {
            return false;
        }
        let Some(u) = &mut a.upload else {
            return false;
        };
        if u.window.is_some()
            || offset != u.offset as u64
            || max_bytes == 0
            || max_bytes as usize > CHUNK_BYTES
        {
            return false;
        }
        u.window = Some(max_bytes as usize);
        true
    }

    fn head_received(&mut self, i: usize, head: ResponseHead) -> bool {
        let a = &mut self.attempts[i];
        if a.state == ReceiveState::Reusing {
            if a.replay_head || !a.verified.as_ref().is_some_and(|v| v.head() == &head) {
                return false;
            }
            a.replay_head = true;
            return true;
        }
        if a.state != ReceiveState::WaitingHead || !a.committed {
            return false;
        }
        if head
            .encoded_length
            .is_some_and(|n| n > self.limits.encoded_bytes as u64)
        {
            return false;
        }
        let Ok(lease) = self.pending.reserve(head_bytes(&head)) else {
            return false;
        };
        a.state = if head.has_body && head.encoded_length.is_some_and(|n| n > AUTO_BYTES) {
            ReceiveState::SizeNotice
        } else {
            ReceiveState::Ready
        };
        a.head = Some(head);
        a.head_lease = Some(lease);
        true
    }

    fn allowance(&mut self, i: usize, encoded_at_least: u64, known_length: Option<u64>) -> bool {
        let a = &mut self.attempts[i];
        if !matches!(
            a.state,
            ReceiveState::Receiving
                | ReceiveState::Ready
                | ReceiveState::SizeNotice
                | ReceiveState::Skipped
        ) {
            return false;
        }
        let Some(head) = &a.head else {
            return false;
        };
        if known_length != head.encoded_length
            || encoded_at_least <= AUTO_BYTES
            || (known_length.is_some_and(|n| n != encoded_at_least))
        {
            return false;
        }
        if head.encoded_length.is_none() {
            if a.offset != AUTO_BYTES || a.credit.as_ref().is_none_or(|c| c.max != 1) {
                return false;
            }
            self.reserved -= a.settle();
        }
        // A queued known-size notice can follow Allow+Credit. It does not revoke either.
        if a.state == ReceiveState::Skipped && a.credit.is_none() {
            a.discard();
        }
        if a.state != ReceiveState::Skipped && !a.allowed {
            a.state = ReceiveState::SizeNotice;
        }
        true
    }

    fn chunk_received(&mut self, i: usize, offset: u64, data_b64: String) -> bool {
        let a = &mut self.attempts[i];
        if !matches!(a.state, ReceiveState::Receiving | ReceiveState::Skipped) {
            return false;
        }
        let Some(c) = &a.credit else {
            return false;
        };
        let Ok(_scratch) = self.pending.reserve(CHUNK_BYTES) else {
            return false;
        };
        let Ok(bytes) = decode_chunk(&data_b64) else {
            return false;
        };
        let Some(end) = offset.checked_add(bytes.len() as u64) else {
            return false;
        };
        let Some(head) = &a.head else {
            return false;
        };
        if bytes.is_empty()
            || bytes.len() > c.max
            || offset != a.offset
            || c.reserved == 0
            || end > self.limits.encoded_bytes as u64
            || head.encoded_length.is_some_and(|n| end > n)
            || (!head.has_body)
            || (!a.allowed && end > AUTO_BYTES)
        {
            return false;
        }
        if a.state != ReceiveState::Skipped
            && append(a, &bytes, &self.pending, self.limits.encoded_bytes).is_err()
        {
            return false;
        }
        self.reserved -= a.settle();
        self.received += bytes.len() as u64;
        a.offset = end;
        if a.state != ReceiveState::Skipped {
            a.state = ReceiveState::Ready;
        } else {
            a.discard();
        }
        true
    }

    fn complete_received(
        &mut self,
        i: usize,
        encoded: tasty_ipc::html_http::BodyIdentity,
        decoded: tasty_ipc::html_http::BodyIdentity,
    ) -> bool {
        let a = &mut self.attempts[i];
        if a.state == ReceiveState::Reusing {
            if !a.replay_head
                || !a.verified.as_ref().is_some_and(|v| {
                    v.encoded_identity() == encoded && v.decoded_identity() == decoded
                })
            {
                return false;
            }
            a.state = ReceiveState::Verified;
            return true;
        }
        if a.state == ReceiveState::Skipped {
            self.reserved -= a.settle();
            a.discard();
            return true;
        }
        if !matches!(a.state, ReceiveState::Ready | ReceiveState::Receiving)
            || encoded.bytes != a.offset
            || decoded.bytes > self.limits.decoded_bytes as u64
            || a.head
                .as_ref()
                .is_none_or(|h| h.encoded_length.is_some_and(|n| n != encoded.bytes))
        {
            return false;
        }
        let Ok(slot) = self.slots.reserve(1) else {
            return false;
        };
        let Ok(body_lease) = a
            .body_lease
            .take()
            .map_or_else(|| self.pending.reserve(0), Ok)
        else {
            return false;
        };
        let Some(head_lease) = a.head_lease.take() else {
            return false;
        };
        let Some(original_head) = a.head.as_ref() else {
            return false;
        };
        // The retained Head remains needed for serial fencing and replay. Charge the
        // worker's metadata clone separately before handing it to the external pool.
        let Ok(mut worker_head) = self.pending.reserve(head_bytes(original_head)) else {
            return false;
        };
        let head = original_head.clone();
        if worker_head.retain(head_bytes(&head)).is_err() {
            return false;
        }
        a.head_lease = Some(head_lease);
        self.reserved -= a.settle();
        let (sender, receiver) = mpsc::sync_channel(1);
        let Some(owner) = &a.owner else {
            return false;
        };
        a.job = Some(VerifyJob {
            work: Some(worker::Work {
                request: a.request.as_ref().expect("live attempt request").clone(),
                head,
                encoded: Arc::new(std::mem::take(&mut a.body)),
                identities: (encoded, decoded),
                budget: self.verified_budget.clone(),
                decoded_limit: self.limits.decoded_bytes,
                _pending: body_lease,
                _head: worker_head,
            }),
            sender: Some(sender),
            cancel: a.cancel.clone(),
            epoch: self.epoch.clone(),
            owner: Arc::downgrade(owner),
            slot: Some(Arc::new(slot)),
            wake: self.wake.clone(),
        });
        a.completion = Some((encoded, decoded));
        a.result = Some(receiver);
        a.state = ReceiveState::Verifying;
        true
    }
}
fn append(
    a: &mut Attempt,
    bytes: &[u8],
    budget: &PendingBudget,
    limit: usize,
) -> Result<(), ClientError> {
    let end = a
        .body
        .len()
        .checked_add(bytes.len())
        .ok_or(ClientError::Quota)?;
    if end > a.body.capacity() {
        let capacity = end.max(a.body.capacity().saturating_mul(2).min(limit));
        let mut lease = budget.reserve(capacity)?;
        let mut body = Vec::new();
        body.try_reserve_exact(capacity)
            .map_err(|_| ClientError::Quota)?;
        if body.capacity() > limit {
            return Err(ClientError::Quota);
        }
        if body.capacity() > capacity {
            lease.grow(body.capacity() - capacity)?;
        }
        body.extend_from_slice(&a.body);
        a.body = body;
        a.body_lease = Some(lease);
    }
    a.body.extend_from_slice(bytes);
    Ok(())
}

fn head_bytes(head: &ResponseHead) -> usize {
    head.headers.iter().fold(
        head.logical_url.capacity().saturating_add(
            head.headers
                .capacity()
                .saturating_mul(std::mem::size_of::<tasty_ipc::html_http::Header>()),
        ),
        |n, h| {
            n.saturating_add(h.name.capacity())
                .saturating_add(h.value.capacity())
        },
    )
}
