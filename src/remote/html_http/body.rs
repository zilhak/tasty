//! Quota-owned immutable bytes and exact-offset upload assembly.
use super::{BodyIdentity, Error, Result};
use sha2::{Digest, Sha256};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use tasty_ipc::html_transfer::CHUNK_BYTES;
#[derive(Debug)]
pub(super) struct Quota {
    pub used: AtomicUsize,
    pub limit: usize,
}
#[derive(Debug)]
pub(super) struct Lease {
    quota: Arc<Quota>,
    held: usize,
}
impl Lease {
    pub fn new(quota: Arc<Quota>) -> Self {
        Self { quota, held: 0 }
    }
    pub fn grow(&mut self, n: usize) -> Result<()> {
        self.quota
            .used
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |used| {
                used.checked_add(n).filter(|n| *n <= self.quota.limit)
            })
            .map_err(|_| Error::Quota("retained bytes"))?;
        self.held += n;
        Ok(())
    }
}
impl Drop for Lease {
    fn drop(&mut self) {
        self.quota.used.fetch_sub(self.held, Ordering::AcqRel);
    }
}
#[derive(Debug)]
struct OwnedBytes {
    bytes: Vec<u8>,
    identity: BodyIdentity,
    _lease: Lease,
}
#[derive(Clone)]
pub struct Body(Arc<OwnedBytes>);
impl std::fmt::Debug for Body {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Body").field(self.identity()).finish()
    }
}
impl Body {
    pub fn bytes(&self) -> &[u8] {
        &self.0.bytes
    }
    pub fn identity(&self) -> &BodyIdentity {
        &self.0.identity
    }
    pub(super) fn from_vec(bytes: Vec<u8>, lease: Lease) -> Self {
        let identity = BodyIdentity {
            bytes: bytes.len(),
            sha256: Sha256::digest(&bytes).into(),
        };
        Self(Arc::new(OwnedBytes {
            bytes,
            identity,
            _lease: lease,
        }))
    }
}
pub struct Upload {
    pub(super) bytes: Vec<u8>,
    pub(super) lease: Lease,
    identity: BodyIdentity,
}
impl std::fmt::Debug for Upload {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("Upload").field(self.identity()).finish()
    }
}
impl Upload {
    pub(super) fn belongs_to(&self, quota: &Arc<Quota>) -> bool {
        Arc::ptr_eq(&self.lease.quota, quota)
    }
    pub fn identity(&self) -> &BodyIdentity {
        &self.identity
    }
}
pub struct UploadBuilder {
    bytes: Vec<u8>,
    digest: Sha256,
    lease: Lease,
    limit: usize,
}
impl UploadBuilder {
    pub(super) fn new(quota: Arc<Quota>, limit: usize) -> Self {
        Self {
            bytes: Vec::new(),
            digest: Sha256::new(),
            lease: Lease::new(quota),
            limit,
        }
    }
    pub fn append(&mut self, offset: u64, bytes: &[u8]) -> Result<()> {
        if offset != self.bytes.len() as u64 || bytes.len() > CHUNK_BYTES {
            return Err(Error::Invalid("upload credit offset or length"));
        }
        if self
            .bytes
            .len()
            .checked_add(bytes.len())
            .is_none_or(|n| n > self.limit)
        {
            return Err(Error::Quota("upload bytes"));
        }
        self.lease.grow(bytes.len())?;
        self.bytes.extend_from_slice(bytes);
        self.digest.update(bytes);
        Ok(())
    }
    pub fn finish(self) -> Upload {
        let identity = BodyIdentity {
            bytes: self.bytes.len(),
            sha256: self.digest.finalize().into(),
        };
        Upload {
            bytes: self.bytes,
            lease: self.lease,
            identity,
        }
    }
}
pub(super) fn decode(
    encoded: Body,
    encoding: &str,
    quota: Arc<Quota>,
    limit: usize,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<Body> {
    use std::io::Read;
    if encoding.is_empty() || encoding == "identity" {
        if encoded.bytes().len() > limit {
            return Err(Error::Quota("decoded bytes"));
        }
        return Ok(encoded);
    }
    let mut reader: Box<dyn Read + '_> = match encoding {
        "gzip" => Box::new(flate2::read::MultiGzDecoder::new(encoded.bytes())),
        "deflate" => Box::new(flate2::read::ZlibDecoder::new(encoded.bytes())),
        _ => return Err(Error::UnsupportedEncoding),
    };
    let mut result = Vec::new();
    let mut lease = Lease::new(quota);
    let mut buf = [0; 16 * 1024];
    loop {
        if cancel.load(Ordering::Acquire) {
            return Err(Error::Cancelled);
        }
        let n = reader
            .read(&mut buf)
            .map_err(|_| Error::Invalid("content decoding"))?;
        if n == 0 {
            break;
        }
        if result.len().checked_add(n).is_none_or(|n| n > limit) {
            return Err(Error::Quota("decoded bytes"));
        }
        lease.grow(n)?;
        result.extend_from_slice(&buf[..n]);
    }
    Ok(Body::from_vec(result, lease))
}
