//! One ephemeral WebKit context with a completed-body-only scheme. No local/network fallback.
use super::{ENTRY_CSP, EntryResponseSource};
use gtk::{gio, glib};
use std::sync::Arc;
use tasty_remote::html_body::{ENTRY_SCHEME, NativeBodyResponse};
use webkit2gtk::{
    URISchemeRequestExt, URISchemeResponse, URISchemeResponseExt, WebContext, WebContextExt,
    WebView,
};

struct BodyBytes(Arc<NativeBodyResponse>);
impl AsRef<[u8]> for BodyBytes {
    fn as_ref(&self) -> &[u8] {
        self.0.bytes()
    }
}

pub(in crate::host_api::webview) fn webview(source: EntryResponseSource) -> WebView {
    let context = WebContext::new_ephemeral();
    context.register_uri_scheme(ENTRY_SCHEME, move |request| {
        let body = request.uri().and_then(|uri| source.response(uri.as_str()));
        let Some(body) = body.filter(|_| request.http_method().as_deref() == Some("GET")) else {
            request.finish_error(&mut glib::Error::new(
                gio::IOErrorEnum::PermissionDenied,
                "unsupported remote entry request",
            ));
            return;
        };
        let length = body.bytes().len() as i64;
        let mime = body.mime().to_owned();
        let bytes = glib::Bytes::from_owned(BodyBytes(body));
        let stream = gio::MemoryInputStream::from_bytes(&bytes);
        let response = URISchemeResponse::new(&stream, length);
        response.set_content_type(&mime);
        response.set_status(200, Some("OK"));
        let headers = soup::MessageHeaders::new(soup::MessageHeadersType::Response);
        headers.append("Content-Security-Policy", ENTRY_CSP);
        headers.append("Cache-Control", "no-store");
        headers.append("X-Content-Type-Options", "nosniff");
        response.set_http_headers(headers);
        request.finish_with_response(&response);
    });
    let view = WebView::with_context(&context);
    if let Some(settings) = webkit2gtk::WebViewExt::settings(&view) {
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
