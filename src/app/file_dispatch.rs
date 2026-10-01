//! Complete file identification in the explicit origin's owning engine.

use crate::app::App;
use crate::app::window_access::engines_mut;
use crate::core::request_target::{Kind, ResourceId, unowned_target_message};
use crate::file::format::{DetectorId, FileTarget};
use crate::identify_worker::IdentifyRequestId;
use crate::view::ui::View;

impl App {
    pub(super) fn handle_identify_done(
        &mut self,
        request_id: IdentifyRequestId,
        target: FileTarget,
        detector: Option<DetectorId>,
        origin_surface_id: Option<u32>,
        dispatch_origin: crate::file::dispatch::FileDispatchOrigin,
        ignore_size_limit: bool,
    ) {
        tracing::debug!(%request_id, target = %target.display(), ?detector,
            ?origin_surface_id, ignore_size_limit, "IdentifyDone");
        let named = origin_surface_id.map(|id| ResourceId {
            kind: Kind::Surface,
            id: u64::from(id),
        });
        let window = match named {
            Some(rid) => self.find_main_with_resource(rid),
            None => self.view.focused_view_id,
        };
        if let Some((main, mut engine)) = window.and_then(|id| engines_mut!(self).window_pair(id)) {
            crate::file::dispatch::apply_identify_result(
                &mut self.services,
                &mut main.state,
                &mut engine,
                target,
                detector,
                origin_surface_id,
                dispatch_origin,
                ignore_size_limit,
            );
            main.mark_dirty();
            return;
        }
        if let Some(rid) = named {
            if let Some((state, mut engine)) = engines_mut!(self).parked_session_with_resource(rid)
            {
                crate::file::dispatch::apply_identify_result(
                    &mut self.services,
                    state,
                    &mut engine,
                    target,
                    detector,
                    origin_surface_id,
                    dispatch_origin,
                    ignore_size_limit,
                );
            } else {
                // The RPC already acknowledged enqueueing. Its existing async
                // error channel is the log; never acknowledge a second response.
                tracing::warn!(%request_id, "{}", unowned_target_message(rid, "file_handler.dispatch"));
            }
        }
    }
}
