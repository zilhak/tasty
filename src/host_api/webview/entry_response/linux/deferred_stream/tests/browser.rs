//! Actual WebKit request Abort and MIME/body observation using the production scheme adapter.
use super::*;
use std::{cell::Cell, collections::HashMap};
use webkit2gtk::{LoadEvent, URIResponseExt, WebResourceExt, WebViewExt};

type Observations = Rc<RefCell<Vec<(Option<String>, Result<Vec<u8>, glib::Error>)>>>;
fn observe_css(view: &webkit2gtk::WebView) -> Observations {
    let received: Observations = Rc::default();
    let out = received.clone();
    view.connect_resource_load_started(move |_, resource, _| {
        if resource.uri().is_none_or(|uri| !uri.ends_with("style.css")) {
            return;
        }
        let out = out.clone();
        let failed = Rc::new(Cell::new(false));
        let failure = failed.clone();
        resource.connect_failed(move |_, error| {
            tracing::info!(%error, "native CSS resource failed");
            failure.set(true);
        });
        resource.connect_finished(move |resource| {
            if failed.get() {
                return;
            }
            let Some(response) = resource.response() else {
                return;
            };
            let mime = response.mime_type().map(|s| s.to_string());
            let out = out.clone();
            WebResourceExt::data(resource, gio::Cancellable::NONE, move |data| {
                tracing::info!(?mime, bytes = ?data.as_ref().map(Vec::len), "native completed CSS");
                out.borrow_mut().push((mime, data))
            });
        });
    });
    received
}
fn start_fetches(view: &webkit2gtk::WebView) -> Rc<Cell<bool>> {
    let evaluated = Rc::new(Cell::new(false));
    let out = evaluated.clone();
    let started = Cell::new(false);
    view.connect_load_changed(move |view, event| {
        if event != LoadEvent::Finished || started.replace(true) { return; }
        let out = out.clone();
        view.evaluate_javascript("const a=new AbortController(), c=new AbortController(); fetch('abort.css',{signal:a.signal}).catch(()=>{}); fetch('style.css',{signal:c.signal}).catch(()=>{}); fetch('style.css').then(r=>r.arrayBuffer()).then(b=>fetch('proof.txt?bytes='+Array.from(new Uint8Array(b)).join(','))).catch(()=>{}); setTimeout(()=>{a.abort();c.abort()},100);", None, None, gio::Cancellable::NONE, move |result| {
            if let Err(error) = result { tracing::warn!(%error, "native fixture script failed"); } else { out.set(true); }
        });
    });
    evaluated
}
pub(super) fn exercise(context: &glib::MainContext) {
    let original = entry();
    let backing = store(original.clone());
    backing.lock().unwrap().enable_resources(true);
    let source = EntryResponseSource::new(&backing);
    let view = crate::webview::entry_response::linux::webview(source.clone());
    let received = observe_css(&view);
    let evaluated = start_fetches(&view);
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.add(&view);
    window.show_all();
    view.load_uri(original.native_uri());
    let start = Instant::now();
    let mut state = BrowserState::default();
    drain_until(context, || {
        state.step(&backing, start.elapsed());
        state.aborted
            && evaluated.get()
            && state.observed.is_some()
            && received.borrow().iter().any(|(_, data)| data.is_ok())
    });
    verify_consumed_body(&state, &received);
    // SAFETY: this fixture owns the window/view on the GTK thread; no native object is used after destroy.
    unsafe {
        window.destroy();
    }
    while context.pending() {
        context.iteration(false);
    }
    assert!(!source.is_current());
}
fn verify_consumed_body(state: &BrowserState, received: &Observations) {
    assert!(state.requests.contains_key("abort.css") && state.requests.contains_key("style.css"));
    assert!(!state.cancelled_style);
    let received = received.borrow();
    let (mime, bytes) = received.iter().find(|(_, data)| data.is_ok()).unwrap();
    assert_eq!(mime.as_deref(), Some("text/css"));
    tracing::info!(
        ?bytes,
        "WebResource data observation differs from canonical fetch-consumed bytes"
    );
    assert_eq!(
        state.observed.as_deref().unwrap(),
        b"body{background:green}"
    );
    tracing::info!(
        bytes = state.observed.as_ref().unwrap().len(),
        "native WebKit Abort and live same-URI sibling body verified"
    );
}
#[derive(Default)]
struct BrowserState {
    requests: HashMap<String, tasty_ipc::html_transfer::TransferId>,
    aborted: bool,
    cancelled_style: bool,
    published: bool,
    observed: Option<Vec<u8>>,
}
impl BrowserState {
    fn step(
        &mut self,
        backing: &Arc<std::sync::Mutex<tasty_remote::html_body::EntryBodyStore>>,
        elapsed: Duration,
    ) {
        let mut store = backing.lock().unwrap();
        let broker = store.resources_mut().unwrap();
        for fetch in broker.take_pending(8) {
            if fetch.relative == "proof.txt" {
                let query = fetch.query.as_deref().unwrap();
                let value = query
                    .strip_prefix("bytes=")
                    .unwrap()
                    .replace("%2C", ",")
                    .replace("%2c", ",");
                self.observed = Some(
                    value
                        .split(',')
                        .map(|byte| byte.parse::<u8>().unwrap())
                        .collect(),
                );
            }
            let request = tasty_ipc::html_transfer::TransferId {
                request: 10 + self.requests.len() as u64,
                transfer: 20 + self.requests.len() as u64,
                ..id()
            };
            assert!(broker.bind(fetch.ticket, request));
            self.requests.insert(fetch.relative, request);
        }
        for request in broker.abandoned() {
            self.aborted |= self.requests.get("abort.css") == Some(&request);
            self.cancelled_style |= self.requests.get("style.css") == Some(&request);
        }
        if !self.published
            && self.aborted
            && elapsed > Duration::from_millis(500)
            && let Some(request) = self.requests.get("style.css")
        {
            let response = NativeBodyResponse::new(
                *request,
                8,
                "text/css",
                "file:///server/style.css",
                Arc::new(b"body{background:green}".to_vec()),
            )
            .unwrap();
            assert!(broker.publish(*request, Arc::new(response)));
            self.published = true;
        }
    }
}
