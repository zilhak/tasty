//! One ordered family for file and encoded HTTP control commands.
use super::*;
use crate::{html_http::HttpRequest, html_transfer::HtmlRequest};

// No Debug: HTTP commands may carry private URLs, headers and upload bytes.
pub enum HtmlControl {
    File(HtmlRequest),
    Http(HttpRequest),
}
impl StreamHub {
    /// The peer must declare both HTTP transport and its native HTML viewer; HTTPS is separate.
    pub fn html_http_enabled(&self, client: StreamClientId, secure: bool) -> bool {
        tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED)
            .get(&client)
            .is_some_and(|sink| {
                sink.html_transfer
                    && sink.html_viewer
                    && sink.html_http
                    && (!secure || sink.html_https)
            })
    }
    /// Claim only a new file request or HTTP Begin, after actual owner validation.
    /// Credits and lifecycle commands never allocate identities. Duplicate HTTP Begin
    /// classification belongs to the existing document before calling this allocator.
    pub fn admit_html_request(
        &self,
        client: StreamClientId,
        binding: &Weak<()>,
        id: crate::html_transfer::TransferId,
    ) -> bool {
        let mut sinks =
            tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED);
        let Some(sink) = sinks.get_mut(&client) else {
            return false;
        };
        if !binding.ptr_eq(&Arc::downgrade(&sink.binding))
            || sink
                .html_admission
                .is_some_and(|(request, transfer)| id.request <= request || id.transfer <= transfer)
        {
            return false;
        }
        sink.html_admission = Some((id.request, id.transfer));
        true
    }

    pub(super) fn collect_html_request(
        &self,
        client: StreamClientId,
        payload: &[u8],
        out: &mut PumpOutcome,
    ) -> bool {
        if let Ok(msg) = serde_json::from_slice::<HtmlRequest>(payload) {
            match msg {
                HtmlRequest::HtmlCapabilities { capabilities } => {
                    self.set_html_capabilities(client, &capabilities);
                    out.html_requests.push((
                        client,
                        HtmlControl::File(HtmlRequest::HtmlCapabilities { capabilities }),
                    ));
                }
                msg if self.html_transfer_enabled(client) => {
                    out.html_requests.push((client, HtmlControl::File(msg)));
                }
                _ => {}
            }
            return true;
        }
        let Ok(msg) = serde_json::from_slice::<HttpRequest>(payload) else {
            return false;
        };
        // This records only the peer's declaration. The host must additionally gate its
        // own live server/native implementation before admitting any HTTP operation.
        let enabled =
            tasty_utils::poison::recover_mutex(self.sinks.lock(), SINKS_WHAT, &SINKS_POISONED)
                .get(&client)
                .is_some_and(|s| s.html_transfer && s.html_http);
        if enabled && msg.validate().is_ok() {
            out.html_requests.push((client, HtmlControl::Http(msg)));
        }
        true
    }
}
#[cfg(test)]
mod tests;
