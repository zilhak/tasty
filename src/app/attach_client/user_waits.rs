//! User request admission and observable waits. View-only; never journals a preview.
use super::forward::find_mirror_session_and_remote_id;
use crate::app::App;
use crate::app::attach_client::GIT_VIEWER_PLUGIN_ID;
use crate::ipc::stream::StructuralOp;
use crate::runtime::engine_session::EngineId;
use crate::state::attach_pending::waits::{Wait, WaitKind};
use crate::state::attach_pending::{Query, QueryConsumer};
use std::collections::HashMap;

impl App {
    pub(super) fn begin_query_wait(
        &mut self,
        surface: u32,
        request: u64,
        admitted: std::time::Instant,
        consumer: QueryConsumer,
    ) {
        let Some((session, _)) =
            find_mirror_session_and_remote_id(&mut self.remote.sessions, surface, "query wait")
        else {
            return;
        };
        let workspace = session.state.local_workspace;
        let epoch = session.transport.frame_tx.epoch();
        for (_, main, engine) in self.engines_mut().window_pairs() {
            let Some(target) =
                crate::runtime::surface_binding::SurfaceBinding::capture(&engine.read(), surface)
            else {
                continue;
            };
            let queries = &mut main.state.attach_pending.queries;
            queries.retain(|query| query.consumer != consumer);
            queries.push(Query {
                consumer,
                request,
                workspace,
                admitted,
                epoch: epoch.clone(),
                target,
            });
            break;
        }
    }
    pub(crate) fn sync_attach_waits(&mut self) {
        use crate::view::ui::View as _;
        let (waits, epochs): (HashMap<_, _>, HashMap<_, _>) = self
            .remote
            .sessions
            .iter()
            .filter(|session| session.transport.frame_tx.epoch().is_active())
            .map(|session| {
                let mut times: Vec<_> = session
                    .state
                    .resize_sync
                    .pending_times()
                    .map(|time| Wait::new(WaitKind::Resize, time))
                    .collect();
                times.extend(
                    session
                        .transport
                        .frame_tx
                        .oldest_unprocessed_input()
                        .map(|time| Wait::new(WaitKind::Input, time)),
                );
                let workspace = session.state.local_workspace;
                (
                    (workspace, times),
                    (workspace, session.transport.frame_tx.began_at()),
                )
            })
            .unzip();
        let git_consumers: std::collections::HashSet<_> = self
            .plugin_manager
            .as_ref()
            .into_iter()
            .flat_map(|manager| manager.popup_instances())
            .filter(|(_, instance)| instance.plugin_id == GIT_VIEWER_PLUGIN_ID)
            .map(|(id, _)| id)
            .collect();
        for (_, main, engine) in self.engines_mut().window_pairs() {
            let mut waits = waits.clone();
            main.state.attach_pending.queries.retain(|query| {
                query.epoch.is_active()
                    && query.target.same_surface_in(&engine.read())
                    && match query.consumer {
                        QueryConsumer::Git(id) => git_consumers.contains(&id),
                        QueryConsumer::Markdown(_) => true,
                    }
            });
            for query in &main.state.attach_pending.queries {
                waits.entry(query.workspace).or_default().push(Wait::new(
                    match query.consumer {
                        QueryConsumer::Git(_) => WaitKind::Git,
                        QueryConsumer::Markdown(_) => WaitKind::Markdown,
                    },
                    query.admitted,
                ));
            }
            for (surface, since) in main.state.explorer_views.pending_remote_times() {
                if let Some((index, _)) = engine.find_workspace_index_for_surface(surface)
                    && let Some(workspace) = engine.workspace_at(index)
                    && waits.contains_key(&workspace.id)
                {
                    waits
                        .entry(workspace.id)
                        .or_default()
                        .push(Wait::new(WaitKind::Folder, since));
                }
            }
            if let Some(picker) = &main.state.dialogs.file_picker
                && let Some(workspace) = picker.mirror_ws_id
                && waits.contains_key(&workspace)
                && let crate::state::FpLoadState::Loading { sent_at, .. } = picker.load
            {
                waits
                    .entry(workspace)
                    .or_default()
                    .push(Wait::new(WaitKind::Folder, sent_at));
            }
            for (workspace, times) in &mut waits {
                if let Some(began) = epochs.get(workspace) {
                    times.retain(|wait| wait.admitted >= *began);
                }
                times.sort_by_key(|wait| wait.admitted);
            }
            main.state.attach_pending.retain(engine.core);
            if main.state.attach_waits != waits {
                main.state.attach_waits.clone_from(&waits);
                main.mark_dirty();
            }
        }
    }
    pub(crate) fn begin_attach_request(
        &mut self,
        engine: EngineId,
        op: &StructuralOp,
        origin: &crate::intent::IntentOrigin,
        admitted: std::time::Instant,
    ) -> Option<std::sync::Arc<tasty_remote::pending_request::PendingRequest>> {
        if !origin.is_user() {
            return None;
        }
        let (session, _) = find_mirror_session_and_remote_id(
            &mut self.remote.sessions,
            op.anchor_surface_id(),
            "user admission",
        )?;
        let epoch = session.transport.frame_tx.epoch();
        let context = self.engines_mut().resolve(engine)?;
        let view = context.view?;
        let receipt = context.state.attach_pending.begin(
            context.engine.core,
            &context.state.navigation,
            op,
            epoch,
            view.state.identity(),
            admitted,
        );
        view.mark_dirty();
        receipt
    }
}
