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
        policy: crate::file::dispatch::FileDispatchPolicy,
    ) {
        tracing::debug!(%request_id, target = %target.display(), ?detector,
            ?origin_surface_id, ?policy, "IdentifyDone");
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
                policy,
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
                    policy,
                );
            } else {
                // 접수 응답은 이미 보냈다. 결과는 로그와 요청의 run으로 알린다.
                let message = unowned_target_message(rid, "file_handler.dispatch");
                tracing::warn!(%request_id, "{message}");
                crate::file::dispatch::fail_run(policy.run, message);
            }
        } else {
            fail_without_focus(request_id, &target, policy.run);
        }
    }
}

/// origin 없는 요청은 포커스된 창에서 연다. 마지막 창이 닫혔거나 parked면 열 곳이 없다.
fn fail_without_focus(request_id: IdentifyRequestId, target: &FileTarget, run: Option<u64>) {
    let message = "no window was focused to open the file in";
    tracing::warn!(%request_id, target = %target.display(), "{message}");
    crate::file::dispatch::fail_run(run, message);
}
