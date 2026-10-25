use super::*;
pub(super) fn run() {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    assert!(!store.0.manager.is_ephemeral());
    tracing::info!("CACHE_MANAGER ephemeral={}", store.0.manager.is_ephemeral());
    let first = prepared(&store, &live);
    let first_context = first.0.context.downgrade();
    let window1 = window(&first);
    first.webview().load_uri(&fixture_url("HTTP", "/install"));
    wait(|| first.webview().title().as_deref() == Some("CACHE READY"));
    tracing::info!("CACHE_OS_BEFORE {:?}", data_types(&store.0.manager));
    first.close();
    destroy(&window1);
    drop(first);
    wait(|| first_context.upgrade().is_none());
    let next = prepared(&store, &live);
    assert!(first_context.upgrade().is_none());
    let window2 = window(&next);
    tracing::info!("CACHE_OS_AFTER {:?}", data_types(&store.0.manager));
    next.webview().load_uri(&fixture_url("HTTP", "/check"));
    wait(|| {
        next.webview()
            .title()
            .is_some_and(|t| t.starts_with("CACHE CLEARED") || t.starts_with("FAIL"))
    });
    assert_eq!(
        next.webview().title().as_deref(),
        Some("CACHE CLEARED COOKIE KEPT")
    );
    next.close();
    destroy(&window2);
    store.close();
}

fn data_types(manager: &WebsiteDataManager) -> Vec<u32> {
    let result = Rc::new(RefCell::new(None));
    let out = result.clone();
    manager.fetch(
        store::clear_types(),
        None::<&gio::Cancellable>,
        move |data| {
            *out.borrow_mut() =
                Some(data.map(|items| items.iter().map(|item| item.types().bits()).collect()));
        },
    );
    wait(|| result.borrow().is_some());
    result.borrow_mut().take().unwrap().unwrap()
}
