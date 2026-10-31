use super::*;
/// `direct_paths` checks the view as the host prepares it. The `_header_on` and `_meta_on` cases
/// serve a document that asks the engine to re-enable DNS prefetch. `direct_paths_control`
/// re-enables the features on the same view and only records the page result; the fixture does
/// not require it to produce traffic, because a WebKitGTK build without WebRTC produces none.
pub(super) fn run(case: &str) {
    let live = Arc::new(AtomicBool::new(true));
    let store = data_store();
    let view = prepared(&store, &live);
    let settings = WebViewExt::settings(view.webview()).unwrap();
    assert!(!settings.enables_webrtc());
    assert!(!settings.enables_media_stream());
    assert!(!settings.enables_dns_prefetching());
    let control = case == "direct_paths_control";
    if control {
        settings.set_enable_webrtc(true);
        settings.set_enable_media_stream(true);
        settings.set_enable_dns_prefetching(true);
    }
    // Scripts run as in a document the user allowed; WebRTC needs them, DNS prefetch does not.
    settings.set_enable_javascript(true);
    let window = window(&view);
    let path = match case {
        "direct_paths_header_on" => "/direct-header-on",
        "direct_paths_meta_on" => "/direct-meta-on",
        _ => "/direct",
    };
    view.webview().load_uri(&fixture_url("HTTP", path));
    wait(|| {
        view.webview()
            .title()
            .is_some_and(|t| t.starts_with("DIRECT "))
    });
    let title = view.webview().title().map(|t| t.to_string()).unwrap();
    tracing::info!("DIRECT_RESULT case={case} title={title}");
    if !control {
        assert!(
            title.starts_with("DIRECT peer=undefined "),
            "WebRTC is exposed: {title}"
        );
    }
    view.close();
    destroy(&window);
    drop(view);
    store.close();
}
