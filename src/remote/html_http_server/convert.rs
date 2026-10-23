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
/// Terminal result only: the exact admitted attempt must already have ended.
/// Synchronous command errors use rejection_reply, preserving any accepted response.
pub fn failure_reply(
    id: TransferId,
    source_generation: u64,
    response: Option<wire::ResponseId>,
    error: &core::Error,
) -> wire::HttpReply {
    let (code, reason) = error_details(error);
    wire::HttpReply::Failure {
        id,
        source_generation,
        response,
        code,
        reason: reason.into(),
    }
}
/// Correlates the rejected command, never inferring the current accepted response.
/// The host must revalidate captured authority before publishing this reply.
pub fn rejection_reply(request: &wire::HttpRequest, error: &core::Error) -> wire::HttpReply {
    let response = match request {
        wire::HttpRequest::Credit { response, .. }
        | wire::HttpRequest::Reuse { response }
        | wire::HttpRequest::Release { response }
            if response.serial != 0 =>
        {
            Some(*response)
        }
        _ => None,
    };
    let (code, reason) = error_details(error);
    wire::HttpReply::CommandRejected {
        id: request.id(),
        source_generation: request.source_generation(),
        command: request.command_kind(),
        response,
        code,
        reason: reason.into(),
    }
}
fn error_details(error: &core::Error) -> (wire::FailureCode, &'static str) {
    use super::core::Error as E;
    use wire::FailureCode as F;
    // Keep fixed category reasons; Invalid/Quota details can contain sensitive input.
    match error {
        E::Closed => (F::Closed, "HTTP request failed: Closed"),
        E::Stale => (F::Stale, "HTTP request failed: Stale"),
        E::Capacity => (F::Capacity, "HTTP request failed: Capacity"),
        E::InProgress => (F::InProgress, "HTTP request failed: InProgress"),
        E::AlreadyUsed => (F::AlreadyUsed, "HTTP request failed: AlreadyUsed"),
        E::DuplicateMismatch => (
            F::DuplicateMismatch,
            "HTTP request failed: DuplicateMismatch",
        ),
        E::Invalid(_) => (F::Invalid, "HTTP request failed: Invalid"),
        E::Quota(_) => (F::Quota, "HTTP request failed: Quota"),
        E::Network => (F::Network, "HTTP request failed: Network"),
        E::Timeout => (F::Timeout, "HTTP request failed: Timeout"),
        E::Cancelled => (F::Cancelled, "HTTP request failed: Cancelled"),
        E::UnsupportedScheme => (
            F::UnsupportedScheme,
            "HTTP request failed: UnsupportedScheme",
        ),
        E::UnsupportedUpgrade => (
            F::UnsupportedUpgrade,
            "HTTP request failed: UnsupportedUpgrade",
        ),
        E::UnsupportedStreaming => (
            F::UnsupportedStreaming,
            "HTTP request failed: UnsupportedStreaming",
        ),
        E::UnsupportedEncoding => (
            F::UnsupportedEncoding,
            "HTTP request failed: UnsupportedEncoding",
        ),
        E::Worker => (F::Worker, "HTTP request failed: Worker"),
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
