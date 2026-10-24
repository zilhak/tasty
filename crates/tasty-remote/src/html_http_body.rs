//! Worker-side verification of a complete encoded HTTP response before scan/native publication.
//! No request, network, filesystem, origin rewrite or script-setting operation is performed here.
#[cfg(test)]
mod tests;

use flate2::read::{MultiGzDecoder, ZlibDecoder};
use sha2::{Digest, Sha256};
use std::{
    io::{self, Read},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
};
use tasty_ipc::html_http::{BodyIdentity, HttpReply, HttpRequest, ResponseHead, ResponseId};
use tasty_model::html_script::{ScriptScan, scan::SCAN_LIMIT_BYTES};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BodyError {
    Invalid,
    Identity,
    Encoding,
    Quota,
    Cancelled,
}
/// An independent client memory quota, unrelated to encoded document consent or wire credits.
/// The assembler must separately bound provisional allocations before calling this worker.
#[derive(Clone)]
pub struct BodyBudget(Arc<Quota>);
struct Quota {
    used: AtomicUsize,
    limit: usize,
}
struct Lease {
    quota: Arc<Quota>,
    bytes: usize,
}
impl BodyBudget {
    pub fn new(limit: usize) -> Self {
        Self(Arc::new(Quota {
            used: AtomicUsize::new(0),
            limit,
        }))
    }
    pub fn used(&self) -> usize {
        self.0.used.load(Ordering::Acquire)
    }
    fn reserve(&self, bytes: usize) -> Result<Lease, BodyError> {
        self.0
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(bytes).filter(|used| *used <= self.0.limit)
            })
            .map_err(|_| BodyError::Quota)?;
        Ok(Lease {
            quota: self.0.clone(),
            bytes,
        })
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.quota.used.fetch_sub(self.bytes, Ordering::AcqRel);
    }
}
/// Retain this object through native delivery/replay, so its memory leases outlive the write.
/// Encoded bytes and original headers/status stay paired; decoded bytes are only the scan input.
pub struct VerifiedBody {
    head: ResponseHead,
    method: String,
    upload: BodyIdentity,
    encoded: Arc<Vec<u8>>,
    decoded: Arc<Vec<u8>>,
    encoded_identity: BodyIdentity,
    decoded_identity: BodyIdentity,
    scan: ScriptScan,
    _metadata_lease: Lease,
    _encoded_lease: Lease,
    _decoded_lease: Option<Lease>,
}
impl VerifiedBody {
    pub fn head(&self) -> &ResponseHead {
        &self.head
    }
    pub fn encoded(&self) -> &[u8] {
        &self.encoded
    }
    pub fn decoded(&self) -> &[u8] {
        &self.decoded
    }
    pub fn encoded_identity(&self) -> BodyIdentity {
        self.encoded_identity
    }
    pub fn decoded_identity(&self) -> BodyIdentity {
        self.decoded_identity
    }
    pub fn scan(&self) -> ScriptScan {
        self.scan
    }
    /// A browser resubmission may consume this representation only by full response/method/upload
    /// identity. Failure must not trigger an automatic upstream retry or URL-only cache lookup.
    pub fn matches_native(&self, response: ResponseId, method: &str, upload: BodyIdentity) -> bool {
        self.head.id == response && self.method == method && self.upload == upload
    }
}
fn cancelled(cancel: &AtomicBool) -> Result<(), BodyError> {
    if cancel.load(Ordering::Acquire) {
        Err(BodyError::Cancelled)
    } else {
        Ok(())
    }
}
fn digest(bytes: &[u8], cancel: &AtomicBool) -> Result<[u8; 32], BodyError> {
    let mut hash = Sha256::new();
    for chunk in bytes.chunks(16 * 1024) {
        cancelled(cancel)?;
        hash.update(chunk);
    }
    cancelled(cancel)?;
    Ok(hash.finalize().into())
}
struct Cancellable<'a> {
    bytes: &'a [u8],
    cancel: &'a AtomicBool,
}
impl Read for Cancellable<'_> {
    fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
        if self.cancel.load(Ordering::Acquire) {
            return Err(io::Error::new(
                io::ErrorKind::ConnectionAborted,
                "cancelled",
            ));
        }
        let n = out.len().min(16 * 1024);
        self.bytes.read(&mut out[..n])
    }
}
fn encoding(head: &ResponseHead) -> Result<&str, BodyError> {
    let mut fields = head
        .headers
        .iter()
        .filter(|h| h.name.eq_ignore_ascii_case("content-encoding"));
    let value = fields.next();
    if fields.next().is_some() {
        return Err(BodyError::Encoding);
    }
    value.map_or(Ok(""), |h| {
        std::str::from_utf8(&h.value).map_err(|_| BodyError::Encoding)
    })
}
fn decompress(
    kind: &str,
    encoded: &[u8],
    expected: usize,
    budget: &BodyBudget,
    cancel: &AtomicBool,
) -> Result<(Arc<Vec<u8>>, Lease), BodyError> {
    let input = Cancellable {
        bytes: encoded,
        cancel,
    };
    let mut reader: Box<dyn Read + '_> = if kind.eq_ignore_ascii_case("gzip") {
        Box::new(MultiGzDecoder::new(input))
    } else if kind.eq_ignore_ascii_case("deflate") {
        Box::new(ZlibDecoder::new(input))
    } else {
        return Err(BodyError::Encoding);
    };
    let mut output = Vec::new();
    // Reserve before allocation and bound output by the advertised verified identity, never
    // grow until EOF. Any allocator capacity rounding is charged before reading into it.
    let mut lease = budget.reserve(expected)?;
    output
        .try_reserve_exact(expected)
        .map_err(|_| BodyError::Quota)?;
    if output.capacity() > expected {
        let mut extra = budget.reserve(output.capacity() - expected)?;
        lease.bytes += extra.bytes;
        extra.bytes = 0;
    }
    output.resize(expected, 0);
    reader.read_exact(&mut output).map_err(|_| {
        if cancel.load(Ordering::Acquire) {
            BodyError::Cancelled
        } else {
            BodyError::Encoding
        }
    })?;
    let mut extra = [0];
    if reader.read(&mut extra).map_err(|_| {
        if cancel.load(Ordering::Acquire) {
            BodyError::Cancelled
        } else {
            BodyError::Encoding
        }
    })? != 0
    {
        return Err(BodyError::Identity);
    }
    Ok((Arc::new(output), lease))
}

/// Run on a cancellable scan/decode worker, after exact Complete and received-offset validation.
/// Caller revalidates its live connection/document before publishing the returned Arc to native.
/// Quota covers retained header, encoded and compressed output capacities. Frame/scan scratch
/// allocations and provisional assembly have separate bounded worker/transport owners.
pub fn verify(
    request: &HttpRequest,
    head: ResponseHead,
    encoded: Arc<Vec<u8>>,
    identities: (BodyIdentity, BodyIdentity),
    budget: &BodyBudget,
    decoded_limit: usize,
    cancel: &AtomicBool,
) -> Result<Arc<VerifiedBody>, BodyError> {
    cancelled(cancel)?;
    request.validate().map_err(|_| BodyError::Invalid)?;
    let HttpRequest::Begin { method, upload, .. } = request else {
        return Err(BodyError::Invalid);
    };
    let method = method.clone();
    let reply = HttpReply::Head { head };
    reply.validate().map_err(|_| BodyError::Invalid)?;
    let metadata_lease =
        budget.reserve(reply.retained_bytes().saturating_add(method.capacity()))?;
    let HttpReply::Head { head } = reply else {
        unreachable!()
    };
    let HttpRequest::Begin { logical_url, .. } = request else {
        unreachable!()
    };
    for value in [logical_url.as_str(), head.logical_url.as_str()] {
        let url = url::Url::parse(value).map_err(|_| BodyError::Invalid)?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(BodyError::Invalid);
        }
    }
    let (encoded_identity, decoded_identity) = identities;
    if head.id.request != request.id()
        || head.id.source_generation != request.source_generation()
        || encoded.len() as u64 != encoded_identity.bytes
        || head
            .encoded_length
            .is_some_and(|n| n != encoded_identity.bytes)
        || (!head.has_body && (!encoded.is_empty() || decoded_identity.bytes != 0))
    {
        return Err(BodyError::Identity);
    }
    let expected = decoded_identity
        .checked_len()
        .map_err(|_| BodyError::Quota)?;
    if expected > decoded_limit {
        return Err(BodyError::Quota);
    }
    let encoded_lease = budget.reserve(encoded.capacity())?;
    if digest(&encoded, cancel)? != encoded_identity.sha256 {
        return Err(BodyError::Identity);
    }
    let kind = if head.has_body { encoding(&head)? } else { "" };
    let (decoded, decoded_lease) = if kind.is_empty() || kind.eq_ignore_ascii_case("identity") {
        if encoded_identity != decoded_identity {
            return Err(BodyError::Identity);
        }
        (encoded.clone(), None)
    } else {
        let (bytes, lease) = decompress(kind, &encoded, expected, budget, cancel)?;
        (bytes, Some(lease))
    };
    let scan = tasty_model::html_script::scan::scan_reader(
        Cancellable {
            bytes: &decoded,
            cancel,
        },
        SCAN_LIMIT_BYTES,
    )
    .map_err(|_| {
        if cancel.load(Ordering::Acquire) {
            BodyError::Cancelled
        } else {
            BodyError::Invalid
        }
    })?;
    if scan.fingerprint.0 != decoded_identity.sha256 {
        return Err(BodyError::Identity);
    }
    cancelled(cancel)?;
    Ok(Arc::new(VerifiedBody {
        head,
        method,
        upload: *upload,
        encoded,
        decoded,
        encoded_identity,
        decoded_identity,
        scan,
        _metadata_lease: metadata_lease,
        _encoded_lease: encoded_lease,
        _decoded_lease: decoded_lease,
    }))
}
