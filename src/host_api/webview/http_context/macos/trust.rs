//! Challenge-local trust only. No system anchors, preferences, or WK global exceptions.
use super::ContextError;
use objc2::{msg_send, rc::Retained, runtime::AnyObject};
use objc2_foundation::{NSArray, NSData, NSError, NSURLAuthenticationChallenge};
use sha2::{Digest, Sha256};
use std::ffi::c_void;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Authority {
    pub host: String,
    pub port: u16,
}
impl Authority {
    pub fn new(host: &str, port: u16) -> Result<Self, ContextError> {
        let host = host
            .strip_prefix('[')
            .and_then(|h| h.strip_suffix(']'))
            .unwrap_or(host);
        if port == 0 || host.is_empty() || host.len() > 253 || host.contains('*') {
            return Err(ContextError::Native);
        }
        let host = if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            ip.to_string()
        } else {
            match url::Host::parse(host).map_err(|_| ContextError::Native)? {
                url::Host::Domain(s) => s,
                url::Host::Ipv4(ip) => ip.to_string(),
                url::Host::Ipv6(ip) => ip.to_string(),
            }
        };
        Ok(Self { host, port })
    }
}
/// Constructor is private: an endpoint descriptor alone is never native proof.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NativeObservation {
    authority: Authority,
    leaf: [u8; 32],
}
impl NativeObservation {
    pub fn authority(&self) -> &Authority {
        &self.authority
    }
    pub fn leaf(&self) -> [u8; 32] {
        self.leaf
    }
}
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecTrustSetNetworkFetchAllowed(trust: *mut c_void, allowed: bool) -> i32;
    fn SecTrustEvaluateWithError(trust: *mut c_void, error: *mut *mut NSError) -> bool;
    fn SecTrustGetCertificateAtIndex(trust: *mut c_void, index: isize) -> *mut c_void;
    fn SecCertificateCopyData(cert: *mut c_void) -> *mut NSData;
    fn SecTrustSetAnchorCertificates(trust: *mut c_void, anchors: *const NSArray<AnyObject>)
    -> i32;
    fn SecTrustSetAnchorCertificatesOnly(trust: *mut c_void, only: bool) -> i32;
}
/// Native error must be untrusted issuer, then the original SSL policies must pass with only
/// the exact presented leaf as the challenge-local anchor. AIA/revocation network is disabled.
/// The main socket and its NSURLRequest are never reconstructed or retried here.
pub(super) fn observe(challenge: &NSURLAuthenticationChallenge) -> Option<NativeObservation> {
    let space = challenge.protectionSpace();
    if space.authenticationMethod().to_string() != "NSURLAuthenticationMethodServerTrust"
        || challenge.previousFailureCount() != 0
    {
        return None;
    }
    let authority =
        Authority::new(&space.host().to_string(), u16::try_from(space.port()).ok()?).ok()?;
    // SAFETY: NSURLProtectionSpace serverTrust is a borrowed SecTrustRef during this callback.
    let trust: *mut c_void = unsafe { msg_send![&space, serverTrust] };
    if trust.is_null() {
        return None;
    }
    // SAFETY: challenge owns a live SecTrust until this method returns.
    let leaf = unsafe { observe_trust(trust) }?;
    Some(NativeObservation { authority, leaf })
}
/// A borrowed SecTrust from a native challenge, confined to the calling thread.
unsafe fn observe_trust(trust: *mut c_void) -> Option<[u8; 32]> {
    // SAFETY: private challenge SecTrust, never a shared/global trust store.
    if unsafe { SecTrustSetNetworkFetchAllowed(trust, false) } != 0 {
        return None;
    }
    let mut error = std::ptr::null_mut();
    // SAFETY: live trust and writable out-error, returned error follows CF +1 ownership.
    let trusted = unsafe { SecTrustEvaluateWithError(trust, &mut error) };
    // SAFETY: CFError and NSError are toll-free bridged, ownership transfers exactly once.
    let error = unsafe { Retained::from_raw(error) };
    if trusted
        || !error.is_some_and(|e| {
            e.domain().to_string() == "NSOSStatusErrorDomain" && e.code() == -67843
        })
    {
        return None;
    }
    // SAFETY: live evaluated trust; leaf remains owned by this trust through the callback.
    let cert = unsafe { SecTrustGetCertificateAtIndex(trust, 0) };
    if cert.is_null() {
        return None;
    }
    // SAFETY: certificate-copy returns CFData/NSData +1.
    let data = unsafe { SecCertificateCopyData(cert) };
    // SAFETY: CFData and NSData are toll-free bridged; transfer copy +1 ownership.
    let der = unsafe { Retained::from_raw(data) }?;
    if der.is_empty() || der.len() > 16_384 {
        return None;
    }
    let leaf = Sha256::digest(der.to_vec()).into();
    // SAFETY: SecCertificate is an NSObject-compatible CF type and NSArray retains it.
    let anchors = NSArray::from_slice(&[unsafe { &*cert.cast::<AnyObject>() }]);
    // SAFETY: change only this challenge's anchor set, never global trust or probe datastore.
    if unsafe { SecTrustSetAnchorCertificates(trust, &*anchors) } != 0 {
        return None;
    }
    // SAFETY: exact challenge-local anchor only; built-in roots cannot hide a second path.
    if unsafe { SecTrustSetAnchorCertificatesOnly(trust, true) } != 0 {
        return None;
    }
    let mut other_error = std::ptr::null_mut();
    // SAFETY: native re-evaluation preserves original hostname, usage, and time policies.
    let accepted = unsafe { SecTrustEvaluateWithError(trust, &mut other_error) };
    // SAFETY: transfer optional +1 CFError ownership even on unexpected successful-with-error.
    let other_error = unsafe { Retained::from_raw(other_error) };
    if !accepted || other_error.is_some() {
        return None;
    }
    Some(leaf)
}

#[cfg(test)]
#[path = "trust/tests.rs"]
mod tests;
