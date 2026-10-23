//! One ordered family for file and encoded HTTP control commands.
use super::*;
use crate::{html_http::HttpRequest, html_transfer::HtmlRequest};

// No Debug: HTTP commands may carry private URLs, headers and upload bytes.
pub enum HtmlControl {
    File(HtmlRequest),
    Http(HttpRequest),
}
impl StreamHub {
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
