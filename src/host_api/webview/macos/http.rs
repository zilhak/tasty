//! WebKit view of a remote HTTP document. Every request goes to the document endpoint as an
//! authenticated HTTP proxy. WebKit still connects to a loopback address itself, so such a host
//! is addressed by its alias and a content rule refuses every direct loopback load.
use super::super::http_endpoint::alias;
use super::super::http_native::NativeDocument;
use crate::app::http_session::HttpSession;
use block2::{DynBlock, RcBlock};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, Message, msg_send,
    rc::Retained,
    runtime::{AnyObject, Sel},
    sel,
};
use objc2_foundation::{
    NSArray, NSDate, NSError, NSSet, NSString, NSURLAuthenticationChallenge,
    NSURLAuthenticationMethodHTTPBasic, NSURLAuthenticationMethodServerTrust, NSURLCredential,
    NSURLCredentialPersistence, NSURLProtectionSpace, NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{
    WKContentRuleList, WKContentRuleListStore, WKWebView, WKWebViewConfiguration,
    WKWebsiteDataStore, WKWebsiteDataTypeDiskCache, WKWebsiteDataTypeFetchCache,
    WKWebsiteDataTypeMemoryCache, WKWebsiteDataTypeServiceWorkerRegistrations,
};
use sha2::{Digest, Sha256};
use std::{
    cell::{Cell, RefCell},
    ffi::{CStr, CString, c_char, c_void},
    ptr::NonNull,
    rc::{Rc, Weak},
};

/// WebKit's url-filter has no alternation, so each loopback form WebKit would reach on the client
/// is its own rule. Canonical URLs spell every IPv4 form as dotted decimal and IPv6 compressed.
const LOOPBACK_HOSTS: [&str; 8] = [
    r"localhost[:/?#]",
    r"localhost\.[:/?#]",
    r"127\.",
    r"0\.0\.0\.0[:/?#]",
    r"\[::1\]",
    r"\[::ffff:7f",
    r"\[::ffff:0:0\]",
    r"\[::\]",
];
fn loopback_rules() -> String {
    let rules: Vec<_> = LOOPBACK_HOSTS
        .iter()
        .map(|host| {
            serde_json::json!({
                "trigger": {"url-filter": format!("^[a-z][-a-z0-9+.]*://([^/?#@]*@)?{host}")},
                "action": {"type": "block"},
            })
        })
        .collect();
    serde_json::Value::Array(rules).to_string()
}

type CreateEndpoint = unsafe extern "C" fn(*const c_char, *const c_char) -> *mut AnyObject;
type CreateProxy = unsafe extern "C" fn(*mut AnyObject, *mut AnyObject) -> *mut AnyObject;
type Failover = unsafe extern "C" fn(*mut AnyObject, bool);
type Clear = unsafe extern "C" fn(*mut AnyObject);
/// Network proxy functions bound at runtime, so no macOS 14 symbol is linked.
struct Network {
    endpoint: CreateEndpoint,
    proxy: CreateProxy,
    failover: Failover,
    clear_match: Clear,
    clear_excluded: Clear,
}
impl Network {
    fn get() -> Option<&'static Self> {
        static NETWORK: std::sync::OnceLock<Option<Network>> = std::sync::OnceLock::new();
        NETWORK.get_or_init(Self::open).as_ref()
    }
    fn open() -> Option<Self> {
        // The handle stays open for the process; proxy objects may outlive any view.
        // SAFETY: constant NUL-terminated path and supported flags.
        let handle = unsafe {
            libc::dlopen(
                c"/System/Library/Frameworks/Network.framework/Network".as_ptr(),
                libc::RTLD_LAZY | libc::RTLD_LOCAL,
            )
        };
        if handle.is_null() {
            return None;
        }
        let symbol = |name: &CStr| {
            // SAFETY: open handle and NUL-terminated name.
            NonNull::new(unsafe { libc::dlsym(handle, name.as_ptr()) })
        };
        macro_rules! bind {
            ($name:literal, $ty:ty) => {{
                let pointer = symbol($name)?;
                // SAFETY: the public SDK declares this symbol with exactly this signature.
                unsafe { std::mem::transmute::<NonNull<c_void>, $ty>(pointer) }
            }};
        }
        Some(Self {
            endpoint: bind!(c"nw_endpoint_create_host", CreateEndpoint),
            proxy: bind!(c"nw_proxy_config_create_http_connect", CreateProxy),
            failover: bind!(c"nw_proxy_config_set_failover_allowed", Failover),
            clear_match: bind!(c"nw_proxy_config_clear_match_domains", Clear),
            clear_excluded: bind!(c"nw_proxy_config_clear_excluded_domains", Clear),
        })
    }
    /// Send every host of this store through the endpoint; never fall back to a direct route.
    fn configure(&self, store: &WKWebsiteDataStore, port: u16) -> Result<(), String> {
        let port = CString::new(port.to_string()).map_err(|_| "proxy port")?;
        // SAFETY: public Network signature with live C strings; returns a +1 object.
        let endpoint = unsafe { (self.endpoint)(c"127.0.0.1".as_ptr(), port.as_ptr()) };
        // SAFETY: Network objects follow Objective-C ownership; take the +1 exactly once.
        let endpoint = unsafe { Retained::from_raw(endpoint) }.ok_or("proxy endpoint")?;
        // SAFETY: live endpoint and no TLS to the loopback proxy; returns a +1 object.
        let proxy =
            unsafe { (self.proxy)(Retained::as_ptr(&endpoint).cast_mut(), std::ptr::null_mut()) };
        // SAFETY: take the returned +1 exactly once.
        let proxy = unsafe { Retained::from_raw(proxy) }.ok_or("proxy configuration")?;
        let pointer = Retained::as_ptr(&proxy).cast_mut();
        // SAFETY: owned proxy configuration.
        unsafe { (self.failover)(pointer, false) };
        // SAFETY: owned proxy configuration; empty lists route every host.
        unsafe { (self.clear_match)(pointer) };
        // SAFETY: owned proxy configuration.
        unsafe { (self.clear_excluded)(pointer) };
        let proxies = NSArray::from_retained_slice(&[proxy]);
        // SAFETY: `available` checked the selector; the store copies the array.
        let () = unsafe { msg_send![store, setProxyConfigurations: &*proxies] };
        Ok(())
    }
}

/// macOS 14 added per-store proxy configuration; earlier systems keep remote HTTP unsupported, and
/// unmeasured releases do too.
pub(crate) fn available() -> bool {
    // SAFETY: class method on the WebKit class object; no instance is created.
    let responds: bool = unsafe {
        msg_send![
            WKWebsiteDataStore::class(),
            instancesRespondToSelector: sel!(setProxyConfigurations:)
        ]
    };
    responds && Network::get().is_some() && measured_release()
}

/// The loopback bypass and the `*.localhost` aliases were measured on macOS 27 only. An
/// unmeasured WebKit could treat an alias as loopback and connect to it directly, so earlier
/// releases keep the placeholder until they are measured the same way.
fn measured_release() -> bool {
    const FIRST_MEASURED: isize = 27;
    objc2_foundation::NSProcessInfo::processInfo()
        .operatingSystemVersion()
        .majorVersion
        >= FIRST_MEASURED
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Rules {
    Pending,
    Ready,
    Failed,
}
pub(super) struct HttpView {
    pub(super) document: Rc<NativeDocument>,
    store: Retained<WKWebsiteDataStore>,
    rules: Rc<Cell<Rules>>,
    /// A load requested before the loopback rule was installed.
    deferred: Rc<RefCell<Option<String>>>,
}
impl HttpView {
    /// A private store whose every request goes through this document's endpoint.
    pub(super) fn configure(
        config: &WKWebViewConfiguration,
        document: NativeDocument,
        mtm: MainThreadMarker,
    ) -> Result<Self, String> {
        let port = url::Url::parse(&document.proxy_uri())
            .ok()
            .and_then(|url| url.port())
            .ok_or("native HTTP endpoint lacks a proxy port")?;
        // SAFETY: main thread; a new private store, never the shared default store.
        let store = unsafe { WKWebsiteDataStore::nonPersistentDataStore(mtm) };
        Network::get()
            .ok_or("Network proxy configuration is unavailable")?
            .configure(&store, port)?;
        // SAFETY: main thread, before the view exists; the configuration retains the store.
        unsafe { config.setWebsiteDataStore(&store) };
        Ok(Self {
            document: Rc::new(document),
            store,
            rules: Rc::new(Cell::new(Rules::Pending)),
            deferred: Rc::default(),
        })
    }
    /// Install the loopback rule, then perform any load that waited for it. A failed compile
    /// leaves the view without loads instead of risking a direct loopback request.
    pub(super) fn install_rules(
        &self,
        webview: &WKWebView,
        surface_id: u32,
        mtm: MainThreadMarker,
    ) {
        // SAFETY: main thread WebKit class method.
        let Some(store) = (unsafe { WKContentRuleListStore::defaultStore(mtm) }) else {
            tracing::warn!(
                "WebView surface {surface_id}: no content rule store; HTTP stays unloaded"
            );
            self.rules.set(Rules::Failed);
            return;
        };
        let rules = self.rules.clone();
        let deferred = self.deferred.clone();
        // The compile can outlive the view and its document; neither is kept alive for it.
        let view = objc2::rc::Weak::from_retained(&webview.retain());
        let document = Rc::downgrade(&self.document);
        let handler = RcBlock::new(move |list: *mut WKContentRuleList, error: *mut NSError| {
            // SAFETY: WebKit calls this on the main thread; a non-null list is +0 and retained here.
            let list = unsafe { Retained::retain(list) };
            let document = document.upgrade();
            let Some(list) = list else {
                // SAFETY: a non-null error is valid for this call.
                let error = unsafe { error.as_ref() }.map(|e| e.localizedDescription().to_string());
                tracing::warn!("WebView surface {surface_id}: loopback rule failed: {error:?}");
                rules.set(Rules::Failed);
                // A load already accepted while the rule was pending can never start.
                if deferred.borrow_mut().take().is_some()
                    && let Some(document) = document
                {
                    load_failed(&document.session());
                }
                return;
            };
            let Some(view) = view.load() else {
                return;
            };
            // SAFETY: main thread WebKit property reads on the retained view.
            let configuration = unsafe { view.configuration() };
            // SAFETY: as above.
            let controller = unsafe { configuration.userContentController() };
            // SAFETY: main thread; the controller retains the rule list.
            unsafe { controller.addContentRuleList(&list) };
            rules.set(Rules::Ready);
            if let Some(url) = deferred.borrow_mut().take()
                && document.is_some_and(|document| document.session().is_current())
            {
                load(&view, &url);
            }
        });
        let id = NSString::from_str("tasty-http-loopback");
        let json = NSString::from_str(&loopback_rules());
        // SAFETY: main thread; WebKit copies the block and calls it once on the main thread.
        unsafe {
            store.compileContentRuleListForIdentifier_encodedContentRuleList_completionHandler(
                Some(&id),
                Some(&json),
                Some(&handler),
            );
        }
    }
    pub(super) fn session(&self) -> std::sync::Arc<HttpSession> {
        self.document.session()
    }
    /// Load `url` on its alias once the loopback rule is in place. Returns false when the rule
    /// failed and nothing will load.
    pub(super) fn load_url(&self, webview: &WKWebView, url: &str) -> bool {
        let url = alias::alias_url(url).unwrap_or_else(|| url.to_owned());
        match self.rules.get() {
            Rules::Ready => {
                load(webview, &url);
                true
            }
            Rules::Pending => {
                *self.deferred.borrow_mut() = Some(url);
                true
            }
            Rules::Failed => false,
        }
    }
}

fn load(webview: &WKWebView, url: &str) {
    let Some(url) = objc2_foundation::NSURL::URLWithString(&NSString::from_str(url)) else {
        return;
    };
    let request = objc2_foundation::NSURLRequest::requestWithURL(&url);
    // SAFETY: main thread WKWebView API with a live request.
    if unsafe { webview.loadRequest(&request) }.is_none() {
        tracing::debug!("WKWebView loadRequest started no navigation");
    }
}

/// The remote address for the address bar and the script gate, never the alias.
pub(super) fn logical(url: &str) -> String {
    alias::logical_url(url).unwrap_or_else(|| url.to_owned())
}

/// A main-frame navigation to a remote loopback address continues on its alias with the same
/// method and body. Returns false when `url` needs no alias.
pub(super) fn redirect_to_alias(
    webview: &WKWebView,
    request: &objc2_foundation::NSURLRequest,
    url: &str,
) -> bool {
    let Some(aliased) = alias::alias_url(url) else {
        return false;
    };
    let Some(aliased) = objc2_foundation::NSURL::URLWithString(&NSString::from_str(&aliased))
    else {
        return false;
    };
    // SAFETY: NSURLRequest conforms to NSMutableCopying; the copy is a new +1 object.
    let copy: Retained<objc2_foundation::NSMutableURLRequest> =
        unsafe { msg_send![request, mutableCopy] };
    copy.setURL(Some(&aliased));
    // SAFETY: main thread WKWebView API with a live request.
    if unsafe { webview.loadRequest(&copy) }.is_none() {
        tracing::debug!("WKWebView alias navigation started no navigation");
    }
    true
}

type Done = DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>;

/// Answer only this endpoint's own proxy challenge and only the exact leaf it presented. A
/// remote page's challenge continues without credentials; any other certificate is refused.
pub(super) fn respond(
    document: &Weak<NativeDocument>,
    surface_id: u32,
    challenge: &NSURLAuthenticationChallenge,
    done: &Done,
) {
    let Some(space) = protection_space(challenge) else {
        done.call((
            NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge,
            std::ptr::null_mut(),
        ));
        return;
    };
    let method = space.authenticationMethod();
    let Some(document) = document.upgrade() else {
        done.call((
            NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge,
            std::ptr::null_mut(),
        ));
        return;
    };
    // SAFETY: Foundation constants.
    let (basic, server_trust) = unsafe {
        (
            NSURLAuthenticationMethodHTTPBasic,
            NSURLAuthenticationMethodServerTrust,
        )
    };
    if space.isProxy() && &*method == basic {
        let uri = format!("http://{}:{}/", space.host(), space.port());
        if let Some((user, password)) = document.answer_proxy(&uri) {
            let credential = NSURLCredential::credentialWithUser_password_persistence(
                &NSString::from_str(user),
                &NSString::from_str(&password),
                NSURLCredentialPersistence::None,
            );
            done.call((
                NSURLSessionAuthChallengeDisposition::UseCredential,
                Retained::as_ptr(&credential).cast_mut(),
            ));
            return;
        }
        tracing::debug!("WebView surface {surface_id}: foreign proxy challenge cancelled");
        done.call((
            NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge,
            std::ptr::null_mut(),
        ));
        return;
    }
    if &*method == server_trust {
        let host = space.host().to_string();
        let port = u16::try_from(space.port()).unwrap_or(0);
        // SAFETY: the protection space of a server-trust challenge holds a live SecTrust.
        let trust: *mut SecTrust = unsafe { msg_send![&space, serverTrust] };
        let accepted = leaf(trust.cast())
            .is_some_and(|leaf| document.expected_leaf(&host, port) == Some(leaf));
        tracing::debug!(
            "WebView surface {surface_id}: certificate for {host}:{port}; endpoint leaf {}",
            if accepted { "accepted" } else { "refused" }
        );
        if accepted {
            // SAFETY: Foundation returns an autoreleased credential for the accepted SecTrust.
            let credential: Option<Retained<NSURLCredential>> =
                unsafe { msg_send![NSURLCredential::class(), credentialForTrust: trust] };
            if let Some(credential) = credential {
                done.call((
                    NSURLSessionAuthChallengeDisposition::UseCredential,
                    Retained::as_ptr(&credential).cast_mut(),
                ));
                return;
            }
        }
        done.call((
            NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge,
            std::ptr::null_mut(),
        ));
        return;
    }
    // A remote 401 shows its own response; no stored credential is offered.
    done.call((
        NSURLSessionAuthChallengeDisposition::RejectProtectionSpace,
        std::ptr::null_mut(),
    ));
}

/// WebKit passes a forwarding wrapper of the challenge. objc2's debug message check looks the
/// selector up on the wrapper class and fails, so this one read uses objc_msgSend directly.
fn protection_space(
    challenge: &NSURLAuthenticationChallenge,
) -> Option<Retained<NSURLProtectionSpace>> {
    type Send = unsafe extern "C-unwind" fn(*const AnyObject, Sel) -> *mut NSURLProtectionSpace;
    // SAFETY: objc_msgSend called with this exact signature of -[NSURLAuthenticationChallenge
    // protectionSpace], which takes no arguments and returns an object.
    let send = unsafe {
        std::mem::transmute::<unsafe extern "C-unwind" fn(), Send>(objc2::ffi::objc_msgSend)
    };
    let receiver = std::ptr::from_ref(challenge).cast::<AnyObject>();
    // SAFETY: live challenge for this delegate call; the getter returns a +0 object.
    let space = unsafe { send(receiver, sel!(protectionSpace)) };
    // SAFETY: a non-null +0 object is retained for use after this call.
    unsafe { Retained::retain(space) }
}

/// Opaque SecTrustRef, encoded as Foundation declares it.
#[repr(C)]
struct SecTrust {
    _private: [u8; 0],
}
// SAFETY: matches the `^{__SecTrust=}` encoding of SecTrustRef in Foundation method types.
unsafe impl objc2::encode::RefEncode for SecTrust {
    const ENCODING_REF: objc2::encode::Encoding =
        objc2::encode::Encoding::Pointer(&objc2::encode::Encoding::Struct("__SecTrust", &[]));
}

#[link(name = "Security", kind = "framework")]
unsafe extern "C" {
    fn SecTrustGetCertificateCount(trust: *const c_void) -> isize;
    fn SecTrustGetCertificateAtIndex(trust: *const c_void, index: isize) -> *const c_void;
    fn SecCertificateCopyData(certificate: *const c_void) -> *const c_void;
}
#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFDataGetLength(data: *const c_void) -> isize;
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
    fn CFRelease(object: *const c_void);
}
/// SHA-256 of the presented leaf certificate's DER.
fn leaf(trust: *const c_void) -> Option<[u8; 32]> {
    if trust.is_null() {
        return None;
    }
    // SAFETY: live SecTrust from the challenge.
    if unsafe { SecTrustGetCertificateCount(trust) } < 1 {
        return None;
    }
    // SAFETY: index 0 exists; the certificate is borrowed from the trust.
    let certificate = unsafe { SecTrustGetCertificateAtIndex(trust, 0) };
    if certificate.is_null() {
        return None;
    }
    // SAFETY: live certificate; returns a +1 CFData released below.
    let data = unsafe { SecCertificateCopyData(certificate) };
    if data.is_null() {
        return None;
    }
    // SAFETY: live CFData for both reads.
    let length = usize::try_from(unsafe { CFDataGetLength(data) }).unwrap_or(0);
    // SAFETY: live CFData.
    let bytes = unsafe { CFDataGetBytePtr(data) };
    let digest = (!bytes.is_null()).then(|| {
        // SAFETY: CFData owns `length` bytes at `bytes` until released.
        Sha256::digest(unsafe { std::slice::from_raw_parts(bytes, length) }).into()
    });
    // SAFETY: releases the +1 from SecCertificateCopyData exactly once.
    unsafe { CFRelease(data) };
    digest
}

/// A deliberate skip or cancel of the remote entry ends the load without failing the session.
pub(super) fn load_failed(session: &HttpSession) {
    let intentional = session.bridge.document().is_some_and(|document| {
        document
            .with_current(|model| {
                matches!(
                    model.state(document.entry()),
                    Some(
                        tasty_remote::html_http_client::ReceiveState::Skipped
                            | tasty_remote::html_http_client::ReceiveState::Cancelled
                    )
                )
            })
            .unwrap_or(false)
    });
    if session.is_current() && !intentional {
        session.fail();
    }
}

impl super::PlatformWebView {
    pub(crate) fn retaining_http(&self) -> bool {
        self.http
            .as_ref()
            .is_some_and(|http| http.document.retaining())
    }
    /// Keep this view and store across the Reload when the document can continue. Otherwise
    /// the caller's new document generation replaces the view.
    pub(crate) fn request_http_reload(&self) -> Result<bool, String> {
        if let Some(http) = &self.http
            && !http.document.request_reload()?
        {
            tracing::debug!(
                "WebView surface {}: no Reload receipt; the view is replaced",
                self.surface_id
            );
        }
        Ok(true)
    }
    /// Adopt the successor session, clear the HTTP caches and resume the original request in
    /// this view, so its cookies, method and upload continue.
    pub(crate) fn poll_http_reload(
        &self,
        session: std::sync::Arc<HttpSession>,
        _wake: std::sync::Arc<dyn Fn() + Send + Sync>,
    ) -> Result<bool, String> {
        let Some(http) = &self.http else {
            return Ok(false);
        };
        if !http.document.poll_reload(&session)? {
            return Ok(false);
        }
        if let Some(gate) = self.script_gate.borrow_mut().as_mut() {
            *gate = gate.clone().with_http_session(session.clone());
            session.attach_script_gate(gate.clone());
        }
        self._nav_delegate.ivars().cancel_pending_decision();
        clear_then_reload(&http.store, &self.webview, self.surface_id, session);
        Ok(true)
    }
    /// A remote HTTP document reloads its verified response once instead of a new request.
    pub(crate) fn reload_after_script_allow(&self) {
        let Some(http) = &self.http else {
            return self.reload();
        };
        let session = http.session();
        let Some(body) = session.entry_body() else {
            return;
        };
        match session.bodies.replay_once(body.head().id) {
            Ok(()) => self.reload(),
            Err(error) => tracing::warn!(?error, "native HTTP script response replay refused"),
        }
    }
}

/// Clear the HTTP caches a retained Reload must not reuse, then resend the current request.
/// Cookies and other storage stay. A failed clear cannot be observed, so the store completion
/// alone gates the Reload.
fn clear_then_reload(
    store: &WKWebsiteDataStore,
    webview: &WKWebView,
    surface_id: u32,
    session: std::sync::Arc<HttpSession>,
) {
    // SAFETY: WebKit constants available since macOS 10.11.
    let types = unsafe {
        NSSet::from_slice(&[
            WKWebsiteDataTypeDiskCache,
            WKWebsiteDataTypeMemoryCache,
            WKWebsiteDataTypeFetchCache,
            WKWebsiteDataTypeServiceWorkerRegistrations,
        ])
    };
    let view = webview.retain();
    let done = RcBlock::new(move || {
        if !session.is_current() {
            return;
        }
        tracing::debug!("WebView surface {surface_id}: HTTP caches cleared; reloading");
        // SAFETY: WebKit runs this completion on the main thread with the retained view.
        if unsafe { view.reload() }.is_none() {
            tracing::debug!("WKWebView reload started no navigation");
        }
    });
    // SAFETY: main thread; the retained store and copied block survive the asynchronous removal.
    unsafe {
        store.removeDataOfTypes_modifiedSince_completionHandler(
            &types,
            &NSDate::distantPast(),
            &done,
        );
    }
}
