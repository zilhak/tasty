use super::*;
impl Server {
    /// Errors are sanitized core categories; the host may send failure_reply through its bounded
    /// connection queue. A failed command never silently starts or retries an upstream request.
    pub fn handle(
        &mut self,
        token: &OwnerToken,
        current: impl FnOnce(&OwnerToken) -> bool,
        request: wire::HttpRequest,
    ) -> core::Result<()> {
        let i = self.current(token, current)?;
        request.validate().map_err(core::Error::Invalid)?;
        let owner = &mut self.documents[i];
        let id = request.id();
        if !owner.matches(id, request.source_generation()) {
            return Err(core::Error::Stale);
        }
        match request {
            begin @ wire::HttpRequest::Begin { .. } => begin_request(
                owner,
                &self.backend,
                self.max_requests,
                self.max_upload,
                self.header_limit,
                begin,
            ),
            wire::HttpRequest::Upload {
                offset, data_b64, ..
            } => upload(owner, id, offset, &data_b64),
            wire::HttpRequest::Commit { .. } => commit(owner, id),
            wire::HttpRequest::Credit {
                response,
                offset,
                max_bytes,
                allow_large,
            } => credit(owner, response, offset, max_bytes, allow_large),
            wire::HttpRequest::Reuse { response } => {
                let slot = owner.requests.get_mut(&id).ok_or(core::Error::Stale)?;
                if !matches!(slot.state,State::Complete(r) if r==response) {
                    return Err(core::Error::Stale);
                }
                let stored = owner.document.reuse(core_response(response))?;
                slot.state = State::Replay {
                    response: stored,
                    head: true,
                };
                Ok(())
            }
            wire::HttpRequest::Release { response } => {
                let slot = owner.requests.get_mut(&id).ok_or(core::Error::Stale)?;
                if !matches!(slot.state,State::Complete(r) if r==response) {
                    return Err(core::Error::Stale);
                }
                owner.document.release(core_response(response))?;
                slot.state = State::Terminal(None);
                Ok(())
            }
            wire::HttpRequest::Cancel { .. } => cancel(owner, id),
            wire::HttpRequest::Stop { entry, .. } => {
                if entry != owner.token.entry() {
                    return Err(core::Error::Stale);
                }
                owner.stopped = true;
                let pending: Vec<_> = owner.requests.keys().copied().collect();
                for id in pending {
                    cancel(owner, id)?;
                }
                Ok(())
            }
            wire::HttpRequest::Close { entry, .. } => {
                if entry != owner.token.entry() {
                    return Err(core::Error::Stale);
                }
                self.retire(token);
                Ok(())
            }
        }
    }
}
fn begin_request(
    owner: &mut Owner,
    backend: &core::Backend,
    max_requests: usize,
    max_upload: usize,
    header_limit: usize,
    begin: wire::HttpRequest,
) -> core::Result<()> {
    let id = begin.id();
    let fingerprint = super::fingerprint::begin(&begin, header_limit)?;
    if let Some(slot) = owner.requests.get(&id) {
        if slot.fingerprint != fingerprint {
            return Err(core::Error::DuplicateMismatch);
        }
        return Err(match slot.state {
            State::Upload { .. } | State::Running(_) => core::Error::InProgress,
            _ => core::Error::AlreadyUsed,
        });
    }
    if owner.stopped {
        return Err(core::Error::Closed);
    }
    if owner.requests.len() >= max_requests {
        return Err(core::Error::Capacity);
    }
    let wire::HttpRequest::Begin {
        logical_url,
        upload,
        allow_remote_content,
        ..
    } = &begin
    else {
        return Err(core::Error::Invalid("begin command"));
    };
    if *allow_remote_content != owner.token.0.allow_remote_content {
        return Err(core::Error::Invalid("document origin policy changed"));
    }
    if owner.requests.is_empty()
        && (id != owner.token.entry() || logical_url != &owner.token.0.entry_url)
    {
        return Err(core::Error::Invalid("authoritative entry URL"));
    }
    // The declared complete upload is checked before retaining even its first frame.
    if upload.checked_len().map_err(core::Error::Invalid)? > max_upload {
        return Err(core::Error::Quota("upload bytes"));
    }
    owner.requests.insert(
        id,
        Slot {
            begin,
            fingerprint,
            state: State::Upload {
                builder: backend.upload(),
                offset: 0,
                ready: true,
            },
        },
    );
    Ok(())
}
fn upload(owner: &mut Owner, id: TransferId, offset: u64, data: &str) -> core::Result<()> {
    let slot = owner.requests.get_mut(&id).ok_or(core::Error::Stale)?;
    let State::Upload {
        builder,
        offset: next,
        ready,
    } = &mut slot.state
    else {
        return Err(core::Error::AlreadyUsed);
    };
    let wire::HttpRequest::Begin { upload, .. } = &slot.begin else {
        return Err(core::Error::Worker);
    };
    if *ready || offset != *next {
        return Err(core::Error::Invalid("uncredited upload or offset"));
    }
    let bytes = wire::decode_chunk(data).map_err(core::Error::Invalid)?;
    let end = offset
        .checked_add(bytes.len() as u64)
        .ok_or(core::Error::Invalid("upload overflow"))?;
    if bytes.is_empty() || end > upload.bytes {
        return Err(core::Error::Invalid("upload length"));
    }
    builder.append(offset, &bytes)?;
    *next = end;
    *ready = end < upload.bytes;
    Ok(())
}
fn commit(owner: &mut Owner, id: TransferId) -> core::Result<()> {
    let slot = owner.requests.get_mut(&id).ok_or(core::Error::Stale)?;
    let wire::HttpRequest::Begin {
        source_generation,
        method,
        logical_url,
        headers,
        upload,
        ..
    } = &slot.begin
    else {
        return Err(core::Error::Worker);
    };
    match &slot.state {
        State::Upload { offset, ready, .. }
            if offset == &upload.bytes && (!ready || upload.bytes == 0) => {}
        State::Upload { .. } => return Err(core::Error::Invalid("incomplete upload")),
        State::Running(_) => return Err(core::Error::InProgress),
        _ => return Err(core::Error::AlreadyUsed),
    }
    let State::Upload { builder, .. } = std::mem::replace(&mut slot.state, State::Terminal(None))
    else {
        return Err(core::Error::Worker);
    };
    let frozen = builder.finish();
    if identity(frozen.identity())? != *upload {
        return Err(core::Error::Invalid("upload identity mismatch"));
    }
    let request = core::Request {
        id,
        source_generation: *source_generation,
        method: method.clone(),
        url: logical_url.clone(),
        headers: headers
            .iter()
            .map(|h| core::Header {
                name: h.name.clone(),
                value: h.value.clone(),
            })
            .collect(),
        upload: frozen,
    };
    match owner.document.open(request)? {
        core::Opened::Fresh(operation) => {
            slot.state = State::Running(Running {
                operation,
                response: None,
                offset: 0,
                outstanding: None,
                allowed: false,
                known_length: None,
            })
        }
        core::Opened::Reused(_) => return Err(core::Error::AlreadyUsed),
    }
    Ok(())
}
fn credit(
    owner: &mut Owner,
    response: wire::ResponseId,
    offset: u64,
    max_bytes: u32,
    allow_large: bool,
) -> core::Result<()> {
    let slot = owner
        .requests
        .get_mut(&response.request)
        .ok_or(core::Error::Stale)?;
    let State::Running(r) = &mut slot.state else {
        return Err(core::Error::Stale);
    };
    if r.response != Some(response) || r.offset != offset {
        return Err(core::Error::Stale);
    }
    if r.outstanding.is_some() {
        return Err(core::Error::InProgress);
    }
    if !r.allowed && !allow_large {
        if r.known_length.is_some_and(|n| n > wire::AUTO_BYTES) {
            return Err(core::Error::Invalid("response Allow required"));
        }
        // At the threshold a positive EOF probe is necessary, but cannot expose byte100001.
        if offset < wire::AUTO_BYTES && u64::from(max_bytes) > wire::AUTO_BYTES - offset {
            return Err(core::Error::Invalid("automatic response window"));
        }
    }
    r.operation.credit(core::Credit {
        response: core_response(response),
        offset,
        max_bytes: max_bytes as usize,
        allow_large,
    })?;
    r.outstanding = Some(max_bytes);
    r.allowed |= allow_large;
    Ok(())
}
fn cancel(owner: &mut Owner, id: TransferId) -> core::Result<()> {
    let slot = owner.requests.get_mut(&id).ok_or(core::Error::Stale)?;
    let response = match &slot.state {
        State::Upload { .. } => None,
        State::Running(r) => {
            owner.document.cancel(id)?;
            r.operation.cancel();
            r.response
        }
        // Stop/Cancel preserve completed retention and are idempotent for a known tombstone.
        _ => return Ok(()),
    };
    // The core task keeps its permit through actual termination after this nonblocking drop.
    slot.state = State::Terminal(Some(wire::HttpReply::Cancelled {
        id,
        source_generation: owner.token.source_generation(),
        response,
    }));
    Ok(())
}
