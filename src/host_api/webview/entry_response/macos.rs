//! Private WK scheme responses. Native tasks stay on the main thread while bytes arrive.
use super::EntryResponseSource;
use objc2::Message;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class};
use objc2_foundation::{NSData, NSDictionary, NSError, NSHTTPURLResponse, NSString};
use objc2_web_kit::{
    WKURLSchemeHandler, WKURLSchemeTask, WKWebView, WKWebViewConfiguration, WKWebsiteDataStore,
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    sync::Arc,
    task::Poll,
};
use tasty_remote::html_body::{NativeBodyResponse, ResponseWait};

struct Pending {
    task: Retained<ProtocolObject<dyn WKURLSchemeTask>>,
    wait: ResponseWait,
}
pub(in crate::host_api::webview) struct Ivars {
    source: EntryResponseSource,
    active: RefCell<HashSet<usize>>,
    pending: RefCell<HashMap<usize, Pending>>,
}
define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = Ivars]
    pub(in crate::host_api::webview) struct EntryHandler;
    unsafe impl NSObjectProtocol for EntryHandler {}
    unsafe impl WKURLSchemeHandler for EntryHandler {
        #[unsafe(method(webView:startURLSchemeTask:))]
        fn start(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            let key = task_key(task);
            self.ivars().active.borrow_mut().insert(key);
            // SAFETY: WebKit supplies a live main-thread scheme task/request.
            #[allow(clippy::multiple_unsafe_ops_per_block)]
            let uri = unsafe {
                let request = task.request();
                request
                    .HTTPMethod()
                    .filter(|m| m.to_string() == "GET")
                    .and_then(|_| request.URL())
                    .and_then(|u| u.absoluteString())
                    .map(|s| s.to_string())
            };
            let Some(uri) = uri.filter(|uri| self.ivars().source.allows_uri(uri)) else {
                self.fail(key, task);
                return;
            };
            if let Some(body) = self.ivars().source.response(&uri) {
                self.deliver(key, task, body);
                return;
            }
            if self.ivars().pending.borrow().len() >= 256 {
                self.fail(key, task);
                return;
            }
            let Some(wait) = self.ivars().source.request(&uri) else {
                self.fail(key, task);
                return;
            };
            // Retained task and waiter never leave this main-thread handler.
            self.ivars().pending.borrow_mut().insert(
                key,
                Pending {
                    task: task.retain(),
                    wait,
                },
            );
            self.poll();
        }
        #[unsafe(method(webView:stopURLSchemeTask:))]
        fn stop(&self, _view: &WKWebView, task: &ProtocolObject<dyn WKURLSchemeTask>) {
            let key = task_key(task);
            self.ivars().active.borrow_mut().remove(&key);
            self.ivars().pending.borrow_mut().remove(&key);
            self.ivars().source.wake();
        }
    }
);
fn task_key(task: &ProtocolObject<dyn WKURLSchemeTask>) -> usize {
    std::ptr::from_ref(task).cast::<()>() as usize
}
impl EntryHandler {
    fn fail(&self, key: usize, task: &ProtocolObject<dyn WKURLSchemeTask>) {
        if self.ivars().active.borrow_mut().remove(&key) {
            // SAFETY: plain main-thread NSError construction with a live domain string.
            let error = unsafe {
                NSError::errorWithDomain_code_userInfo(
                    &NSString::from_str("TastyRemoteEntry"),
                    1,
                    None,
                )
            };
            // SAFETY: active tasks have not stopped/finished; remove before reentrant callbacks.
            unsafe { task.didFailWithError(&error) };
        }
    }
    pub(in crate::host_api::webview) fn poll(&self) {
        // Never borrow pending across a WebKit callback (stop may reenter).
        let pending: Vec<_> = self.ivars().pending.borrow_mut().drain().collect();
        for (key, mut pending) in pending {
            if !self.ivars().active.borrow().contains(&key) {
                continue;
            }
            match self.ivars().source.poll_response(&mut pending.wait) {
                Poll::Pending => {
                    self.ivars().pending.borrow_mut().insert(key, pending);
                }
                Poll::Ready(Ok(body)) => self.deliver(key, &pending.task, body),
                Poll::Ready(Err(_)) => self.fail(key, &pending.task),
            }
        }
    }
    pub(in crate::host_api::webview) fn close(&self) {
        let pending: Vec<_> = self.ivars().pending.borrow_mut().drain().collect();
        for (key, pending) in pending {
            self.fail(key, &pending.task);
        }
        self.ivars().source.wake();
    }
    fn deliver(
        &self,
        key: usize,
        task: &ProtocolObject<dyn WKURLSchemeTask>,
        body: Arc<NativeBodyResponse>,
    ) {
        if !self.ivars().source.is_current() {
            self.fail(key, task);
            return;
        }
        // SAFETY: owned active main-thread task. Reentrant stop is checked after every callback.
        #[allow(clippy::multiple_unsafe_ops_per_block)]
        unsafe {
            let Some(url) = task.request().URL() else {
                self.fail(key, task);
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
                NSString::from_str(self.ivars().source.csp()),
                NSString::from_str("no-store"),
                NSString::from_str("nosniff"),
            ];
            let headers = NSDictionary::from_slices(
                &keys.iter().map(|s| &**s).collect::<Vec<_>>(),
                &values.iter().map(|s| &**s).collect::<Vec<_>>(),
            );
            let Some(response) = NSHTTPURLResponse::initWithURL_statusCode_HTTPVersion_headerFields(
                NSHTTPURLResponse::alloc(),
                &url,
                200,
                Some(&NSString::from_str("HTTP/1.1")),
                Some(&headers),
            ) else {
                if self.ivars().active.borrow_mut().remove(&key) {
                    task.didFailWithError(&NSError::errorWithDomain_code_userInfo(
                        &NSString::from_str("TastyRemoteEntry"),
                        2,
                        None,
                    ));
                }
                return;
            };
            if !self.ivars().source.is_current() {
                self.fail(key, task);
                return;
            }
            if !self.ivars().active.borrow().contains(&key) {
                return;
            }
            task.didReceiveResponse(&response);
            if !self.ivars().source.is_current() {
                self.fail(key, task);
                return;
            }
            if !self.ivars().active.borrow().contains(&key) {
                return;
            }
            let data = NSData::with_bytes(body.bytes());
            task.didReceiveData(&data);
            if !self.ivars().source.is_current() {
                self.fail(key, task);
                return;
            }
            if self.ivars().active.borrow_mut().remove(&key) {
                task.didFinish();
            }
        }
    }
}
/// Configuration retains its scheme handler before the first WKWebView is created.
pub(in crate::host_api::webview) fn configure(
    config: &WKWebViewConfiguration,
    source: EntryResponseSource,
    mtm: MainThreadMarker,
) -> Retained<EntryHandler> {
    let this = EntryHandler::alloc(mtm).set_ivars(Ivars {
        source,
        active: RefCell::default(),
        pending: RefCell::default(),
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
    handler
}
