//! Materialize per-binding state and drive metadata/credit/scan effects on the owning connection.
use super::lock_mirror;
use crate::{plugin_bridge::remote_surface::RemoteSurface, runtime::engine_access::EngineMut};
use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use tasty_ipc::{
    html_transfer::{CAPABILITY, HtmlRequest, VIEWER_CAPABILITY},
    stream::StreamTag,
};
use tasty_remote::{
    client_session::{AttachClientSession, OutFrame},
    html_mirror::HtmlMirror,
};
static NEXT_BINDING: AtomicU64 = AtomicU64::new(1);
fn send(sess: &AttachClientSession, request: HtmlRequest) -> bool {
    let payload = match serde_json::to_vec(&request) {
        Ok(payload) => payload,
        Err(error) => {
            tracing::error!(%error, "HTML request serialization failed");
            return false;
        }
    };
    if sess
        .transport
        .frame_tx
        .send(OutFrame {
            tag: StreamTag::Control,
            payload,
        })
        .is_err()
    {
        tracing::debug!("HTML request belongs to a retired connection");
        return false;
    }
    true
}
pub(in crate::app::attach_client) fn synchronize(
    sess: &mut AttachClientSession,
    engine: &mut EngineMut<'_>,
) -> bool {
    let live = !sess.transport.disconnected.load(Ordering::Acquire)
        && sess.transport.frame_tx.epoch().is_active();
    let available = live
        && sess.transport.html.server_entry
        && sess.transport.html.server_transfer
        && super::viewer_available();
    if available {
        engine
            .remote
            .html_entry_hosts
            .insert(sess.state.local_workspace);
    } else {
        engine
            .remote
            .html_entry_hosts
            .remove(&sess.state.local_workspace);
    }
    if available && !sess.transport.html.viewer_declared {
        if !send(
            sess,
            HtmlRequest::HtmlCapabilities {
                capabilities: if sess.transport.html.server_resources {
                    vec![
                        CAPABILITY.into(),
                        VIEWER_CAPABILITY.into(),
                        tasty_ipc::html_transfer::RESOURCE_CAPABILITY.into(),
                    ]
                } else {
                    vec![CAPABILITY.into(), VIEWER_CAPABILITY.into()]
                },
            },
        ) {
            return false;
        }
        sess.transport.html.viewer_declared = true;
    }
    let mut changed = bind_mirrors(sess, engine, live, available);
    let mirrors: Vec<_> = sess
        .transport
        .html
        .mirrors
        .iter()
        .map(|(id, mirror)| (*id, Arc::clone(mirror)))
        .collect();
    for (remote, shared) in mirrors {
        let mut mirror = lock_mirror(&shared);
        if !live || !mirror.connection.same(&sess.transport.frame_tx.epoch()) {
            if mirror.connected {
                mirror.disconnect();
                changed = true;
            }
            continue;
        }
        mirror.enable_resources(sess.transport.html.server_resources && super::viewer_available());
        changed |= mirror.poll_navigation();
        changed |= mirror.poll_resources();
        for request in mirror.take_pending() {
            if !send(sess, request) {
                mirror.disconnect();
                return true;
            }
        }
        if mirror.needs_request() {
            let Some(id) = sess
                .transport
                .html
                .entry_id(remote, mirror.binding, mirror.generation)
            else {
                mirror.disconnect();
                continue;
            };
            for request in mirror.begin(id) {
                if !send(sess, request) {
                    mirror.disconnect();
                    return true;
                }
            }
            changed = true;
        }
        for fetch in mirror.take_resource_fetches() {
            let Some(id) = sess
                .transport
                .html
                .entry_id(remote, mirror.binding, mirror.generation)
            else {
                mirror.disconnect();
                return true;
            };
            if let Some(request) = mirror.bind_resource(fetch, id)
                && !send(sess, request)
            {
                mirror.disconnect();
                return true;
            }
        }
        if let Some(metadata) = &mirror.metadata
            && let Some(&local) = sess.state.remote_to_local.get(&remote)
        {
            engine.set_mirror_surface_cwd(local, metadata.cwd.clone());
        }
        if let Some(credit) = mirror.credit()
            && !send(sess, credit)
        {
            mirror.disconnect();
            return true;
        }
        changed |= mirror.poll_scan(Arc::clone(&engine.runtime.waker));
        #[cfg(feature = "gui")]
        if (mirror.failure.is_some()
            || matches!(
                mirror.state(),
                Some(
                    tasty_remote::html_document::ReceiveState::Skipped
                        | tasty_remote::html_document::ReceiveState::Failed
                        | tasty_remote::html_document::ReceiveState::Cancelled
                )
            ))
            && let Some(surface) = sess
                .state
                .remote_to_local
                .get(&remote)
                .and_then(|local| engine.runtime.surfaces.get(local))
                .and_then(|surface| surface.as_any().downcast_ref::<RemoteSurface>())
        {
            surface.with_html_script(|state| state.on_load_failed());
        }
    }
    changed
}

fn bind_mirrors(
    sess: &mut AttachClientSession,
    engine: &mut EngineMut<'_>,
    live: bool,
    available: bool,
) -> bool {
    let mut changed = false;
    let mut current = std::collections::HashSet::new();
    for (&remote, &local) in &sess.state.remote_to_local {
        let Some(surface) = engine
            .runtime
            .surfaces
            .get_mut(&local)
            .and_then(|surface| surface.as_any_mut().downcast_mut::<RemoteSurface>())
        else {
            continue;
        };
        if surface.kind_static != "html" || surface.plugin_id != "com.tasty.html" {
            continue;
        }
        if live
            && available
            && surface.html_mirror.as_ref().is_none_or(|mirror| {
                !lock_mirror(mirror)
                    .connection
                    .same(&sess.transport.frame_tx.epoch())
            })
        {
            let Ok(binding) = NEXT_BINDING
                .fetch_update(Ordering::AcqRel, Ordering::Acquire, |v| v.checked_add(1))
            else {
                tracing::error!("HTML binding identity exhausted");
                continue;
            };
            surface.html_mirror = Some(Arc::new(std::sync::Mutex::new(HtmlMirror::new(
                remote,
                binding,
                sess.transport.frame_tx.epoch(),
            ))));
            #[cfg(feature = "gui")]
            surface.with_html_script(|state| *state = Default::default());
            changed = true;
        }
        if let Some(mirror) = &surface.html_mirror {
            current.insert(remote);
            sess.transport
                .html
                .mirrors
                .insert(remote, Arc::clone(mirror));
        }
    }
    sess.transport.html.mirrors.retain(|remote, mirror| {
        let keep = current.contains(remote);
        if !keep {
            lock_mirror(mirror).disconnect();
        }
        keep
    });
    changed
}
