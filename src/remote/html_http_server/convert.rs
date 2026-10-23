use super::*;
pub(super) fn response(id: core::ResponseId) -> wire::ResponseId {
    wire::ResponseId {
        request: id.request,
        source_generation: id.source_generation,
        serial: id.serial,
    }
}
pub(super) fn core_response(id: wire::ResponseId) -> core::ResponseId {
    core::ResponseId {
        request: id.request,
        source_generation: id.source_generation,
        serial: id.serial,
    }
}
pub(super) fn identity(id: &core::BodyIdentity) -> core::Result<wire::BodyIdentity> {
    Ok(wire::BodyIdentity {
        bytes: u64::try_from(id.bytes).map_err(|_| core::Error::Invalid("body length overflow"))?,
        sha256: id.sha256,
    })
}
pub(super) fn head(h: &core::ResponseHead) -> wire::ResponseHead {
    wire::ResponseHead {
        id: response(h.id),
        logical_url: h.logical_url.clone(),
        status: h.status,
        headers: h
            .headers
            .iter()
            .map(|h| wire::Header {
                name: h.name.clone(),
                value: h.value.clone(),
            })
            .collect(),
        encoded_length: h.encoded_length,
        has_body: h.has_body,
    }
}
pub(super) fn complete(r: &core::StoredResponse) -> core::Result<wire::HttpReply> {
    Ok(wire::HttpReply::Complete {
        response: response(r.head.id),
        encoded: identity(r.encoded.identity())?,
        decoded: identity(r.decoded.identity())?,
    })
}
/// Exhaustive category mapping; never copy upstream errors, URLs or header values into reasons.
pub fn failure_reply(
    id: TransferId,
    source_generation: u64,
    response: Option<wire::ResponseId>,
    error: &core::Error,
) -> wire::HttpReply {
    use super::core::Error as E;
    use wire::FailureCode as F;
    let code = match error {
        E::Closed => F::Closed,
        E::Stale => F::Stale,
        E::Capacity => F::Capacity,
        E::InProgress => F::InProgress,
        E::AlreadyUsed => F::AlreadyUsed,
        E::DuplicateMismatch => F::DuplicateMismatch,
        E::Invalid(_) => F::Invalid,
        E::Quota(_) => F::Quota,
        E::Network => F::Network,
        E::Timeout => F::Timeout,
        E::Cancelled => F::Cancelled,
        E::UnsupportedScheme => F::UnsupportedScheme,
        E::UnsupportedUpgrade => F::UnsupportedUpgrade,
        E::UnsupportedStreaming => F::UnsupportedStreaming,
        E::UnsupportedEncoding => F::UnsupportedEncoding,
        E::Worker => F::Worker,
    };
    // Category names are fixed local strings; sensitive Invalid/Quota details are not echoed.
    wire::HttpReply::Failure {
        id,
        source_generation,
        response,
        code,
        reason: format!("HTTP request failed: {code:?}"),
    }
}
pub(super) fn encode(bytes: &[u8]) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for c in bytes.chunks(3) {
        out.push(TABLE[(c[0] >> 2) as usize] as char);
        out.push(
            TABLE[(((c[0] & 3) << 4) | (c.get(1).copied().unwrap_or(0) >> 4)) as usize] as char,
        );
        out.push(if c.len() > 1 {
            TABLE[(((c[1] & 15) << 2) | (c.get(2).copied().unwrap_or(0) >> 6)) as usize] as char
        } else {
            '='
        });
        out.push(if c.len() > 2 {
            TABLE[(c[2] & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
