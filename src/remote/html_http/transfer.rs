//! One operation consumes one upload and never retries or follows redirects.
use super::*;
use tasty_ipc::html_transfer::{AUTO_BYTES, CHUNK_BYTES};
use tokio::sync::{mpsc, watch};

pub(super) async fn send(
    tx: &mpsc::Sender<Event>,
    event: Event,
    cancel: &Cancellation,
) -> Result<()> {
    let mut stop = cancel.tx.subscribe();
    if cancel.flag.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let result = tokio::select! {biased;_=stop.changed()=>Err(Error::Cancelled),sent=tx.send(event)=>sent.map_err(|_|Error::Cancelled)};
    if result.is_ok() {
        (cancel.wake)();
    }
    result
}
fn network(e: reqwest::Error) -> Error {
    if e.is_timeout() {
        Error::Timeout
    } else {
        Error::Network
    }
}
async fn next_chunk(
    response: &mut reqwest::Response,
    stop: &mut watch::Receiver<bool>,
    cancel: &Cancellation,
    budget: &mut std::time::Duration,
) -> Result<Option<Vec<u8>>> {
    loop {
        if cancel.flag.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let start = std::time::Instant::now();
        let result = tokio::select! {biased;_=stop.changed()=>Err(Error::Cancelled),chunk=tokio::time::timeout(*budget,response.chunk())=>chunk.map_err(|_|Error::Timeout)?.map(|c|c.map(|c|c.to_vec())).map_err(network)};
        *budget = budget.saturating_sub(start.elapsed());
        match result? {
            Some(bytes) if bytes.is_empty() => continue,
            result => return Ok(result),
        }
    }
}
#[expect(
    clippy::too_many_arguments,
    reason = "one owned request and its independently owned flow-control endpoints"
)]
pub(super) async fn run(
    inner: &DocumentInner,
    id: ResponseId,
    method: reqwest::Method,
    url: reqwest::Url,
    headers: Vec<Header>,
    upload: Upload,
    mut credits: mpsc::Receiver<Credit>,
    events: &mpsc::Sender<Event>,
    cancel: &Cancellation,
) -> Result<Arc<StoredResponse>> {
    let mut stop = cancel.tx.subscribe();
    if cancel.flag.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    let response_has_body = method != reqwest::Method::HEAD;
    let Upload { bytes, lease, .. } = upload;
    let request = inner
        .backend
        .client
        .request(method, url.clone())
        .headers(super::headers::request(headers)?)
        .body(bytes);
    let mut budget = inner.backend.limits.network_time;
    let started = std::time::Instant::now();
    let mut response = tokio::select! {biased;_=stop.changed()=>return Err(Error::Cancelled),r=tokio::time::timeout(budget,request.send())=>r.map_err(|_|Error::Timeout)?.map_err(network)?};
    budget = budget.saturating_sub(started.elapsed());
    // The transport can own upload bytes until response headers arrive.
    let _upload_lease = lease;
    let (head, encoding) = metadata(
        &response,
        id,
        url.to_string(),
        response_has_body,
        &inner.backend.limits,
    )?;
    send(events, Event::Head(head.clone()), cancel).await?;
    let mut encoded = Vec::new();
    let mut lease = body::Lease::new(inner.backend.quota.clone());
    let mut pending = Vec::new();
    let mut position = 0usize;
    let mut offset = 0u64;
    let mut allowed = false;
    let mut eof = !head.has_body;
    if !eof && head.encoded_length.is_some_and(|n| n > AUTO_BYTES) {
        send(
            events,
            Event::AllowanceRequired {
                id,
                encoded_at_least: head.encoded_length.unwrap_or(0),
                known_length: head.encoded_length,
            },
            cancel,
        )
        .await?;
    }
    while !eof {
        if cancel.flag.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let credit = tokio::select! {biased;_=stop.changed()=>return Err(Error::Cancelled),c=credits.recv()=>c.ok_or(Error::Cancelled)?};
        if credit.response != id
            || credit.offset != offset
            || credit.max_bytes == 0
            || credit.max_bytes > CHUNK_BYTES
        {
            return Err(Error::Invalid("response credit identity/offset/length"));
        }
        allowed |= credit.allow_large;
        if !allowed && head.encoded_length.is_some_and(|n| n > AUTO_BYTES) {
            continue;
        }
        if position == pending.len() {
            match next_chunk(&mut response, &mut stop, cancel, &mut budget).await? {
                Some(bytes) => {
                    // Bound application-retained lookahead, separately from HTTP library buffers.
                    if bytes.len()
                        > inner
                            .backend
                            .limits
                            .encoded_bytes
                            .saturating_sub(encoded.len())
                    {
                        return Err(Error::Quota("encoded bytes"));
                    }
                    lease.grow(bytes.len())?;
                    pending = bytes;
                    position = 0;
                }
                None => {
                    eof = true;
                    continue;
                }
            }
        }
        if !allowed && offset == AUTO_BYTES {
            send(
                events,
                Event::AllowanceRequired {
                    id,
                    encoded_at_least: AUTO_BYTES + 1,
                    known_length: None,
                },
                cancel,
            )
            .await?;
            continue;
        }
        let consent = if allowed {
            usize::MAX
        } else {
            (AUTO_BYTES - offset) as usize
        };
        let n = credit.max_bytes.min(pending.len() - position).min(consent);
        let bytes = pending[position..position + n].to_vec();
        send(
            events,
            Event::Chunk {
                id,
                offset,
                bytes: bytes.clone(),
            },
            cancel,
        )
        .await?;
        encoded.extend_from_slice(&bytes);
        position += n;
        offset += n as u64;
    }
    // A cached frame could be at EOF only after all its bytes have been emitted.
    let encoded = Body::from_vec(encoded, lease);
    let body_for_decode = encoded.clone();
    let quota = inner.backend.quota.clone();
    let limit = inner.backend.limits.decoded_bytes;
    let flag = cancel.flag.clone();
    let decoded = tokio::task::spawn_blocking(move || {
        body::decode(body_for_decode, &encoding, quota, limit, &flag)
    })
    .await
    .map_err(|_| Error::Worker)??;
    if cancel.flag.load(Ordering::Acquire) {
        return Err(Error::Cancelled);
    }
    Ok(Arc::new(StoredResponse {
        head,
        encoded,
        decoded,
    }))
}

fn metadata(
    response: &reqwest::Response,
    id: ResponseId,
    logical_url: String,
    response_has_body: bool,
    limits: &Limits,
) -> Result<(ResponseHead, String)> {
    if response
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|h| h.to_str().ok())
        .is_some_and(|h| {
            h.split(';')
                .next()
                .is_some_and(|h| h.trim().eq_ignore_ascii_case("text/event-stream"))
        })
    {
        return Err(Error::UnsupportedStreaming);
    }
    if response.status() == reqwest::StatusCode::SWITCHING_PROTOCOLS {
        return Err(Error::UnsupportedUpgrade);
    }
    let head = ResponseHead {
        id,
        logical_url,
        status: response.status().as_u16(),
        headers: super::headers::response(response.headers(), limits.header_bytes)?,
        encoded_length: response.content_length(),
        has_body: response_has_body
            && response.status().as_u16() != 204
            && response.status().as_u16() != 304,
    };
    if response
        .headers()
        .get_all(reqwest::header::CONTENT_ENCODING)
        .iter()
        .count()
        > 1
    {
        return Err(Error::UnsupportedEncoding);
    }
    let mut encoding = response
        .headers()
        .get(reqwest::header::CONTENT_ENCODING)
        .map(|h| {
            h.to_str()
                .map(str::to_ascii_lowercase)
                .map_err(|_| Error::UnsupportedEncoding)
        })
        .transpose()?
        .unwrap_or_default();
    if !head.has_body {
        encoding.clear();
    }
    if !["", "identity", "gzip", "deflate"].contains(&encoding.as_str()) {
        return Err(Error::UnsupportedEncoding);
    }
    if head.has_body
        && head
            .encoded_length
            .is_some_and(|n| n > limits.encoded_bytes as u64)
    {
        return Err(Error::Quota("encoded bytes"));
    }
    Ok((head, encoding))
}
