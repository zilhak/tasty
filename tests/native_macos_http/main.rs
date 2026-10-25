//! Input-free native fixture using an owned hidden NSWindow.
//! No activation, default stores, or global settings.
#[path = "../../src/host_api/webview/http_context/macos.rs"]
pub mod macos;
use macos::{DataStore, ProxyRoute};
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{MainThreadMarker, MainThreadOnly, runtime::ProtocolObject};
use objc2_app_kit::{
    NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSDate, NSRect, NSRunLoop, NSString, NSURL, NSURLRequest};
use objc2_web_kit::WKNavigationDelegate;
use objc2_web_kit::{WKWebView, WKWebViewConfiguration};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    time::{Duration, Instant},
};
objc2::define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[ivars = objc2::rc::Retained<macos::NavigationDelegate>]
    struct FixtureDelegate;
    // SAFETY: main-thread NSObject subclass implements the declared WebKit callback signatures.
    unsafe impl NSObjectProtocol for FixtureDelegate {}
    // SAFETY: main-thread NSObject subclass implements the declared WebKit callback signatures.
    unsafe impl WKNavigationDelegate for FixtureDelegate {
        #[unsafe(method(webView:didReceiveAuthenticationChallenge:completionHandler:))]
        fn challenge(&self, _view: &WKWebView, challenge: &objc2_foundation::NSURLAuthenticationChallenge, done: &block2::DynBlock<dyn Fn(objc2_foundation::NSURLSessionAuthChallengeDisposition, *mut objc2_foundation::NSURLCredential)>) {
            use objc2::DefinedClass;

            self.ivars().respond(challenge, done);
        }
    }
);
fn spin_until(mut ready: impl FnMut() -> bool) {
    let start = Instant::now();
    while !ready() {
        assert!(
            start.elapsed() < Duration::from_secs(30),
            "native callback deadline"
        );
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.01));
    }
}
fn eval(view: &WKWebView, script: &str) -> String {
    let value = Rc::new(RefCell::new(None));
    let result = value.clone();
    let callback = block2::RcBlock::new(
        move |object: *mut objc2::runtime::AnyObject, error: *mut objc2_foundation::NSError| {
            if !error.is_null() {
                // SAFETY: WebKit supplies a live NSError only for this callback duration.
                let error = unsafe { &*error };
                *result.borrow_mut() = Some(Err(format!("{error:?}")));
                return;
            }
            let text = if object.is_null() {
                "null".into()
            } else {
                // SAFETY: WebKit returns a live JS string/number NSObject for callback duration.
                let description: objc2::rc::Retained<NSString> =
                    // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
                    unsafe { objc2::msg_send![object, description] };
                description.to_string()
            };
            *result.borrow_mut() = Some(Ok(text));
        },
    );
    // SAFETY: main-thread fixture view and copied completion block.
    unsafe {
        view.evaluateJavaScript_completionHandler(&NSString::from_str(script), Some(&callback));
    }
    spin_until(|| value.borrow().is_some());
    value
        .borrow_mut()
        .take()
        .unwrap()
        .expect("native JavaScript evaluation")
}
fn load(view: &WKWebView, uri: &str) {
    let url = NSURL::URLWithString(&NSString::from_str(uri)).unwrap();
    // SAFETY: same native request travels unmodified to WKWebView.
    unsafe {
        view.loadRequest(&NSURLRequest::requestWithURL(&url));
    }
    // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
    spin_until(|| !unsafe { view.isLoading() });
}
fn storage_controls(
    a: &DataStore,
    b: &DataStore,
    config: &WKWebViewConfiguration,
    view: &WKWebView,
    window: &NSWindow,
    mtm: MainThreadMarker,
) {
    let base = NSURL::URLWithString(&NSString::from_str("https://storage.invalid/")).unwrap();
    // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
    unsafe {
        view.loadHTMLString_baseURL(
            &NSString::from_str("<body>storage fixture</body>"),
            Some(&base),
        );
    }
    spin_until(|| eval(view, "document.body.innerText") == "storage fixture");
    assert_eq!(
        eval(
            view,
            "document.cookie='isolated=yes; SameSite=Lax; Path=/';localStorage.setItem('keep','yes');'ready'"
        ),
        "ready"
    );
    assert!(eval(view, "document.cookie").contains("isolated=yes"));
    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    a.clear_selected(move |result| {
        result.unwrap();
        finished.set(true);
    })
    .unwrap();
    assert_eq!(a.configure(config), Err(macos::ContextError::Busy));
    spin_until(|| done.get());
    assert!(eval(view, "document.cookie").contains("isolated=yes"));
    assert_eq!(eval(view, "localStorage.getItem('keep')"), "yes");

    // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
    let config_b = unsafe { WKWebViewConfiguration::new(mtm) };
    b.configure(&config_b).unwrap();
    // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
    let view_b = unsafe {
        WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &config_b)
    };
    window.setContentView(Some(&view_b));
    // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
    unsafe {
        view_b.loadHTMLString_baseURL(&NSString::from_str("<body>other</body>"), Some(&base));
    }
    spin_until(|| eval(&view_b, "document.body.innerText") == "other");
    assert_eq!(eval(&view_b, "document.cookie"), "");
    assert_eq!(eval(&view_b, "localStorage.getItem('keep')"), "<null>");

    let done = Rc::new(Cell::new(false));
    let finished = done.clone();
    a.clear_selected(move |result| {
        assert_eq!(result, Err(macos::ContextError::Retired));
        finished.set(true);
    })
    .unwrap();
    a.close_view(view);
    spin_until(|| done.get());

    b.close_view(&view_b);
    window.setContentView(None);
}
fn main() {
    objc2::rc::autoreleasepool(|_| {
        let mtm = MainThreadMarker::new().expect("native fixture main thread");
        let app = NSApplication::sharedApplication(mtm);

        assert_eq!(
            app.activationPolicy(),
            NSApplicationActivationPolicy::Prohibited
        );
        app.finishLaunching();
        let port = std::env::var("FIXTURE_PROXY_PORT").unwrap_or_else(|_| "9".into());
        let target_host = if std::env::args().any(|arg| arg == "--domain-control") {
            "remote.invalid"
        } else {
            "localhost"
        };
        let target_port = std::env::var("FIXTURE_TARGET_PORT").unwrap_or_else(|_| "9".into());
        let make = || {
            DataStore::new(
                ProxyRoute::from_endpoint(&format!("socks5://fixture:secret@127.0.0.1:{port}"))
                    .unwrap(),
                Rc::new(|| true),
                mtm,
            )
            .unwrap()
        };
        let a = make();
        let b = make();
        assert!(!std::ptr::eq(a.native(), b.native()));
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        assert!(!unsafe { a.native().isPersistent() });

        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        let config = unsafe { WKWebViewConfiguration::new(mtm) };
        a.configure(&config).unwrap();
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        assert!(std::ptr::eq(a.native(), &*unsafe {
            config.websiteDataStore()
        }));
        let proxies: objc2::rc::Retained<objc2_foundation::NSArray<objc2::runtime::AnyObject>> =
            // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
            unsafe { objc2::msg_send![a.native(), proxyConfigurations] };
        assert_eq!(proxies.len(), 1);

        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        let delegate = unsafe {
            objc2::msg_send![
                super(FixtureDelegate::alloc(mtm).set_ivars(a.delegate(mtm))),
                init
            ]
        };
        let delegate: objc2::rc::Retained<FixtureDelegate> = delegate;
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        let view = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &config)
        };
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        unsafe {
            view.setNavigationDelegate(Some(ProtocolObject::from_ref(&*delegate)));
        }
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                objc2_foundation::NSRect::new(
                    objc2_foundation::NSPoint::ZERO,
                    objc2_foundation::NSSize::new(640.0, 480.0),
                ),
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        window.setContentView(Some(&view));
        assert!(!window.isVisible());
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        unsafe {
            view.loadHTMLString_baseURL(&NSString::from_str("<body>baseline</body>"), None);
        }

        assert_eq!(eval(&view, "'baseline-js'"), "baseline-js");
        if std::env::args().any(|arg| arg == "--storage-controls") {
            storage_controls(&a, &b, &config, &view, &window, mtm);
            return;
        }
        load(&view, &format!("http://{target_host}:{target_port}/set"));
        assert_eq!(eval(&view, "document.body.innerText"), "remote fixture");

        assert_eq!(eval(&view, "document.body.innerText"), "remote fixture");
        assert!(eval(&view, "document.cookie").contains("isolated=yes"));
        assert_eq!(
            eval(
                &view,
                "localStorage.setItem('keep','yes'); localStorage.getItem('keep')"
            ),
            "yes"
        );

        let done = Rc::new(Cell::new(false));
        let finished = done.clone();
        a.clear_selected(move |result| {
            result.unwrap();
            finished.set(true);
        })
        .unwrap();
        spin_until(|| done.get());
        assert!(eval(&view, "document.cookie").contains("isolated=yes"));
        assert_eq!(eval(&view, "localStorage.getItem('keep')"), "yes");

        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        let config_b = unsafe { WKWebViewConfiguration::new(mtm) };
        b.configure(&config_b).unwrap();
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        let view_b = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), NSRect::ZERO, &config_b)
        };
        load(&view_b, &format!("http://{target_host}:{target_port}/read"));
        assert_eq!(eval(&view_b, "document.cookie"), "");
        assert_eq!(eval(&view_b, "localStorage.getItem('keep')"), "<null>");

        let url = NSURL::URLWithString(&NSString::from_str(&format!(
            "http://{target_host}:{target_port}/post"
        )))
        .unwrap();
        let request = objc2_foundation::NSMutableURLRequest::requestWithURL(&url);
        request.setHTTPMethod(&NSString::from_str("POST"));
        request.setHTTPBody(Some(&objc2_foundation::NSData::with_bytes(
            b"exact\0POST\xff",
        )));
        // SAFETY: main-thread fixture uses retained native objects and the public SDK signature.
        unsafe {
            view.loadRequest(&request);
        }
        spin_until(|| eval(&view, "document.body.innerText") == "POST exact");

        load(&view, &format!("http://localhost:{target_port}/loopback"));
        assert_eq!(eval(&view, "document.body.innerText"), "remote fixture");

        let done = Rc::new(Cell::new(false));
        let finished = done.clone();
        a.clear_selected(move |result| {
            assert_eq!(result, Err(macos::ContextError::Retired));
            finished.set(true);
        })
        .unwrap();
        a.close_view(&view);
        spin_until(|| done.get());
        assert!(!a.is_current());

        b.close_view(&view_b);
        window.setContentView(None);
    });
}
