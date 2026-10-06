//! Application values identify the sole target; Remote retains pending connection resources.

use super::projection::mirror_id_needs;
use super::wire::mirror_event_from_control;
use crate::AppEvent;
use crate::app::App;
use crate::runtime::engine_session::EngineId;
use std::sync::Arc;
use tasty_remote::pending_connection::{ConnectionTarget, ConnectionTicket};
use winit::event_loop::EventLoopProxy;

#[derive(Clone)]
pub(crate) struct PendingMirrorInstall {
    pub engine: EngineId,
    pub window: Option<winit::window::WindowId>,
    pub view: Option<std::sync::Weak<()>>,
    pub selection: Option<std::sync::Weak<()>>,
    pub activate: bool,
    pub anchor: Option<u32>,
    pub mapping: Option<crate::model::WorkspaceAttachMapping>,
    pub mapping_token: Option<std::sync::Weak<()>>,
    pub reconnect: Option<(u32, tasty_remote::connection::ConnectionEpoch)>,
    pub resync: bool,
}
impl App {
    pub(crate) fn mirror_install_target(
        &self,
        explicit: Option<EngineId>,
        anchor: Option<u32>,
        reconnect: Option<usize>,
        activate: bool,
    ) -> anyhow::Result<PendingMirrorInstall> {
        let anchor = anchor.or_else(|| {
            reconnect
                .and_then(|index| self.remote.sessions.get(index))
                .and_then(|session| session.state.anchor_ws_id)
        });
        let resync = reconnect
            .and_then(|index| self.remote.sessions.get(index))
            .is_some_and(|session| session.state.resync_pending.is_some());
        let old = reconnect
            .and_then(|index| self.remote.sessions.get(index))
            .map(|session| {
                (
                    session.state.local_workspace,
                    session.transport.frame_tx.epoch(),
                )
            });
        let engine = explicit
            .or_else(|| {
                old.as_ref().and_then(|(workspace, _)| {
                    self.engines
                        .all_sessions()
                        .find(|engine| engine.core_state.has_workspace(*workspace))
                        .map(|engine| engine.id)
                })
            })
            .or_else(|| {
                anchor.and_then(|workspace| {
                    self.engines
                        .all_sessions()
                        .find(|engine| engine.core_state.has_workspace(workspace))
                        .map(|engine| engine.id)
                })
            })
            .or_else(|| {
                self.focused_window()
                    .and_then(|main| self.engines.of_window(main.base.winit.id()))
            })
            .ok_or_else(|| anyhow::anyhow!("no engine is available to receive a mirror"))?;
        let window = self.engines.window_of(engine);
        let main = window
            .and_then(|window| self.view.views.get(&window))
            .and_then(|view| view.as_main());
        if old.is_none() && main.is_none() {
            anyhow::bail!("mirror creation has no originating View");
        }
        let owner = self
            .engines
            .get(engine)
            .ok_or_else(|| anyhow::anyhow!("mirror target engine missing"))?;
        let mapping = anchor.and_then(|id| {
            owner
                .find_workspace_index_for_id(id)
                .and_then(|index| owner.workspace_at(index))
                .and_then(|workspace| workspace.attach_mapping.clone())
        });
        let mapping_token = anchor
            .and_then(|id| owner.remote.attach_mapping_tokens.get(&id))
            .map(Arc::downgrade);
        Ok(PendingMirrorInstall {
            resync,
            mapping,
            mapping_token,
            engine,
            window,
            view: main.map(|main| main.base.state.identity()),
            selection: main.map(|main| main.state.navigation.generation()),
            activate,
            anchor,
            reconnect: old,
        })
    }
    pub(crate) fn mirror_install_target_is_current(&self, target: &PendingMirrorInstall) -> bool {
        let Some(engine) = self.engines.get(target.engine) else {
            return false;
        };
        if let Some(anchor) = target.anchor {
            let current = engine
                .find_workspace_index_for_id(anchor)
                .and_then(|index| engine.workspace_at(index));
            if current.is_none_or(|workspace| workspace.attach_mapping != target.mapping)
                || target.mapping_token.as_ref().is_none_or(|token| {
                    engine
                        .remote
                        .attach_mapping_tokens
                        .get(&anchor)
                        .is_none_or(|current| !token.ptr_eq(&Arc::downgrade(current)))
                })
            {
                return false;
            }
        }
        if let Some((workspace, epoch)) = &target.reconnect {
            return engine.has_workspace(*workspace)
                && self.remote.sessions.iter().any(|session| {
                    session.state.local_workspace == *workspace
                        && session.transport.frame_tx.epoch().same(epoch)
                });
        }
        target.window.is_some_and(|window| {
            self.engines.of_window(window) == Some(target.engine)
                && self
                    .view
                    .views
                    .get(&window)
                    .and_then(|view| view.as_main())
                    .is_some_and(|main| {
                        target
                            .view
                            .as_ref()
                            .is_some_and(|identity| main.base.state.matches_identity(identity))
                    })
        })
    }
    pub(crate) fn queue_mirror_connection(
        &mut self,
        target: PendingMirrorInstall,
        port: u16,
        workspace: u32,
        tunnel: Option<tasty_ssh::SshTunnel>,
    ) -> anyhow::Result<()> {
        if !self.mirror_install_target_is_current(&target) {
            self.remote.retire_tunnel(tunnel);
            anyhow::bail!("mirror origin was retired before connection");
        }
        self.state
            .mirror_attempts
            .supersede_connections(&target, &mut self.remote);
        let ticket = self
            .remote
            .queue_connection(
                ConnectionTarget {
                    port,
                    workspace,
                    anchor: target.anchor,
                    mapping: target.mapping.clone(),
                },
                tunnel,
                attach_wake(&self.view.proxy),
                mirror_event_from_control,
            )
            .map_err(anyhow::Error::msg)?;
        self.state
            .mirror_attempts
            .register_connection(ticket, target);
        Ok(())
    }
    pub(crate) fn poll_pending_mirror_installs(&mut self) {
        self.remote.collect_connections();
        let pending = self.state.mirror_attempts.connection_snapshot();
        for (ticket, target) in pending {
            if !self.mirror_install_target_is_current(&target) {
                self.state
                    .mirror_attempts
                    .cancel_connection(ticket, &mut self.remote);
                continue;
            }
            if let Some(error) = self.remote.connection_error(ticket) {
                self.fail_pending_mirror(ticket, &target, error);
                continue;
            }
            if target.reconnect.is_some() && self.engines.window_of(target.engine).is_none() {
                continue;
            }
            let Some(prepared) = self.remote.prepared_connection(ticket) else {
                continue;
            };
            let Some(engine) = self.engines.get(target.engine) else {
                continue;
            };
            let admission = mirror_id_needs(
                &prepared.tree,
                prepared.surfaces.len(),
                target.reconnect.is_none(),
            )
            .and_then(|needed| {
                engine
                    .runtime
                    .ids
                    .ensure(&needed)
                    .map_err(anyhow::Error::new)
            });
            if let Err(error) = admission {
                if matches!(
                    error.downcast_ref::<crate::runtime::id_reservations::ReservationError>(),
                    Some(crate::runtime::id_reservations::ReservationError::Pending)
                ) {
                    (engine.runtime.waker)();
                    continue;
                }
                self.fail_pending_mirror(ticket, &target, error.to_string());
                continue;
            }
            let Some(prepared) = self.remote.take_connection(ticket) else {
                continue;
            };
            self.state.mirror_attempts.finish_connection(ticket);
            let installed = if let Some((workspace, epoch)) = &target.reconnect {
                let index = self.remote.sessions.iter().position(|session| {
                    session.state.local_workspace == *workspace
                        && session.transport.frame_tx.epoch().same(epoch)
                });
                index
                    .ok_or_else(|| anyhow::anyhow!("reconnect target retired"))
                    .and_then(|index| self.install_reconnected_mirror(index, &target, prepared))
                    .map(|_| *workspace)
            } else {
                self.install_new_mirror(&target, prepared)
            };
            match installed {
                Ok(workspace) => {
                    if target.activate
                        && let Some(window) = target.window
                        && let Some((main, engine)) = self.engines().window_pair(window)
                        && target
                            .view
                            .as_ref()
                            .is_some_and(|view| main.base.state.matches_identity(view))
                        && target.selection.as_ref().is_some_and(|selection| {
                            main.state.navigation.matches_generation(selection)
                        })
                        && engine.has_workspace(workspace)
                    {
                        self.focus_mirror_workspace(workspace);
                    }
                    if let Some(anchor) = target.anchor {
                        crate::app::auto_attach::forget_anchor_backoff(&mut self.remote, anchor);
                    }
                }
                Err(error) => self.fail_pending_mirror(ticket, &target, error.to_string()),
            }
        }
    }
    fn fail_pending_mirror(
        &mut self,
        ticket: ConnectionTicket,
        target: &PendingMirrorInstall,
        error: String,
    ) {
        self.state
            .mirror_attempts
            .cancel_connection(ticket, &mut self.remote);
        tracing::warn!(engine=?target.engine,"mirror connection could not be installed: {error}");
        if target.resync
            && let Some((workspace, epoch)) = &target.reconnect
            && let Some(index) = self.remote.sessions.iter().position(|session| {
                session.state.local_workspace == *workspace
                    && session.transport.frame_tx.epoch().same(epoch)
            })
        {
            self.remote.sessions[index].state.resync_pending = None;
            if target.anchor.is_some() {
                self.enter_reconnecting(index);
            } else {
                let session = self.remote.sessions.remove(index);
                self.cleanup_mirror_workspace(&session, true);
            }
        }
        if let Some(anchor) = target.anchor {
            self.remote.active.remove(&anchor);
            if target.reconnect.is_some() {
                self.on_reconnect_attempt_failed(anchor, &anyhow::anyhow!(error));
            } else {
                back_off_failed_first_attach(
                    &mut self.remote,
                    target.anchor,
                    target.reconnect.is_some(),
                    target.mapping.as_ref(),
                );
            }
        }
    }
}

fn attach_wake(proxy: &EventLoopProxy<AppEvent>) -> Arc<dyn Fn() + Send + Sync> {
    let proxy = proxy.clone();
    Arc::new(move || {
        if let Err(error) = proxy.send_event(AppEvent::AttachClientData) {
            tracing::debug!("remote wake after event loop closed: {error}");
        }
    })
}

/// 접수 뒤 연결 단계에서 실패한 첫 자동 attach도 해석 실패와 같은 간격으로 미룬다.
/// 재연결(`reconnecting`)은 재연결 백오프가 맡으므로 기록하지 않는다.
fn back_off_failed_first_attach(
    remote: &mut tasty_remote::outbound::Remote,
    anchor: Option<u32>,
    reconnecting: bool,
    mapping: Option<&crate::model::WorkspaceAttachMapping>,
) {
    if let (Some(anchor), false) = (anchor, reconnecting) {
        crate::app::auto_attach::back_off_first_attach(remote, anchor, mapping);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mapping() -> crate::model::WorkspaceAttachMapping {
        crate::model::WorkspaceAttachMapping {
            target: crate::model::WorkspaceAttachTarget::Profile { name: "p".into() },
            remote_workspace: Some(1),
        }
    }

    #[test]
    fn a_first_attach_that_fails_after_queueing_backs_off() {
        let mut remote = tasty_remote::outbound::Remote::new();
        back_off_failed_first_attach(&mut remote, Some(10), false, Some(&mapping()));
        let retry = remote.attach_retry.get(&10).expect("retry recorded");
        assert_eq!(retry.mapping, mapping());
        assert!(retry.holds(&mapping(), std::time::Instant::now()));
    }

    #[test]
    fn a_reconnect_or_an_unanchored_failure_records_no_retry() {
        let mut remote = tasty_remote::outbound::Remote::new();
        back_off_failed_first_attach(&mut remote, Some(10), true, Some(&mapping()));
        back_off_failed_first_attach(&mut remote, None, false, Some(&mapping()));
        assert!(remote.attach_retry.is_empty());
    }
}
