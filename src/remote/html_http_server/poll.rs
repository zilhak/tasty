use super::*;
pub(super) fn owner(owner: &mut Owner) -> core::Result<Option<StreamFrame>> {
    let ids: Vec<_> = owner.requests.keys().copied().collect();
    if ids.is_empty() {
        return Ok(None);
    }
    for step in 0..ids.len() {
        let index = (owner.cursor + step) % ids.len();
        let id = ids[index];
        let slot = owner.requests.get_mut(&id).ok_or(core::Error::Worker)?;
        let reply = next(
            slot,
            owner.token.source_generation(),
            &mut owner.transferred,
        )?;
        if let Some(reply) = reply {
            owner.cursor = (index + 1) % ids.len();
            return match reply.frame() {
                Ok(frame) => Ok(Some(frame)),
                Err(_) => fail_slot(
                    slot,
                    owner.token.source_generation(),
                    &core::Error::Invalid("outbound HTTP frame"),
                )
                .frame()
                .map(Some)
                .map_err(core::Error::Invalid),
            };
        }
    }
    Ok(None)
}
fn next(
    slot: &mut Slot,
    generation: u64,
    total: &mut u64,
) -> core::Result<Option<wire::HttpReply>> {
    let id = slot.begin.id();
    match &mut slot.state {
        State::Upload { offset, ready, .. } if *ready => {
            *ready = false;
            let wire::HttpRequest::Begin { upload, .. } = &slot.begin else {
                return Err(core::Error::Worker);
            };
            // Empty uploads still receive one legal ready window and may commit immediately.
            let max_bytes = upload
                .bytes
                .saturating_sub(*offset)
                .min(wire::CHUNK_BYTES as u64)
                .max(1) as u32;
            Ok(Some(wire::HttpReply::UploadReady {
                id,
                source_generation: generation,
                offset: *offset,
                max_bytes,
            }))
        }
        State::Running(r) => match r.operation.events.try_recv() {
            Ok(event) => event_reply(slot, event, generation, total).map(Some),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty) => Ok(None),
            Err(tokio::sync::mpsc::error::TryRecvError::Disconnected) => {
                let reply = failure_reply(id, generation, r.response, &core::Error::Worker);
                slot.state = State::Terminal(None);
                Ok(Some(reply))
            }
        },
        State::Replay {
            response: r,
            head: pending,
        } => {
            if *pending {
                *pending = false;
                Ok(Some(wire::HttpReply::Head {
                    head: head(&r.head),
                }))
            } else {
                let reply = complete(r)?;
                let id = response(r.head.id);
                slot.state = State::Complete(id);
                Ok(Some(reply))
            }
        }
        State::Terminal(reply) => Ok(reply.take()),
        _ => Ok(None),
    }
}
fn event_reply(
    slot: &mut Slot,
    event: core::Event,
    generation: u64,
    total: &mut u64,
) -> core::Result<wire::HttpReply> {
    let State::Running(r) = &mut slot.state else {
        return Err(core::Error::Worker);
    };
    match event {
        core::Event::Head(h) => {
            let id = response(h.id);
            if h.id.request != slot.begin.id() || h.id.source_generation != generation {
                return Err(core::Error::Stale);
            }
            r.response = Some(id);
            r.known_length = h.encoded_length;
            Ok(wire::HttpReply::Head { head: head(&h) })
        }
        core::Event::AllowanceRequired {
            id,
            encoded_at_least,
            known_length,
        } => {
            if r.response != Some(response(id)) {
                return Err(core::Error::Stale);
            }
            // Unknown-size lookahead consumed its credit without a chunk. Known-size allowance
            // precedes credit consumption, so an already granted Allow remains outstanding.
            if known_length.is_none() {
                r.outstanding = None;
            }
            Ok(wire::HttpReply::AllowanceRequired {
                response: response(id),
                encoded_at_least,
                known_length,
            })
        }
        core::Event::Chunk { id, offset, bytes } => {
            if r.response != Some(response(id))
                || offset != r.offset
                || r.outstanding.is_none_or(|n| bytes.len() > n as usize)
            {
                return Err(core::Error::Invalid("core chunk credit"));
            }
            r.offset = r
                .offset
                .checked_add(bytes.len() as u64)
                .ok_or(core::Error::Quota("response byte overflow"))?;
            *total = total
                .checked_add(bytes.len() as u64)
                .ok_or(core::Error::Quota("connection byte overflow"))?;
            r.outstanding = None;
            Ok(wire::HttpReply::Chunk {
                response: response(id),
                offset,
                data_b64: encode(&bytes),
            })
        }
        core::Event::Complete(stored) => {
            if r.response != Some(response(stored.head.id))
                || stored.encoded.identity().bytes as u64 != r.offset
            {
                return Err(core::Error::Invalid("core completion identity"));
            }
            let reply = complete(&stored)?;
            slot.state = State::Complete(response(stored.head.id));
            Ok(reply)
        }
        core::Event::Failed { id, error } => {
            if id.request != slot.begin.id()
                || id.source_generation != generation
                || r.response.is_some_and(|r| r != response(id))
            {
                return Err(core::Error::Stale);
            }
            let reply = failure_reply(id.request, generation, Some(response(id)), &error);
            slot.state = State::Terminal(None);
            Ok(reply)
        }
    }
}

fn fail_slot(slot: &mut Slot, generation: u64, error: &core::Error) -> wire::HttpReply {
    let response = match &slot.state {
        State::Running(r) => {
            r.operation.cancel();
            r.response
        }
        State::Complete(r) => Some(*r),
        State::Replay { response: r, .. } => Some(response(r.head.id)),
        _ => None,
    };
    let reply = failure_reply(slot.begin.id(), generation, response, error);
    slot.state = State::Terminal(None);
    reply
}
