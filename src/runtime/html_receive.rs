//! View-only remote entry snapshots and one-shot decisions bound to their original resource.
use crate::{
    core::origin::IntentOrigin,
    plugin_bridge::remote_surface::RemoteSurface,
    runtime::{engine_access::EngineMut, surface_binding::SurfaceBinding},
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use tasty_ipc::html_transfer::{HtmlSource, TransferId};
use tasty_remote::{html_document::ReceiveState, html_mirror::HtmlMirror};

#[derive(Clone, Copy, Debug)]
pub(crate) enum ReceiveActionKind {
    Allow,
    Decline,
    Continue,
    Stop,
}
#[derive(Clone)]
pub(crate) struct ReceiveAction {
    target: SurfaceBinding,
    owner: Weak<Mutex<HtmlMirror>>,
    id: TransferId,
    kind: ReceiveActionKind,
    consumed: Arc<AtomicBool>,
}
impl std::fmt::Debug for ReceiveAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReceiveAction")
            .field("target", &self.target)
            .field("id", &self.id)
            .field("kind", &self.kind)
            .finish_non_exhaustive()
    }
}
pub(crate) struct ReceiveSnapshot {
    pub address: String,
    pub connected: bool,
    pub received: u64,
    pub skipped: usize,
    pub size_notice: Option<u64>,
    pub waiting: usize,
    pub paused: bool,
    pub receiving: bool,
    pub ready: bool,
    pub failed: bool,
    request: Option<ReceiveAction>,
}
impl ReceiveSnapshot {
    pub(crate) fn action(&self, kind: ReceiveActionKind) -> Option<ReceiveAction> {
        let mut request = self.request.clone()?;
        request.kind = kind;
        request.consumed = Arc::new(AtomicBool::new(false));
        Some(request)
    }
}
pub(crate) fn snapshot(remote: &RemoteSurface, target: SurfaceBinding) -> Option<ReceiveSnapshot> {
    let mirror = remote.lock_html_mirror()?;
    let metadata = mirror.metadata.as_ref();
    let address = match metadata.map(|m| &m.source) {
        Some(HtmlSource::File { path, .. }) => path.clone(),
        Some(HtmlSource::Http { url }) => url.clone(),
        _ => String::new(),
    };
    let document = mirror.document.as_ref();
    let state = mirror.state();
    let ready = mirror.body_store().current().is_some();
    Some(ReceiveSnapshot {
        address,
        connected: mirror.connected && mirror.connection.is_active(),
        received: document.map_or(0, |d| d.received),
        skipped: document.map_or(0, |d| d.skipped),
        size_notice: (state == Some(ReceiveState::SizeNotice))
            .then(|| metadata.map(|m| m.size))
            .flatten(),
        waiting: document.map_or(0, |d| d.notices.len().saturating_sub(1)),
        paused: document.is_some_and(|d| d.paused),
        receiving: document.is_none_or(|d| d.receiving())
            || (state == Some(ReceiveState::Complete) && !ready && mirror.failure.is_none()),
        ready,
        failed: mirror.failure.is_some()
            || matches!(state, Some(ReceiveState::Failed | ReceiveState::Cancelled)),
        request: mirror.id.map(|id| ReceiveAction {
            target,
            owner: Arc::downgrade(
                remote
                    .html_mirror
                    .as_ref()
                    .expect("locked mirror has owner"),
            ),
            id,
            kind: ReceiveActionKind::Decline,
            consumed: Arc::new(AtomicBool::new(false)),
        }),
    })
}
impl ReceiveAction {
    pub(crate) fn apply(&self, engine: &mut EngineMut<'_>, origin: &IntentOrigin) {
        if !origin.is_user() {
            tracing::warn!(?origin, "remote HTML receive decision requires user origin");
            return;
        }
        if self.consumed.swap(true, Ordering::AcqRel) || !self.target.current(&engine.as_ref()) {
            return;
        }
        let Some(remote) = engine
            .runtime
            .surfaces
            .get(&self.target.surface_id())
            .and_then(|surface| surface.as_any().downcast_ref::<RemoteSurface>())
        else {
            return;
        };
        if remote
            .html_mirror
            .as_ref()
            .is_none_or(|owner| !Arc::downgrade(owner).ptr_eq(&self.owner))
        {
            return;
        }
        let Some(mut mirror) = remote.lock_html_mirror() else {
            return;
        };
        match self.kind {
            ReceiveActionKind::Allow => {
                mirror.allow(self.id);
            }
            ReceiveActionKind::Decline => mirror.queue_decline(self.id),
            ReceiveActionKind::Continue => {
                mirror.continue_receiving(self.id);
            }
            ReceiveActionKind::Stop => mirror.queue_stop(self.id),
        }
    }
}
