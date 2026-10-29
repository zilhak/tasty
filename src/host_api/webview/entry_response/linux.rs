//! Isolated WebKit file responses. Pending requests never read client paths or use ambient network.
mod deferred_stream;
mod pending_reads;

use super::EntryResponseSource;
use gtk::prelude::*;
use gtk::{gio, glib};
use std::sync::Arc;
use tasty_remote::html_body::{ENTRY_SCHEME, NativeBodyResponse};

use webkit2gtk::{
    SecurityManagerExt, URIRequestExt, URISchemeRequest, URISchemeRequestExt, URISchemeResponse,
    URISchemeResponseExt, WebContext, WebContextExt, WebView, WebViewExt,
};
pub(crate) fn callbacks_pending() -> bool {
    pending_reads::callbacks_pending()
}
struct BodyBytes(Arc<NativeBodyResponse>);
impl AsRef<[u8]> for BodyBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.bytes()
    }
}
fn fail(request: &URISchemeRequest, reason: &str) {
    request.finish_error(&mut glib::Error::new(
        gio::IOErrorEnum::PermissionDenied,
        reason,
    ));
}
fn finish_headers(
    response: &URISchemeResponse,
    source: &EntryResponseSource,
    mime: &str,
    length: i64,
) {
    response.set_content_type(mime);
    response.set_status(200, Some("OK"));
    let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
    // WebKit's nosniff checks read the HTTP field independently of the response MIME property.
    headers.append("Content-Type", mime);
    headers.append("Content-Length", &length.to_string());
    headers.append("Content-Security-Policy", source.csp());
    headers.append("Cache-Control", "no-store");
    headers.append("X-Content-Type-Options", "nosniff");
    response.set_http_headers(headers);
}
fn finish(request: &URISchemeRequest, source: &EntryResponseSource, body: Arc<NativeBodyResponse>) {
    if !source.is_current() {
        fail(request, "retired remote document");
        return;
    }
    let length = body.bytes().len() as i64;
    let mime = body.mime().to_owned();
    let bytes = glib::Bytes::from_owned(BodyBytes(body));
    let stream = gio::MemoryInputStream::from_bytes(&bytes);
    let response = URISchemeResponse::new(&stream, length);
    finish_headers(&response, source, &mime, length);
    request.finish_with_response(&response);
}
pub(in crate::host_api::webview) fn webview(source: EntryResponseSource) -> WebView {
    let context = WebContext::new_ephemeral();
    crate::host_api::webview::linux::selection::install(&context);
    if source.resources_enabled()
        && let Some(security) = context.security_manager()
    {
        security.register_uri_scheme_as_secure(ENTRY_SCHEME);
        security.register_uri_scheme_as_cors_enabled(ENTRY_SCHEME);
    }
    let pending = pending_reads::PendingReads::default();
    let callbacks = pending.clone();
    let provider = source.clone();
    context.register_uri_scheme(ENTRY_SCHEME, move |request| {
        let Some(uri) = request
            .uri()
            .filter(|_| request.http_method().as_deref() == Some("GET"))
        else {
            fail(request, "unsupported remote file request");
            return;
        };
        if let Some(body) = provider.response(uri.as_str()) {
            finish(request, &provider, body);
            return;
        }
        if !provider.allows_uri(uri.as_str()) {
            fail(request, "unsupported remote file route");
            return;
        }
        if !callbacks.available() {
            fail(request, "remote file response capacity");
            return;
        }
        let Some(wait) = provider.request(uri.as_str()) else {
            fail(request, "retired remote document");
            return;
        };
        // Finishing the response installs WebKit's exact per-request GIO cancellable. No bytes
        // leave this deferred stream until the complete body and final metadata are available.
        let Some(stream) = deferred_stream::DeferredStream::new(provider.clone(), wait, &callbacks)
        else {
            fail(request, "remote file stream capacity");
            return;
        };
        let response = URISchemeResponse::new(&stream, -1);
        stream.set_response(&response);
        request.finish_with_response(&response);
    });
    let view = WebView::with_context(&context);
    let main = source.clone();
    view.connect_resource_load_started(move |view, resource, request| {
        if view.main_resource().as_ref() == Some(resource)
            && request
                .uri()
                .is_some_and(|uri| main.main_resource(uri.as_str()))
        {
            view.stop_loading();
        }
    });
    view.connect_destroy(move |_| {
        source.lost_native_view();
        pending.close();
    });
    if let Some(settings) = WebViewExt::settings(&view) {
        use webkit2gtk::SettingsExt;
        settings.set_enable_dns_prefetching(false);
        settings.set_enable_hyperlink_auditing(false);
        settings.set_enable_webrtc(false);
        settings.set_enable_media_stream(false);
        settings.set_allow_file_access_from_file_urls(false);
        settings.set_allow_universal_access_from_file_urls(false);
    }
    view
}
