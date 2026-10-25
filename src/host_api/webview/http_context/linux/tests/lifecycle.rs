use super::*;
fn result(
    store: &DataStore,
    live: &Arc<AtomicBool>,
) -> Rc<RefCell<Vec<Result<PreparedView, ContextError>>>> {
    let results = Rc::new(RefCell::new(Vec::new()));
    let out = results.clone();
    store.prepare(
        config(live.clone()),
        Box::new(move |r| out.borrow_mut().push(r)),
    );
    results
}
pub(super) fn run() {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let results = result(&store, &live);
    let busy = result(&store, &live);
    assert!(matches!(busy.borrow()[0], Err(ContextError::Busy)));
    store.close();
    store.close();
    assert!(matches!(results.borrow()[0], Err(ContextError::Retired)));
    wait(|| results.borrow().len() == 1);
    drop(store);
    let store = data_store();
    let results = result(&store, &live);
    live.store(false, Ordering::Release);
    wait(|| !results.borrow().is_empty());
    assert!(matches!(results.borrow()[0], Err(ContextError::Retired)));
    live.store(true, Ordering::Release);
    let failed = result(&store, &live);
    // Deliberate clear-result fault injection, distinct from a native clear error.
    store.0.finished(false);
    assert!(matches!(failed.borrow()[0], Err(ContextError::Cache)));
    store.close();
    let store = data_store();
    let view = prepared(&store, &live);
    let busy = result(&store, &live);
    assert!(matches!(busy.borrow()[0], Err(ContextError::Busy)));
    view.close();
    view.close();
    let next = prepared(&store, &live);
    next.close();
    store.close();
    retained_after_native_cancel();
}

fn retained_after_native_cancel() {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let manager = store.0.manager.clone();
    let data = std::path::PathBuf::from(manager.base_data_directory().unwrap());
    let cache = std::path::PathBuf::from(manager.base_cache_directory().unwrap());
    let pending = result(&store, &live);
    store.close();
    drop(store);
    assert!(matches!(pending.borrow()[0], Err(ContextError::Retired)));
    let completed = Rc::new(Cell::new(false));
    let done = completed.clone();
    manager.fetch(
        store::clear_types(),
        None::<&gio::Cancellable>,
        move |result| {
            assert!(result.is_ok());
            done.set(true);
        },
    );
    wait(|| completed.get());
    assert_eq!(
        pending.borrow().len(),
        1,
        "late native callback cannot reply twice"
    );
    drop(manager);
    // GObject disposal is not a network-process teardown acknowledgement. Exposed
    // roots must survive; filesystem inspection stays off the GTK thread.
    std::thread::spawn(move || {
        use std::os::unix::fs::PermissionsExt;
        assert_ne!(data, cache);
        for directory in [data, cache] {
            assert_eq!(
                std::fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
                0o700
            );
            assert!(directory.parent().unwrap().is_dir());
        }
    })
    .join()
    .expect("owned offthread retention check");
}
