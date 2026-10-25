//! Authentication hook that composes into the App navigation delegate.
use super::{DataStore, NativeObservation, trust};
use block2::DynBlock;
use objc2::{
    DefinedClass, MainThreadMarker, MainThreadOnly, define_class,
    rc::Retained,
    runtime::{NSObject, NSObjectProtocol},
};
use objc2_foundation::{
    NSURLAuthenticationChallenge, NSURLCredential, NSURLSessionAuthChallengeDisposition,
};
use objc2_web_kit::{WKNavigationDelegate, WKWebView};
use std::{ptr::NonNull, rc::Rc};

pub(super) type ObservationSink = Rc<dyn Fn(NativeObservation)>;
pub struct DelegateIvars {
    store: DataStore,
    probe: Option<ObservationSink>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = DelegateIvars]
    pub struct NavigationDelegate;
    // SAFETY: main-thread NSObject subclass implements the declared WebKit callback signatures.
    unsafe impl NSObjectProtocol for NavigationDelegate {}
    // SAFETY: main-thread NSObject subclass implements the declared WebKit callback signatures.
    unsafe impl WKNavigationDelegate for NavigationDelegate {
        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        fn challenge(
            &self,
            _view: &WKWebView,
            challenge: &NSURLAuthenticationChallenge,
            done: &DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
        ) {
            self.respond(challenge, done);
        }
    }
);
impl NavigationDelegate {
    pub(super) fn new(
        store: DataStore,
        probe: Option<ObservationSink>,
        mtm: MainThreadMarker,
    ) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(DelegateIvars { store, probe });
        // SAFETY: main-thread NSObject superclass initializer.
        unsafe { objc2::msg_send![super(this), init] }
    }
    /// Call from the real App delegate's authentication method; navigation/body policies stay there.
    pub fn respond(
        &self,
        challenge: &NSURLAuthenticationChallenge,
        done: &DynBlock<dyn Fn(NSURLSessionAuthChallengeDisposition, *mut NSURLCredential)>,
    ) {
        let disposition = NSURLSessionAuthChallengeDisposition::CancelAuthenticationChallenge;
        if !self.ivars().store.is_current() {
            done.call((disposition, std::ptr::null_mut()));
            return;
        }
        let Some(observation) = trust::observe(challenge) else {
            // Never default-handle non-server auth, which could read a user's credential store.
            done.call((disposition, std::ptr::null_mut()));
            return;
        };
        if let Some(probe) = &self.ivars().probe {
            // No credential is ever supplied to a private probe. Cancel native TLS first.
            done.call((disposition, std::ptr::null_mut()));
            if self.ivars().store.is_current() {
                probe(observation);
            }
        } else if self.ivars().store.accepts(&observation) {
            let space = challenge.protectionSpace();
            // SAFETY: borrowed challenge trust, evaluated with original policies in observe.
            let trust: *mut std::ffi::c_void = unsafe { objc2::msg_send![&space, serverTrust] };
            // SAFETY: Foundation creates an autoreleased credential from the accepted SecTrust.
            let credential: Option<Retained<NSURLCredential>> = unsafe {
                objc2::msg_send![objc2::class!(NSURLCredential), credentialForTrust: trust]
            };
            if let Some(credential) = credential.filter(|_| self.ivars().store.is_current()) {
                done.call((
                    NSURLSessionAuthChallengeDisposition::UseCredential,
                    NonNull::from(&*credential).as_ptr(),
                ));
            } else {
                done.call((disposition, std::ptr::null_mut()));
            }
        } else {
            done.call((disposition, std::ptr::null_mut()));
        }
    }
}
