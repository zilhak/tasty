//! Remote document views close the engine features whose traffic does not follow the view's
//! proxy. WebRTC sends ICE traffic over UDP from the client, and DNS prefetch resolves link
//! hosts through the client resolver. Both stay off explicitly instead of relying on WebKitGTK
//! defaults or a distribution build. Contract: `docs/design/systems/webview.md`.
use webkit2gtk::{Settings, SettingsExt};

/// Media stream is closed with WebRTC because enabling WebRTC implies media stream.
pub(in crate::host_api::webview) fn close(settings: &Settings) {
    settings.set_enable_webrtc(false);
    settings.set_enable_media_stream(false);
    settings.set_enable_dns_prefetching(false);
}
