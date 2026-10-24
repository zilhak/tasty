use super::{NativeBudget, NativeError, NativeLimits, Result, budget::Lease};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tasty_ipc::html_http::{
    BodyIdentity, HEADER_BYTES, HEADER_COUNT, Header, METHOD_BYTES, URL_BYTES,
};
use tokio::io::{
    AsyncBufRead, AsyncBufReadExt, AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, BufReader,
};
use url::Url;

/// The endpoint captures this authority after authentication, before accepting a SOCKS route.
pub struct Destination {
    url: Url,
}
impl Destination {
    pub fn new(scheme: &str, host: &str, port: u16) -> Result<Self> {
        if !matches!(scheme, "http" | "https") || port == 0 {
            return Err(NativeError::Destination);
        }
        let host = if host.contains(':') && !host.starts_with('[') {
            format!("[{host}]")
        } else {
            host.to_owned()
        };
        let url = Url::parse(&format!("{scheme}://{host}:{port}/"))
            .map_err(|_| NativeError::Destination)?;
        if url.as_str().len() > URL_BYTES
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(NativeError::Destination);
        }
        Ok(Self { url })
    }
    fn same(&self, url: &Url) -> bool {
        url.scheme() == self.url.scheme()
            && url.host() == self.url.host()
            && url.port_or_known_default() == self.url.port_or_known_default()
            && url.username().is_empty()
            && url.password().is_none()
    }
    fn target(&self, target: &str) -> Result<Url> {
        if target.len() > URL_BYTES || target.contains('#') || target.starts_with("//") {
            return Err(NativeError::Destination);
        }
        let url = if target.starts_with('/') {
            self.url.join(target)
        } else {
            Url::parse(target)
        }
        .map_err(|_| NativeError::Destination)?;
        (self.same(&url) && url.as_str().len() <= URL_BYTES)
            .then_some(url)
            .ok_or(NativeError::Destination)
    }
    fn host(&self, host: &[u8]) -> Result<()> {
        let host = std::str::from_utf8(host).map_err(|_| NativeError::Destination)?;
        let url = Url::parse(&format!("{}://{host}/", self.url.scheme()))
            .map_err(|_| NativeError::Destination)?;
        if !self.same(&url)
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
        {
            return Err(NativeError::Destination);
        }
        Ok(())
    }
}
/// Retain this owner while its upload Arc or metadata are in use; its native-capacity leases
/// are independent of the receive model's provisional and verified-body quotas.
pub struct NativeRequest {
    pub method: String,
    pub logical_url: String,
    pub headers: Vec<Header>,
    pub upload: Arc<Vec<u8>>,
    pub identity: BodyIdentity,
    _metadata: Lease,
    _body: Option<Lease>,
}
struct Head {
    method: String,
    url: String,
    headers: Vec<Header>,
    length: Option<usize>,
    chunked: bool,
    expect: bool,
}
fn framing(
    headers: &[httparse::Header<'_>],
    destination: &Destination,
) -> Result<(Option<usize>, bool, bool)> {
    let (mut host, mut length, mut chunked, mut expect) = (false, None, false, false);
    for h in headers {
        match h.name.to_ascii_lowercase().as_str() {
            "host" => {
                if host {
                    return Err(NativeError::Destination);
                }
                destination.host(h.value)?;
                host = true;
            }
            "content-length" => {
                if length.is_some() || h.value.is_empty() || !h.value.iter().all(u8::is_ascii_digit)
                {
                    return Err(NativeError::Invalid);
                }
                length = Some(
                    std::str::from_utf8(h.value)
                        .map_err(|_| NativeError::Invalid)?
                        .parse()
                        .map_err(|_| NativeError::Quota)?,
                );
            }
            "transfer-encoding" => {
                if chunked || !h.value.eq_ignore_ascii_case(b"chunked") {
                    return Err(NativeError::Invalid);
                }
                chunked = true;
            }
            "expect" => {
                if expect || !h.value.eq_ignore_ascii_case(b"100-continue") {
                    return Err(NativeError::Invalid);
                }
                expect = true;
            }
            "upgrade" | "sec-websocket-key" | "sec-websocket-version" => {
                return Err(NativeError::Upgrade);
            }
            _ => {}
        }
    }
    if !host {
        return Err(NativeError::Destination);
    }
    if chunked && length.is_some() {
        return Err(NativeError::Invalid);
    }
    Ok((length, chunked, expect))
}
fn parse(bytes: &[u8], destination: &Destination) -> Result<Head> {
    let mut slots = [httparse::EMPTY_HEADER; HEADER_COUNT];
    let mut r = httparse::Request::new(&mut slots);
    if !r
        .parse(bytes)
        .map_err(|_| NativeError::Invalid)?
        .is_complete()
        || !matches!(r.version, Some(0 | 1))
    {
        return Err(NativeError::Invalid);
    }
    let method = r
        .method
        .filter(|s| !s.is_empty() && s.len() <= METHOD_BYTES)
        .ok_or(NativeError::Invalid)?;
    if method.eq_ignore_ascii_case("CONNECT") {
        return Err(NativeError::Upgrade);
    }
    let url = destination.target(r.path.ok_or(NativeError::Invalid)?)?;
    let (length, chunked, expect) = framing(r.headers, destination)?;
    let headers = r
        .headers
        .iter()
        .filter(|h| !h.name.eq_ignore_ascii_case("expect"))
        .map(|h| Header {
            name: h.name.to_owned(),
            value: h.value.to_vec(),
        })
        .collect();
    Ok(Head {
        method: method.into(),
        url: url.into(),
        headers,
        length,
        chunked,
        expect,
    })
}
async fn line<R: AsyncBufRead + Unpin>(io: &mut R, max: usize) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    (&mut *io)
        .take(max as u64 + 1)
        .read_until(b'\n', &mut bytes)
        .await
        .map_err(|_| NativeError::Io)?;
    if bytes.len() > max || !bytes.ends_with(b"\r\n") {
        return Err(NativeError::Invalid);
    }
    Ok(bytes)
}
pub async fn read_request<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    destination: &Destination,
    limits: NativeLimits,
    budget: &NativeBudget,
    current: &(dyn Fn() -> bool + Send + Sync),
) -> Result<NativeRequest> {
    if limits.upload_bytes == 0 || limits.io_timeout.is_zero() {
        return Err(NativeError::Quota);
    }
    if !current() {
        return Err(NativeError::Closed);
    }
    tokio::time::timeout(
        limits.io_timeout,
        read(io, destination, limits, budget, current),
    )
    .await
    .map_err(|_| NativeError::Timeout)?
}
async fn read<S: AsyncRead + AsyncWrite + Unpin>(
    io: &mut S,
    destination: &Destination,
    limits: NativeLimits,
    budget: &NativeBudget,
    current: &(dyn Fn() -> bool + Send + Sync),
) -> Result<NativeRequest> {
    // Covers raw+cloned header bytes, parse/read scratch, header slots, normalized URL and method.
    let metadata = budget.reserve(
        4 * HEADER_BYTES + HEADER_COUNT * std::mem::size_of::<Header>() + URL_BYTES + METHOD_BYTES,
    )?;
    let mut io = BufReader::with_capacity(8192, io);
    let mut bytes = Vec::with_capacity(HEADER_BYTES);
    loop {
        let next = line(&mut io, HEADER_BYTES - bytes.len()).await?;
        bytes.extend_from_slice(&next);
        if next == b"\r\n" {
            break;
        }
    }
    let head = parse(&bytes, destination)?;
    if head.length.is_some_and(|n| n > limits.upload_bytes) {
        return Err(NativeError::Quota);
    }
    if !current() {
        return Err(NativeError::Closed);
    }
    if head.expect {
        io.get_mut()
            .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
            .await
            .map_err(|_| NativeError::Io)?;
        io.get_mut().flush().await.map_err(|_| NativeError::Io)?;
    }
    let (body, lease) = if head.chunked {
        chunked(&mut io, limits.upload_bytes, budget).await?
    } else {
        let length = head.length.unwrap_or(0);
        let mut lease = budget.reserve(length)?;
        let mut body = Vec::new();
        body.try_reserve_exact(length)
            .map_err(|_| NativeError::Quota)?;
        lease.grow(body.capacity().saturating_sub(length))?;
        body.resize(length, 0);
        io.read_exact(&mut body)
            .await
            .map_err(|_| NativeError::Io)?;
        (body, Some(lease))
    };
    if !current() {
        return Err(NativeError::Closed);
    }
    let identity = BodyIdentity {
        bytes: body.len() as u64,
        sha256: Sha256::digest(&body).into(),
    };
    Ok(NativeRequest {
        method: head.method,
        logical_url: head.url,
        headers: head.headers,
        upload: Arc::new(body),
        identity,
        _metadata: metadata,
        _body: lease,
    })
}
async fn chunked<R: AsyncBufRead + Unpin>(
    io: &mut R,
    limit: usize,
    budget: &NativeBudget,
) -> Result<(Vec<u8>, Option<Lease>)> {
    let (mut body, mut lease, mut overhead) = (Vec::new(), None, 0usize);
    loop {
        let next = line(io, 4096).await?;
        overhead = overhead
            .checked_add(next.len())
            .filter(|n| *n <= HEADER_BYTES)
            .ok_or(NativeError::Quota)?;
        let size = next[..next.len() - 2]
            .split(|b| *b == b';')
            .next()
            .ok_or(NativeError::Invalid)?;
        if size.is_empty() || !size.iter().all(u8::is_ascii_hexdigit) {
            return Err(NativeError::Invalid);
        }
        let n = usize::from_str_radix(
            std::str::from_utf8(size).map_err(|_| NativeError::Invalid)?,
            16,
        )
        .map_err(|_| NativeError::Quota)?;
        if n == 0 {
            // Request trailers have no representation in this bounded native request contract.
            if line(io, 4096).await? != b"\r\n" {
                return Err(NativeError::Invalid);
            }
            return Ok((body, lease));
        }
        let end = body
            .len()
            .checked_add(n)
            .filter(|n| *n <= limit)
            .ok_or(NativeError::Quota)?;
        if end > body.capacity() {
            let capacity = end.max(body.capacity().saturating_mul(2)).min(limit);
            let mut next_lease = budget.reserve(capacity)?;
            let mut next = Vec::new();
            next.try_reserve_exact(capacity)
                .map_err(|_| NativeError::Quota)?;
            next_lease.grow(next.capacity().saturating_sub(capacity))?;
            next.extend_from_slice(&body);
            body = next;
            lease = Some(next_lease);
        }
        let offset = body.len();
        body.resize(end, 0);
        io.read_exact(&mut body[offset..])
            .await
            .map_err(|_| NativeError::Io)?;
        let mut end_line = [0; 2];
        io.read_exact(&mut end_line)
            .await
            .map_err(|_| NativeError::Io)?;
        if end_line != *b"\r\n" {
            return Err(NativeError::Invalid);
        }
    }
}
