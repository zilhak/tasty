use super::*;
use objc2_foundation::NSString;
#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecCertificateCreateWithData(allocator: *const c_void, data: &NSData) -> *mut c_void;
    fn SecPolicyCreateSSL(server: bool, hostname: &NSString) -> *mut c_void;
    fn SecTrustCreateWithCertificates(
        cert: *const c_void,
        policy: *const c_void,
        trust: *mut *mut c_void,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFRelease(value: *const c_void);
}
fn check(host: &str, expired: bool, future: bool, wrong_usage: bool, expected: bool) {
    let mut params = rcgen::CertificateParams::new(vec!["mirror.example".into()]).unwrap();
    if expired {
        params.not_before = rcgen::date_time_ymd(2020, 1, 1);
        params.not_after = rcgen::date_time_ymd(2021, 1, 1);
    }
    if future {
        params.not_before = rcgen::date_time_ymd(2035, 1, 1);
        params.not_after = rcgen::date_time_ymd(2036, 1, 1);
    }
    if wrong_usage {
        params.extended_key_usages = vec![rcgen::ExtendedKeyUsagePurpose::ClientAuth];
    }
    let key = rcgen::KeyPair::generate().unwrap();
    let cert = params.self_signed(&key).unwrap();
    let data = NSData::with_bytes(cert.der());
    // SAFETY: test-only native CF create APIs and live input NSData.
    let cert = unsafe { SecCertificateCreateWithData(std::ptr::null(), &data) };
    assert!(!cert.is_null());
    // SAFETY: creates a native server SSL policy with the exact expected hostname.
    let policy = unsafe { SecPolicyCreateSSL(true, &NSString::from_str(host)) };
    assert!(!policy.is_null());
    let mut trust = std::ptr::null_mut();
    // SAFETY: live certificate/policy CF objects and writable result.
    let status = unsafe { SecTrustCreateWithCertificates(cert, policy, &mut trust) };
    assert_eq!(status, 0);
    // SAFETY: current thread exclusively owns the test's trust object.
    let result = unsafe { observe_trust(trust) };

    // SAFETY: each Create result has exactly one release after all borrowed uses.
    unsafe {
        CFRelease(trust);
    }
    // SAFETY: owned Create result.
    unsafe {
        CFRelease(policy);
    }
    // SAFETY: owned Create result.
    unsafe {
        CFRelease(cert);
    }
    assert_eq!(result.is_some(), expected);
}
#[test]
fn native_unknown_issuer_only() {
    check("mirror.example", false, false, false, true);
}
#[test]
fn native_unknown_issuer_and_wrong_host() {
    check("wrong.example", false, false, false, false);
}
#[test]
fn native_unknown_issuer_and_expired() {
    check("mirror.example", true, false, false, false);
}
#[test]
fn native_unknown_issuer_expired_and_wrong_host() {
    check("wrong.example", true, false, false, false);
}

#[test]
fn native_unknown_issuer_and_future() {
    check("mirror.example", false, true, false, false);
}
#[test]
fn native_unknown_issuer_and_wrong_usage() {
    check("mirror.example", false, false, true, false);
}
