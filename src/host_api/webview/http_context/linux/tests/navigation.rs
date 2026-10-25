use super::*;
pub(super) fn post(view: &PreparedView) {
    post_to(view, fixture_port("HTTPS"));
}
fn post_to(view: &PreparedView, port: u16) {
    let html = r#"<!doctype html><title>pending</title><form method="POST" enctype="multipart/form-data" action="https://localhost:8443/post"><input type="file" name="payload"></form><script>const d=new DataTransfer();d.items.add(new File([new Uint8Array([0,255,1,128,13,10,42])],"owned.bin",{type:"application/octet-stream"}));document.querySelector('input').files=d.files;document.forms[0].submit();</script>"#;
    view.webview().load_html(
        &html.replace(":8443/post", &format!(":{port}/post")),
        Some(&fixture_url("HTTP", "/bootstrap")),
    );
}
pub(super) fn run(case: &str) {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    if case == "history" {
        for n in 0..allowance::LIMIT - 1 {
            let mut leaf = [0; 32];
            leaf[..8].copy_from_slice(&(n as u64).to_le_bytes());
            let mut permit = allowance::History::reserve(&store.0.history, leaf)
                .unwrap_or_else(|e| panic!("{e:?}"));
            permit.commit();
        }
    }
    let view = prepared(&store, &live);
    let window = window(&view);
    WebSettingsExt::set_enable_javascript(&WebViewExt::settings(view.webview()).unwrap(), true);
    if case == "bound" {
        bound(&view);
    } else {
        post(&view);
    }
    if matches!(case, "retired" | "closed") {
        wait(|| !view.0.pending.borrow().is_empty());
        if case == "retired" {
            live.store(false, Ordering::Release);
        } else {
            view.close();
        }
    }
    if matches!(case, "valid" | "repeat" | "port" | "history") || case.starts_with("fresh") {
        successful(case, &store, &view);
    } else if !matches!(case, "closed" | "bound") {
        wait(|| view.0.failure.get().is_some());
    }
    rejected_tls(case, &store, &view);
    tracing::info!(
        "TLS_OBS flags={:?} probes={} trusted={}",
        view.0.observations.borrow(),
        view.0.sequence.get(),
        view.0.trusted.borrow().len()
    );
    view.close();
    assert!(view.0.pending.borrow().is_empty());
    assert!(view.0.resuming.borrow().is_empty());
    destroy(&window);
    drop(view);
    if case.starts_with("fresh") {
        live.store(false, Ordering::Release);
        fresh_pin(&store, case);
    }
    store.close();
}

fn rejected_tls(case: &str, store: &DataStore, view: &PreparedView) {
    if matches!(case, "wrong_leaf" | "extra_flags") {
        assert_eq!(view.0.failure.get(), Some(ContextError::Tls));
        assert_eq!(view.0.sequence.get(), 1);
        let flags = if case == "extra_flags" {
            (gio::TlsCertificateFlags::UNKNOWN_CA | gio::TlsCertificateFlags::BAD_IDENTITY).bits()
        } else {
            gio::TlsCertificateFlags::UNKNOWN_CA.bits()
        };
        assert_eq!(&*view.0.observations.borrow(), &[flags]);
        assert!(view.0.trusted.borrow().is_empty());
        assert_eq!(store.0.history.borrow().used(), 0);
    }
}

fn bound(view: &PreparedView) {
    let frames = (0..9)
        .map(|n| {
            format!(
                "<iframe src='https://p{n}.localhost:{}/hold'></iframe>",
                fixture_port("HTTPS")
            )
        })
        .collect::<String>();
    view.webview()
        .load_html(&frames, Some(&fixture_url("HTTP", "/bootstrap")));
    wait(|| {
        if let Some(error) = view.0.failure.get() {
            assert_eq!(
                error,
                ContextError::Capacity,
                "pending={} sequence={}",
                view.0.pending.borrow().len(),
                view.0.sequence.get()
            );
            true
        } else {
            false
        }
    });
    assert_eq!(view.0.pending.borrow().len(), tls::LIMIT);
    assert_eq!(view.0.sequence.get(), tls::LIMIT as u64);
}

fn fresh_pin(store: &DataStore, case: &str) {
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    let root = std::path::PathBuf::from(std::env::var("DE_FIXTURE_ROOT").unwrap());
    std::fs::write(root.join("retire-endpoint"), b"retire").unwrap();
    wait(|| root.join("next-proxy").exists());
    let mut cfg = config(Arc::new(AtomicBool::new(true)));
    cfg.proxy_uri = std::fs::read_to_string(root.join("next-proxy")).unwrap();
    let expected = std::fs::read_to_string(root.join("next-expected")).unwrap();
    let expected: [u8; 32] = (0..32)
        .map(|i| u8::from_str_radix(&expected[i * 2..i * 2 + 2], 16).unwrap())
        .collect::<Vec<_>>()
        .try_into()
        .unwrap();
    cfg.expected_leaf = Arc::new(move |_, _| Some(expected));
    store.prepare(cfg, Box::new(move |r| *out.borrow_mut() = Some(r)));
    wait(|| result.borrow().is_some());
    let next = result
        .borrow_mut()
        .take()
        .unwrap()
        .unwrap_or_else(|e| panic!("prepare: {e:?}"));
    if let Some(settings) = WebViewExt::settings(next.webview()) {
        WebSettingsExt::set_enable_page_cache(&settings, false);
    }
    tracing::info!(
        "NATIVE_CONTEXT generation=2 context={:p} manager={:p} current={}",
        next.0.context.as_ptr(),
        store.0.manager.as_ptr(),
        next.0.live()
    );
    let window = window(&next);
    post(&next);
    if case == "fresh_changed_valid" {
        wait(|| next.webview().title().as_deref() == Some("POST RETAINED"));
    } else {
        wait(|| next.0.failure.get().is_some());
    }
    tracing::info!("FRESH_TLS_OBS flags={:?}", next.0.observations.borrow());
    if case == "fresh_changed_valid" {
        assert_eq!(next.0.sequence.get(), 1);
        assert_eq!(&*next.0.held_methods.borrow(), &["POST"]);
        assert_eq!(
            &*next.0.observations.borrow(),
            &[gio::TlsCertificateFlags::UNKNOWN_CA.bits()]
        );
        assert_eq!(store.0.history.borrow().used(), 2);
    } else {
        // The manager has already allowed this expected leaf. Refuse it before
        // probing, even when the endpoint now presents a different certificate.
        assert_eq!(next.0.failure.get(), Some(ContextError::Tls));
        assert_eq!(next.0.sequence.get(), 0);
        assert!(next.0.observations.borrow().is_empty());
        assert!(next.0.held_methods.borrow().is_empty());
        assert!(next.0.trusted.borrow().is_empty());
        assert_eq!(store.0.history.borrow().used(), 1);
    }
    tracing::info!(
        "FRESH_POLICY sequence={} trusted={} history={} failure={:?}",
        next.0.sequence.get(),
        next.0.trusted.borrow().len(),
        store.0.history.borrow().used(),
        next.0.failure.get()
    );
    next.close();
    destroy(&window);
}

fn successful(case: &str, store: &DataStore, view: &PreparedView) {
    wait(|| view.webview().title().as_deref() == Some("POST RETAINED"));
    assert_eq!(view.0.sequence.get(), 1);
    assert_eq!(&*view.0.held_methods.borrow(), &["POST"]);
    assert_eq!(
        &*view.0.observations.borrow(),
        &[gio::TlsCertificateFlags::UNKNOWN_CA.bits()]
    );
    if case == "repeat" {
        post(view);
        wait(|| view.webview().title().as_deref() == Some("pending"));
        wait(|| view.webview().title().as_deref() == Some("POST RETAINED"));
        assert_eq!(view.0.sequence.get(), 1);
        assert_eq!(store.0.history.borrow().used(), 1);
    }
    if case == "port" {
        post_to(view, fixture_port("OTHER"));
        wait(|| view.webview().title().as_deref() == Some("pending"));
        wait(|| view.webview().title().as_deref() == Some("POST RETAINED"));
        assert_eq!(view.0.sequence.get(), 2);
        assert_eq!(store.0.history.borrow().used(), 2);
        assert_eq!(view.0.observations.borrow().len(), 2);
    }
    if case == "history" {
        assert_eq!(store.0.history.borrow().used(), allowance::LIMIT);
        post_to(view, fixture_port("OTHER"));
        wait(|| {
            if let Some(error) = view.0.failure.get() {
                assert_eq!(
                    error,
                    ContextError::Capacity,
                    "pending={} sequence={}",
                    view.0.pending.borrow().len(),
                    view.0.sequence.get()
                );
                true
            } else {
                false
            }
        });
        assert_eq!(view.0.sequence.get(), 1);
        assert_eq!(store.0.history.borrow().used(), allowance::LIMIT);
    }
}
