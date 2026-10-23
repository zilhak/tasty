//! HTML transport wiring. Native viewer installation is intentionally a separate capability.
use tasty_ipc::{
    html_transfer::HtmlReply,
    stream_hub::{PumpOutcome, StreamHub},
};
use tasty_remote::client_session::{AttachClientSession, MirrorEvent};
impl crate::app::App {
    pub(in crate::app) fn apply_html_batch(&mut self, outcome: &mut PumpOutcome, hub: &StreamHub) {
        for (client, msg) in outcome.html_requests.drain(..) {
            if let Some(mut engine) = crate::app::event_handler::find_workspace_holder_engine_mut(
                self.engines_mut(),
                client,
            ) {
                engine.handle_html_request(hub, client, msg);
            } else {
                crate::remote::html_transfer::refuse(hub, client, msg);
            }
        }
    }
}
pub(super) fn parse(payload: &[u8]) -> Option<MirrorEvent> {
    serde_json::from_slice::<HtmlReply>(payload)
        .ok()
        .map(MirrorEvent::Html)
}
pub(super) fn receive(sess: &mut AttachClientSession, reply: HtmlReply) {
    let remote = reply.id().surface_id;
    if sess.state.remote_to_local.contains_key(&remote)
        && let Some(document) = sess.transport.html.documents.get_mut(&remote)
    {
        document.receive(reply);
    }
}
pub(super) fn retain(sess: &mut AttachClientSession, surfaces: &[serde_json::Value]) {
    sess.transport.html.documents.retain(|remote, _| {
        surfaces.iter().any(|surface| {
            surface["remote_id"].as_u64() == Some(*remote as u64) && surface["role"] == "html"
        })
    });
}
