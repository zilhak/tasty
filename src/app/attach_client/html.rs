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
    if let Some(mirror) = sess.transport.html.mirrors.get(&remote) {
        lock_mirror(mirror).receive(reply);
        return;
    }
    if sess.state.remote_to_local.contains_key(&remote)
        && let Some(document) = sess.transport.html.documents.get_mut(&remote)
    {
        document.receive(reply);
    }
}
pub(super) fn retain(sess: &mut AttachClientSession, surfaces: &[serde_json::Value]) {
    sess.transport.html.mirrors.retain(|remote, mirror| {
        let keep = surfaces.iter().any(|surface| {
            surface["remote_id"].as_u64() == Some(*remote as u64) && surface["role"] == "html"
        });
        if !keep {
            lock_mirror(mirror).disconnect();
        }
        keep
    });
    sess.transport.html.documents.retain(|remote, _| {
        surfaces.iter().any(|surface| {
            surface["remote_id"].as_u64() == Some(*remote as u64) && surface["role"] == "html"
        })
    });
}

pub(super) mod resources;
mod viewer;
pub(super) use viewer::synchronize;
static MIRROR_POISONED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
pub(crate) fn lock_mirror(
    mirror: &std::sync::Mutex<tasty_remote::html_mirror::HtmlMirror>,
) -> std::sync::MutexGuard<'_, tasty_remote::html_mirror::HtmlMirror> {
    crate::poison::recover_mutex(mirror.lock(), "remote HTML mirror", &MIRROR_POISONED)
}
pub(super) fn viewer_available() -> bool {
    cfg!(all(feature = "gui", target_os = "linux"))
}
