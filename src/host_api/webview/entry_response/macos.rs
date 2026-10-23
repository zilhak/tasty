//! Completed entry responses in a private WKWebsiteDataStore. No path or network lookup.
use super::{ENTRY_CSP, EntryResponseSource};
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class};
use objc2_foundation::{NSData, NSDictionary, NSError, NSHTTPURLResponse, NSString};
use objc2_web_kit::{
    WKURLSchemeHandler, WKURLSchemeTask, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use std::cell::RefCell;
use std::collections::HashSet;

pub(super) struct Ivars {
    source: EntryResponseSource,
    active: RefCell<HashSet<usize>>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub(super) struct EntryHandler;
    unsafe impl NSObjectProtocol for EntryHandler {}
    unsafe impl WKURLSchemeHandler for EntryHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            let key = std::ptr::from_ref(task).cast::<()>() as usize;
            self.ivars().active.borrow_mut().insert(key);
            // SAFETY: WebKit calls its main-thread handler with a live task. Every
            // callback checks stop/reentrant cancellation; no callback follows completion.
            #[allow(clippy::multiple_unsafe_ops_per_block)]
            unsafe {
                let request = task.request();
                let url = request.URL();
                let uri = url
                    .as_ref()
                    .and_then(|url| url.absoluteString())
                    .map(|s| s.to_string());
                let response = uri
                    .as_deref()
                    .and_then(|uri| self.ivars().source.response(uri))
                    .filter(|_| {
                        request
                            .HTTPMethod()
                            .is_some_and(|method| method.to_string() == "GET")
                    });
                let Some((body, url)) = response.zip(url) else {
                    if self.ivars().active.borrow_mut().remove(&key) {
                        let error = NSError::errorWithDomain_code_userInfo(
                            &NSString::from_str("TastyRemoteEntry"),
                            1,
                            None,
                        );
                        task.didFailWithError(&error);
                    }
                    return;
                };
                let keys = [
                    NSString::from_str("Content-Type"),
                    NSString::from_str("Content-Security-Policy"),
                    NSString::from_str("Cache-Control"),
                    NSString::from_str("X-Content-Type-Options"),
                ];
                let values = [
                    NSString::from_str(body.mime()),
                    NSString::from_str(ENTRY_CSP),
                    NSString::from_str("no-store"),
                    NSString::from_str("nosniff"),
                ];
                let headers = NSDictionary::from_slices(
                    &keys.iter().map(|s| &**s).collect::<Vec<_>>(),
                    &values.iter().map(|s| &**s).collect::<Vec<_>>(),
                );
                let Some(response) =
                    NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                        NSHTTPURLResponse::alloc(),
                        &url,
                        200,
                        Some(&NSString::from_str("HTTP/1.1")),
                        Some(&headers),
                    )
                else {
                    if self.ivars().active.borrow_mut().remove(&key) {
                        task.didFailWithError(&NSError::errorWithDomain_code_userInfo(
                            &NSString::from_str("TastyRemoteEntry"),
                            2,
                            None,
                        ));
                    }
                    return;
                };
                if !self.ivars().active.borrow().contains(&key) {
                    return;
                }
                task.didReceiveResponse(&response);
                if !self.ivars().active.borrow().contains(&key) {
                    return;
                }
                let data = NSData::with_bytes(body.bytes());
                task.didReceiveData(&data);
                if self.ivars().active.borrow_mut().remove(&key) {
                    task.didFinish();
                }
            }
        }
        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            self.ivars()
                .active
                .borrow_mut()
                .remove(&(std::ptr::from_ref(task).cast::<()>() as usize));
        }
    }
);
/// Configuration retains its scheme handler before the first WKWebView is created.
pub(in crate::host_api::webview) fn configure(
    config: &WKWebViewConfiguration,
    source: EntryResponseSource,
    mtm: MainThreadMarker,
) {
    let this = EntryHandler::alloc(mtm).set_ivars(Ivars {
        source,
        active: RefCell::default(),
    });
    // SAFETY: main-thread NSObject initialization and configuration before view creation.
    let handler: Retained<EntryHandler> = unsafe { objc2::msg_send![super(this), init] };
    // SAFETY: main-thread configuration owns both nonpersistent store and handler.
    #[allow(clippy::multiple_unsafe_ops_per_block)]
    unsafe {
        config.setWebsiteDataStore(&WKWebsiteDataStore::nonPersistentDataStore(mtm));
        config.setURLSchemeHandler_forURLScheme(
            Some(ProtocolObject::from_ref(&*handler)),
            &NSString::from_str(tasty_remote::html_body::ENTRY_SCHEME),
        );
    }
}
