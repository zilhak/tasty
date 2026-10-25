use super::*;
type Receipt = Rc<RefCell<Vec<Result<ReadyView, ContextError>>>>;
fn start(store: &DataStore, view: &PreparedView, config: ContextConfig) -> (u64, Receipt) {
    let token = store
        .retire_document(view, view.reload_continuation())
        .unwrap_or_else(|e| panic!("retire: {e:?}"));
    let id = token.id;
    let result = Rc::new(RefCell::new(Vec::new()));
    let out = result.clone();
    token.after_endpoint_join(config, Box::new(move |r| out.borrow_mut().push(r)));
    (id, result)
}
fn take(receipt: &Receipt) -> Result<ReadyView, ContextError> {
    wait(|| !receipt.borrow().is_empty());
    assert_eq!(receipt.borrow().len(), 1);
    receipt.borrow_mut().remove(0)
}
fn loaded() -> (DataStore, PreparedView, gtk::Window, Arc<AtomicBool>) {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let view = prepared(&store, &live);
    let window = window(&view);
    view.webview().load_uri(&fixture_url("HTTP", "/base"));
    wait(|| view.webview().title().as_deref() == Some("POST RETAINED"));
    (store, view, window, live)
}
pub(super) fn lifecycle() {
    let (store, view, window, live) = loaded();
    let original = view.webview().as_ptr();
    let token = store
        .retire_document(&view, view.reload_continuation())
        .unwrap_or_else(|e| panic!("{e:?}"));
    assert!(view.0.blocked.get());
    assert!(matches!(
        store.retire_document(&view, view.reload_continuation()),
        Err(ContextError::Busy)
    ));
    let busy = Rc::new(Cell::new(false));
    let out = busy.clone();
    store.prepare(
        config(live.clone()),
        Box::new(move |r| {
            assert!(matches!(r, Err(ContextError::Busy)));
            out.set(true);
        }),
    );
    assert!(busy.get());
    drop(token);
    assert!(view.0.blocked.get());
    let mut cfg = config(live.clone());
    cfg.proxy_uri = "http://invalid/".into();
    let (_, invalid) = start(&store, &view, cfg);
    assert!(matches!(take(&invalid), Err(ContextError::Policy)));
    let (first_id, failed) = start(&store, &view, config(live.clone()));
    assert!(failed.borrow().is_empty());
    // Clear-result injection, not an OS I/O error; old real completion stays queued.
    store.0.finish_retained(first_id, false);
    assert!(matches!(take(&failed), Err(ContextError::Cache)));
    assert!(
        matches!(
            store.retire_document(&view, view.reload_continuation()),
            Err(ContextError::Busy)
        ),
        "logical cancellation must not release the in-flight native clear owner"
    );
    wait(|| !store.0.native_busy());
    let (_, next) = start(&store, &view, config(live.clone()));
    store.0.native_finished(first_id);
    store.0.finish_retained(first_id, true);
    assert!(
        store.0.native_busy(),
        "old native completion cannot release the new clear owner"
    );
    assert!(
        next.borrow().is_empty(),
        "old completion cannot consume new callback"
    );
    drop(take(&next).unwrap_or_else(|e| panic!("{e:?}")));
    assert!(view.0.blocked.get(), "abandoned Ready must not navigate");
    let (_, stale) = start(&store, &view, config(live.clone()));
    let ready = take(&stale).unwrap_or_else(|e| panic!("{e:?}"));
    live.store(false, Ordering::Release);
    assert_eq!(ready.resume(), Err(ContextError::Retired));
    live.store(true, Ordering::Release);
    close_during_clear(&store, &view, live);
    assert_eq!(view.webview().as_ptr(), original);
    destroy(&window);
    store.close();
    owner_bounds();
}
fn close_during_clear(store: &DataStore, view: &PreparedView, live: Arc<AtomicBool>) {
    let (_, closing) = start(store, view, config(live.clone()));
    view.close();
    assert!(matches!(take(&closing), Err(ContextError::Retired)));
    assert!(closing.borrow().is_empty());
    assert!(store.0.native_busy());
    let busy = Rc::new(Cell::new(false));
    let out = busy.clone();
    store.prepare(
        config(live.clone()),
        Box::new(move |r| {
            assert!(matches!(r, Err(ContextError::Busy)));
            out.set(true);
        }),
    );
    assert!(
        busy.get(),
        "closed logical view cannot bypass an unfinished native clear"
    );
    wait(|| !store.0.native_busy());
    assert!(
        closing.borrow().is_empty(),
        "actual late native callback must not reply twice"
    );
    let next_view = prepared(store, &live);
    next_view.close();
}

fn owner_bounds() {
    let (store, view, window, live) = loaded();
    let other = data_store();
    let other_view = prepared(&other, &live);
    assert!(matches!(
        store.retire_document(&other_view, other_view.reload_continuation()),
        Err(ContextError::Policy)
    ));
    assert!(matches!(
        store.retire_document(&view, other_view.reload_continuation()),
        Err(ContextError::Policy)
    ));
    let (_, closing) = start(&store, &view, config(live));
    store.close();
    assert!(matches!(take(&closing), Err(ContextError::Retired)));
    destroy(&window);
    other_view.close();
    other.close();
    let (store, view, window, live) = loaded();
    for n in 0..allowance::LIMIT {
        let mut leaf = [0; 32];
        leaf[..8].copy_from_slice(&(n as u64).to_le_bytes());
        let mut permit =
            allowance::History::reserve(&store.0.history, leaf).unwrap_or_else(|e| panic!("{e:?}"));
        permit.commit();
    }
    let (_, full) = start(&store, &view, config(live));
    assert!(matches!(take(&full), Err(ContextError::Capacity)));
    assert_eq!(store.0.history.borrow().used(), allowance::LIMIT);
    view.close();
    destroy(&window);
    store.close();
}

fn replacement_config(root: &std::path::Path, live: Arc<AtomicBool>) -> ContextConfig {
    wait(|| root.join("next-proxy").exists());
    let expected = std::fs::read_to_string(root.join("next-expected")).unwrap();
    let expected: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&expected[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    let mut config = config(live);
    config.proxy_uri = std::fs::read_to_string(root.join("next-proxy")).unwrap();
    config.expected_leaf = Arc::new(move |host, port| {
        (host == "localhost" && port == fixture_port("HTTPS")).then_some(expected)
    });
    config
}
pub(super) fn post_cookie(case: &str) {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let view = prepared(&store, &live);
    let original = view.webview().as_ptr();
    let window = window(&view);
    WebSettingsExt::set_enable_javascript(&WebViewExt::settings(view.webview()).unwrap(), true);
    view.webview()
        .load_uri(&fixture_url("HTTP", "/retained-seed"));
    wait(|| view.webview().title().as_deref() == Some("CACHE READY"));
    if case == "retained_no_clear" {
        view.webview().reload_bypass_cache();
        wait(|| view.webview().title().as_deref() == Some("SW OLD"));
    } else {
        let token = store
            .retire_document(&view, view.reload_continuation())
            .unwrap_or_else(|e| panic!("{e:?}"));
        live.store(false, Ordering::Release);
        let root = std::path::PathBuf::from(std::env::var("DE_FIXTURE_ROOT").unwrap());
        std::fs::write(root.join("retire-endpoint"), b"retire").unwrap();
        // The driver writes next-proxy only AFTER closing and joining old sockets.
        let cfg = replacement_config(&root, Arc::new(AtomicBool::new(true)));
        let result = Rc::new(RefCell::new(Vec::new()));
        let out = result.clone();
        token.after_endpoint_join(cfg, Box::new(move |r| out.borrow_mut().push(r)));
        assert!(result.borrow().is_empty());
        let ready = take(&result).unwrap_or_else(|e| panic!("{e:?}"));
        assert_eq!(view.webview().as_ptr(), original);
        assert_eq!(view.webview().title().as_deref(), Some("CACHE READY"));
        assert!(view.0.blocked.get());
        ready.resume().unwrap();
        wait(|| {
            view.webview()
                .title()
                .is_some_and(|t| t.starts_with("CACHE CLEARED") || t.starts_with("FAIL"))
        });
        assert_eq!(
            view.webview().title().as_deref(),
            Some("CACHE CLEARED COOKIE KEPT")
        );
        assert_eq!(view.webview().as_ptr(), original);
    }
    tracing::info!(
        "RETAINED_VIEW_SAME pointer={original:p} title={:?}",
        view.webview().title()
    );
    view.close();
    destroy(&window);
    store.close();
}

fn joined_config() -> ContextConfig {
    let root = std::path::PathBuf::from(std::env::var("DE_FIXTURE_ROOT").unwrap());
    std::fs::write(root.join("retire-endpoint"), b"retire").unwrap();
    replacement_config(&root, Arc::new(AtomicBool::new(true)))
}
fn cleared(token: RetiredView, config: ContextConfig) -> ReadyView {
    let result = Rc::new(RefCell::new(Vec::new()));
    let out = result.clone();
    token.after_endpoint_join(config, Box::new(move |r| out.borrow_mut().push(r)));
    assert!(result.borrow().is_empty());
    take(&result).unwrap_or_else(|e| panic!("clear: {e:?}"))
}
pub(super) fn https() {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let view = prepared(&store, &live);
    let window = window(&view);
    super::navigation::post(&view);
    wait(|| view.webview().title().as_deref() == Some("POST RETAINED"));
    assert_eq!(store.0.history.borrow().used(), 1);
    let cfg = joined_config();
    let mut missing = cfg.clone();
    missing.expected_leaf = Arc::new(|_, _| None);
    let (_, failure) = start(&store, &view, missing);
    assert!(matches!(take(&failure), Err(ContextError::Tls)));
    let (_, reused) = start(&store, &view, config(live.clone()));
    assert!(matches!(take(&reused), Err(ContextError::Tls)));
    assert_eq!(store.0.history.borrow().used(), 1);
    let token = store
        .retire_document(&view, view.reload_continuation())
        .unwrap_or_else(|e| panic!("{e:?}"));
    let original = view.webview().as_ptr();
    let ready = cleared(token, cfg);
    assert_eq!(
        view.0.sequence.get(),
        1,
        "barrier must not create speculative probe/network request"
    );
    ready.resume().unwrap();
    wait(|| {
        store.0.history.borrow().used() == 2
            && !view.webview().is_loading()
            && view.webview().title().as_deref() == Some("POST RETAINED")
    });
    assert_eq!(store.0.history.borrow().used(), 2);
    assert_eq!(view.webview().as_ptr(), original);
    assert_eq!(&*view.0.held_methods.borrow(), &["POST", "POST"]);
    assert_eq!(&*view.0.observations.borrow(), &[1, 1]);
    tracing::info!("RETAINED_HTTPS history=2 probes=2 original_POST=2");
    view.close();
    destroy(&window);
    store.close();
}
pub(super) fn held(cancel: bool) {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let view = Rc::new(prepared(&store, &live));
    let window = window(&view);
    let original = view.webview().as_ptr();
    let action = Rc::new(RefCell::new(None));
    let out = action.clone();
    let weak = Rc::downgrade(&view);
    let handler = view
        .webview()
        .connect_decide_policy(move |_, decision, kind| {
            if kind != webkit2gtk::PolicyDecisionType::NavigationAction {
                return false;
            }
            let Some(nav) = decision.downcast_ref::<webkit2gtk::NavigationPolicyDecision>() else {
                return false;
            };
            let method = nav
                .navigation_action()
                .and_then(|a| a.request())
                .and_then(|r| r.http_method());
            if method.as_deref() != Some("POST") {
                return false;
            }
            assert!(out.borrow().is_none());
            *out.borrow_mut() = Some(weak.upgrade().unwrap().retain_navigation(nav));
            true
        });
    view.webview()
        .load_uri(&fixture_url("HTTP", "/retained-held-seed"));
    wait(|| action.borrow().is_some());
    view.webview().disconnect(handler);
    let token = store
        .retire_document(&view, action.borrow_mut().take().unwrap())
        .unwrap_or_else(|e| panic!("{e:?}"));
    if cancel {
        drop(token);
        wait(|| !view.webview().is_loading());
        assert!(view.0.blocked.get());
    } else {
        let ready = cleared(token, joined_config());
        assert_eq!(view.webview().as_ptr(), original);
        ready.resume().unwrap();
        wait(|| view.webview().title().as_deref() == Some("POST RETAINED"));
    }
    tracing::info!(
        "RETAINED_HELD original_POST_decision=true cancelled={cancel} same_view={}",
        view.webview().as_ptr() == original
    );
    view.close();
    destroy(&window);
    store.close();
}
pub(super) fn epoch() {
    let (store, view, window, _) = loaded();
    let old = view
        .webview()
        .main_resource()
        .expect("loaded native main resource");
    let root = std::path::PathBuf::from(std::env::var("DE_FIXTURE_ROOT").unwrap());
    let certificate =
        gio::TlsCertificate::from_file(root.parent().unwrap().join("owned.pem")).unwrap();
    let emit = |resource: &webkit2gtk::WebResource| {
        resource.emit_by_name::<()>(
            "failed-with-tls-errors",
            &[&certificate, &gio::TlsCertificateFlags::UNKNOWN_CA],
        )
    };
    // Synthetic delayed event on actual WebResource; first prove the handler is live.
    emit(&old);
    assert_eq!(view.0.failure.get(), Some(ContextError::Tls));
    let token = store
        .retire_document(&view, view.reload_continuation())
        .unwrap_or_else(|e| panic!("{e:?}"));
    cleared(token, joined_config()).resume().unwrap();
    wait(|| !view.webview().is_loading());
    let current = view.webview().main_resource().unwrap();
    assert_ne!(old, current);
    emit(&old);
    assert_eq!(
        view.0.failure.get(),
        None,
        "retired resource cannot fail new document"
    );
    assert!(view.0.live());
    emit(&current);
    assert_eq!(
        view.0.failure.get(),
        Some(ContextError::Tls),
        "current failure must remain visible"
    );
    tracing::info!("RETAINED_RESOURCE_EPOCH old_ignored=true current_failure_visible=true");
    view.close();
    destroy(&window);
    store.close();
}
