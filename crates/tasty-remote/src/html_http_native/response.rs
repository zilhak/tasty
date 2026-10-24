use super::{NativeError, NativeLimits, Result};
use crate::html_http_body::VerifiedBody;
use std::sync::Arc;
use tasty_ipc::html_http::{BodyIdentity, CHUNK_BYTES, ResponseId};
use tokio::io::{AsyncWrite, AsyncWriteExt};

/// The caller retains the current native waiter and chooses its exact already-scanned response.
/// This function cannot create a new Begin or retry a POST. The Arc retains body budget leases
/// through actual writes; response identity includes all transfer/source/serial fields.
pub async fn write_response<S: AsyncWrite + Unpin>(
    io: &mut S,
    body: Arc<VerifiedBody>,
    expected: ResponseId,
    method: &str,
    upload: BodyIdentity,
    limits: NativeLimits,
    current: &(dyn Fn() -> bool + Send + Sync),
) -> Result<()> {
    if !body.matches_native(expected, method, upload) {
        return Err(NativeError::Identity);
    }
    if !current() {
        return Err(NativeError::Closed);
    }
    if limits.io_timeout.is_zero() {
        return Err(NativeError::Timeout);
    }
    tokio::time::timeout(limits.io_timeout, write(io, &body, current))
        .await
        .map_err(|_| NativeError::Timeout)?
}
fn headers(body: &VerifiedBody) -> Result<bool> {
    let mut length = false;
    for header in &body.head().headers {
        if header.name.eq_ignore_ascii_case("content-length") {
            let value = std::str::from_utf8(&header.value).map_err(|_| NativeError::Invalid)?;
            if value.is_empty() || !value.bytes().all(|b| b.is_ascii_digit()) {
                return Err(NativeError::Invalid);
            }
            let value: u64 = value.parse().map_err(|_| NativeError::Invalid)?;
            if body.head().has_body && value != body.encoded().len() as u64 {
                return Err(NativeError::Identity);
            }
            length = true;
        }
        if [
            "connection",
            "proxy-connection",
            "keep-alive",
            "transfer-encoding",
            "trailer",
            "te",
            "upgrade",
            "proxy-authenticate",
            "proxy-authorization",
        ]
        .iter()
        .any(|name| header.name.eq_ignore_ascii_case(name))
        {
            return Err(NativeError::Invalid);
        }
    }
    Ok(length)
}
async fn part<S: AsyncWrite + Unpin>(
    io: &mut S,
    bytes: &[u8],
    current: &(dyn Fn() -> bool + Send + Sync),
) -> Result<()> {
    if !current() {
        return Err(NativeError::Closed);
    }
    io.write_all(bytes).await.map_err(|_| NativeError::Io)
}
async fn write<S: AsyncWrite + Unpin>(
    io: &mut S,
    body: &VerifiedBody,
    current: &(dyn Fn() -> bool + Send + Sync),
) -> Result<()> {
    // Check all framing before publishing even the status line. Header values and duplicates
    // retain their original bytes; only absent transfer framing is synthesized locally.
    let length = headers(body)?;
    part(
        io,
        format!("HTTP/1.1 {} \r\n", body.head().status).as_bytes(),
        current,
    )
    .await?;
    for header in &body.head().headers {
        part(io, header.name.as_bytes(), current).await?;
        part(io, b": ", current).await?;
        part(io, &header.value, current).await?;
        part(io, b"\r\n", current).await?;
    }
    if !length && body.head().has_body {
        part(
            io,
            format!("Content-Length: {}\r\n", body.encoded().len()).as_bytes(),
            current,
        )
        .await?;
    }
    part(io, b"Connection: close\r\n\r\n", current).await?;
    for chunk in body.encoded().chunks(CHUNK_BYTES) {
        part(io, chunk, current).await?;
    }
    if !current() {
        return Err(NativeError::Closed);
    }
    io.flush().await.map_err(|_| NativeError::Io)
}
