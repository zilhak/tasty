//! DomainIntent를 적용하고 CoreEvent에 따른 후속 처리를 실행한다.
//! 구조 변경의 공통 처리는 core::structural_cascade가 CascadeWindow 포트를 통해 수행한다.
//! 설정·테마·플러그인 등 App 자원이 필요한 처리는 여기에 둔다.
//! [계층 경계](../../docs/adr/0002-domain-execution-and-ports.md)를 따른다.

use tasty_settings::Settings;
use winit::window::WindowId;

use crate::app::App;
use crate::core::AttentionKind;
use crate::core::intent::CoreEvent;
use crate::core::structural_cascade::{
    PaneSplitCascade, SurfaceCloseCascade, cascade_pane_closed_full, cascade_pane_split,
    cascade_surface_closed, cascade_surface_created, cascade_surface_split,
    cascade_tab_closed_full, cascade_tab_created,
};
use crate::intent::{DispatchedIntent, Intent, IntentOrigin};
use crate::view::ui::View as _;

/// 요청이 시작된 창 또는 parked 상태. 도메인 변경과 후속 처리가 같은 engine을 사용한다.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DispatchSource {
    Main(WindowId),
    Parked(usize),
}

/// workspace 생성 결과. window_id는 호출한 쪽이 원래 engine에서 구한다.
pub(crate) struct WorkspaceCreatedCascade {
    pub(crate) workspace_id: u32,
    pub(crate) index: usize,
    pub(crate) surface_id: Option<u32>,
    pub(crate) renamed_name: Option<String>,
    pub(crate) renamed_subtitle: Option<String>,
    pub(crate) renamed_description: Option<String>,
}

impl App {
    /// Core가 반환한 이벤트를 같은 source·origin으로 처리한다.
    pub(crate) fn dispatch_domain_intent(
        &mut self,
        source: DispatchSource,
        dispatched: DispatchedIntent,
    ) -> anyhow::Result<()> {
        let Intent::Domain(intent) = dispatched.body else {
            anyhow::bail!("dispatch_domain_intent: non-Domain Intent");
        };
        let origin = dispatched.origin;
        let core = &mut self.core;
        let events = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    anyhow::bail!("dispatch_domain_intent: main window {wid:?} not found");
                };
                core.apply(&mut main.core_state, intent)?
            }
            DispatchSource::Parked(idx) => {
                let Some((_, engine)) = self.parked_states.get_mut(idx) else {
                    anyhow::bail!("dispatch_domain_intent: parked state {idx} not found");
                };
                core.apply(engine, intent)?
            }
        };
        for event in events {
            self.handle_core_event(source, &origin, event);
        }
        Ok(())
    }

    /// PTY 출력은 사용자·에이전트 요청이 아니므로 System origin으로 처리한다.
    pub(crate) fn handle_core_event_system(&mut self, source: DispatchSource, event: CoreEvent) {
        let origin = IntentOrigin::System;
        self.handle_core_event(source, &origin, event);
    }

    /// 창별 변경은 source·origin을 사용하고 전역 변경은 공용 상태에 적용한다.
    fn handle_core_event(
        &mut self,
        source: DispatchSource,
        origin: &IntentOrigin,
        event: CoreEvent,
    ) {
        match event {
            CoreEvent::SettingsUpdated(new_settings) => {
                self.cascade_settings_updated(new_settings);
            }
            CoreEvent::NotificationPushRequested {
                ws_id,
                surface_id,
                title,
                body,
                source: src,
            } => {
                self.cascade_notification_pushed(ws_id, surface_id, title, body, src);
            }
            CoreEvent::NotificationReadRequested { id } => {
                self.cascade_notification_read(id);
            }
            CoreEvent::AllNotificationsReadRequested => {
                self.cascade_all_notifications_read();
            }
            CoreEvent::SurfaceCwdChanged { surface_id } => {
                self.cascade_surface_cwd_changed(surface_id);
            }
            CoreEvent::TerminalMarkSet { surface_id } => {
                self.cascade_terminal_mark_set(surface_id);
            }
            CoreEvent::SurfaceCompletionRequested { surface_id, kind } => {
                self.cascade_surface_completion(surface_id, kind);
            }
            CoreEvent::SurfaceAttentionClearRequested { surface_id, kind } => {
                self.cascade_surface_attention_clear(surface_id, kind);
            }
            CoreEvent::WorkspaceCreated {
                id,
                index,
                surface_id,
                renamed_name,
                renamed_subtitle,
                renamed_description,
            } => {
                self.dispatch_workspace_created_cascade(
                    source,
                    origin,
                    WorkspaceCreatedCascade {
                        workspace_id: id,
                        index,
                        surface_id,
                        renamed_name,
                        renamed_subtitle,
                        renamed_description,
                    },
                );
            }
            CoreEvent::WorkspaceMetaUpdated {
                workspace_id,
                index: _,
                name,
                subtitle,
                description,
            } => {
                self.dispatch_workspace_meta_updated_cascade(
                    source,
                    workspace_id,
                    name,
                    subtitle,
                    description,
                );
            }
            CoreEvent::WorkspaceMoved {
                from_index,
                to_index,
                moved,
            } => {
                if moved {
                    self.dispatch_workspace_moved_cascade(source, from_index, to_index);
                }
            }
            CoreEvent::TabCreated {
                pane_id,
                tab_id,
                surface_id,
                tab_count: _,
                active_tab: _,
            } => {
                self.dispatch_tab_created_cascade(source, pane_id, tab_id, surface_id);
            }
            CoreEvent::TabClosed {
                tab_id,
                pane_id,
                closed,
                cleanup_targets,
            } => {
                if closed {
                    let is_user_close = origin.is_user();
                    self.dispatch_tab_closed_cascade(
                        source,
                        tab_id,
                        pane_id,
                        cleanup_targets,
                        is_user_close,
                    );
                }
            }
            CoreEvent::TabMoved { moved } => {
                if moved
                    && let DispatchSource::Main(wid) = source
                    && let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
                {
                    main.mark_dirty();
                }
            }
            CoreEvent::PaneSplit {
                workspace_index,
                original_pane_id,
                new_pane_id,
                new_surface_id,
                direction,
            } => {
                self.dispatch_pane_split_cascade(
                    source,
                    origin,
                    PaneSplitCascade {
                        workspace_index,
                        original_pane_id,
                        new_pane_id,
                        new_surface_id,
                        direction,
                    },
                );
            }
            CoreEvent::SurfaceSplit {
                workspace_index,
                pane_id,
                new_surface_id,
            } => {
                self.dispatch_surface_split_cascade(
                    source,
                    origin,
                    workspace_index,
                    pane_id,
                    new_surface_id,
                );
            }
            CoreEvent::PaneClosed {
                pane_id,
                closed,
                cleanup_targets,
            } => {
                if closed {
                    let is_user_close = origin.is_user();
                    self.dispatch_pane_closed_cascade(
                        source,
                        pane_id,
                        cleanup_targets,
                        is_user_close,
                    );
                }
            }
            ev @ CoreEvent::SurfaceClosed { .. } => {
                if let Some(c) = SurfaceCloseCascade::from_surface_closed(ev, origin.is_user()) {
                    self.dispatch_surface_closed_cascade(source, c);
                }
            }
            CoreEvent::SurfaceConverted {
                surface_id,
                replaced,
                ..
            } => {
                // 같은 surface ID를 새 콘텐츠로 바꿨으면 옛 mesh를 버려 다시 초기화하게 한다.
                if replaced {
                    if let Some(mgr) = self.plugin_manager.as_mut() {
                        mgr.drop_egui_mesh_frame(surface_id);
                    }
                    if let DispatchSource::Main(wid) = source
                        && let Some(main) =
                            self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
                    {
                        main.mark_dirty();
                    }
                }
            }
            ev @ CoreEvent::MoveSurfaceApplied { .. } => {
                if let Some(c) =
                    SurfaceCloseCascade::from_move_surface_applied(ev, origin.is_user())
                {
                    self.dispatch_surface_closed_cascade(source, c);
                }
            }
            CoreEvent::SurfaceSent { .. } => {}
            CoreEvent::TerminalRespawned { .. } => {}
            CoreEvent::ClosedItemRestored { restored, kind } => {
                if restored {
                    self.dispatch_closed_item_restored_cascade(source, origin, kind);
                }
            }

            CoreEvent::TerminalNotification {
                surface_id,
                title,
                body,
            } => {
                self.cascade_terminal_notification(source, surface_id, title, body);
            }
            CoreEvent::TerminalBellRing { surface_id } => {
                self.cascade_terminal_bell_ring(source, surface_id);
            }
            CoreEvent::TerminalOutputMatch { surface_id, text } => {
                self.cascade_terminal_output_match(source, surface_id, text);
            }
            CoreEvent::TerminalTitleChanged { surface_id, title } => {
                self.cascade_terminal_title_changed(source, surface_id, title);
            }
            CoreEvent::TerminalCwdChanged { surface_id } => {
                self.cascade_terminal_pty_cwd_changed(source, surface_id);
            }
            CoreEvent::TerminalCommandCompleted {
                surface_id,
                exit_code,
            } => {
                self.cascade_terminal_command_completed(source, surface_id, exit_code);
            }
            CoreEvent::TerminalShellIntegrationHint { surface_id } => {
                self.cascade_terminal_shell_integration_hint(source, surface_id);
            }
            CoreEvent::TerminalClipboardSet { surface_id } => {
                self.cascade_terminal_clipboard_set(source, surface_id);
            }
            CoreEvent::TerminalProcessExited { surface_id } => {
                self.cascade_terminal_process_exited(source, surface_id);
            }
            CoreEvent::TabNameUpdated { .. } => {
                // OSC 제목은 저장 대상이 아니므로 레이아웃 저장 대신 화면 갱신만 요청한다.
                if let DispatchSource::Main(wid) = source
                    && let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
                {
                    main.mark_dirty();
                }
            }
            CoreEvent::LayoutSaved => {}
            CoreEvent::LayoutRestored { .. } => {
                // 레이아웃 복원은 직접 Core::apply를 호출한 부팅 경로가 결과를 처리한다.
            }
            CoreEvent::PluginLoaded { plugin_id, version } => {
                self.cascade_plugin_loaded(plugin_id, version)
            }
            CoreEvent::PluginEnableToggled { plugin_id, enabled } => {
                self.cascade_plugin_enable_toggled(plugin_id, enabled)
            }
            CoreEvent::PluginUnloaded { plugin_id, reason } => {
                self.cascade_plugin_unloaded(plugin_id, reason)
            }
            CoreEvent::PluginError {
                plugin_id,
                error_kind,
                message,
            } => self.cascade_plugin_error(plugin_id, error_kind, message),
            CoreEvent::PluginSurfaceKindRegistered {
                plugin_id,
                kind,
                rendering,
            } => self.cascade_plugin_surface_kind_registered(plugin_id, kind, rendering),
            CoreEvent::PluginRegistryChanged { plugin_id, change } => {
                self.cascade_plugin_registry_changed(plugin_id, change)
            }
            CoreEvent::PluginWindowDeclared {
                plugin_id,
                window_id,
            } => self.cascade_plugin_window_declared(plugin_id, window_id),
        }
    }

    fn dispatch_closed_item_restored_cascade(
        &mut self,
        source: DispatchSource,
        origin: &IntentOrigin,
        kind: crate::core::intent::RestoredKind,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_closed_item_restored(&mut main.state, &mut main.core_state, origin, kind);
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_closed_item_restored(state, engine, origin, kind);
            }
        }
    }

    fn cascade_terminal_notification(
        &mut self,
        source: DispatchSource,
        surface_id: u32,
        title: String,
        body: String,
    ) {
        let (state, engine, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, &mut main.core_state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, engine, None)
            }
        };
        if engine.settings.notification.enabled {
            let ws_id = state.active_workspace(engine).id;
            state.dispatch_intent(
                crate::core::intent::DomainIntent::PushNotification {
                    ws_id,
                    surface_id,
                    title,
                    body,
                    source: "host".to_string(),
                }
                .from_system(),
            );
        }
        let fired = engine
            .hook_manager
            .check_and_fire(surface_id, &[tasty_hooks::HookEvent::Notification]);
        let injector = self.core.host_ipc_injector.get().cloned();
        for f in fired {
            crate::hook_handler::trigger::execute_binding(
                &f.binding,
                injector.as_ref(),
                &f.event,
                &f.received,
                surface_id,
            );
            state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                hook_id: f.hook_id,
                event_kind: "notification".to_string(),
                surface_id,
                exit_code: None,
            });
        }
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    fn cascade_terminal_bell_ring(&mut self, source: DispatchSource, surface_id: u32) {
        let (state, engine, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, &mut main.core_state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, engine, None)
            }
        };
        // 사용자가 등록한 Bell 훅은 알림·벨 표시 설정을 꺼도 실행한다.
        if engine.settings.notification.enabled && engine.settings.general.bell_notification {
            let ws_id = state.active_workspace(engine).id;
            state.dispatch_intent(
                crate::core::intent::DomainIntent::PushNotification {
                    ws_id,
                    surface_id,
                    title: crate::i18n::t("notification.bell_title").to_string(),
                    body: String::new(),
                    source: "host".to_string(),
                }
                .from_system(),
            );
        }
        let fired = engine
            .hook_manager
            .check_and_fire(surface_id, &[tasty_hooks::HookEvent::Bell]);
        let injector = self.core.host_ipc_injector.get().cloned();
        for f in fired {
            crate::hook_handler::trigger::execute_binding(
                &f.binding,
                injector.as_ref(),
                &f.event,
                &f.received,
                surface_id,
            );
            state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                hook_id: f.hook_id,
                event_kind: "bell".to_string(),
                surface_id,
                exit_code: None,
            });
        }
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    /// 등록된 OutputMatch 훅은 알림 설정과 무관하게 확인한다.
    fn cascade_terminal_output_match(
        &mut self,
        source: DispatchSource,
        surface_id: u32,
        text: String,
    ) {
        let (state, engine, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, &mut main.core_state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, engine, None)
            }
        };
        let fired = engine
            .hook_manager
            .check_and_fire(surface_id, &[tasty_hooks::HookEvent::OutputMatch(text)]);
        let injector = self.core.host_ipc_injector.get().cloned();
        for f in fired {
            crate::hook_handler::trigger::execute_binding(
                &f.binding,
                injector.as_ref(),
                &f.event,
                &f.received,
                surface_id,
            );
            state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                hook_id: f.hook_id,
                event_kind: "output-match".to_string(),
                surface_id,
                exit_code: None,
            });
        }
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    fn cascade_terminal_title_changed(
        &mut self,
        source: DispatchSource,
        surface_id: u32,
        title: String,
    ) {
        let (state, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, _)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, None)
            }
        };
        state.enqueue_host_event(crate::state::PendingHostEvent::SurfaceTitleChanged {
            surface_id,
            title: title.clone(),
        });
        state.dispatch_intent(
            crate::core::intent::DomainIntent::UpdateTabName {
                surface_id,
                name: title,
            }
            .from_system(),
        );
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    fn cascade_terminal_pty_cwd_changed(&mut self, source: DispatchSource, surface_id: u32) {
        let (state, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, _)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, None)
            }
        };
        state.dispatch_intent(
            crate::core::intent::DomainIntent::SurfaceCwdChanged { surface_id }.from_system(),
        );
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    /// OSC 133 완료는 attention과 등록된 완료 훅에 모두 전달한다.
    /// 별도 알림 패널 항목은 만들지 않으며 훅에는 실제 exit_code를 넘긴다.
    fn cascade_terminal_command_completed(
        &mut self,
        source: DispatchSource,
        surface_id: u32,
        exit_code: Option<i32>,
    ) {
        let (state, engine, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, &mut main.core_state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, engine, None)
            }
        };
        tracing::debug!(
            surface_id,
            exit_code = ?exit_code,
            "command completed — raising surface attention"
        );
        engine.raise_attention(surface_id, AttentionKind::Completion);
        engine.mark_layout_dirty();
        let fired = engine.hook_manager.check_and_fire(
            surface_id,
            &[tasty_hooks::HookEvent::CommandCompleted(exit_code)],
        );
        let injector = self.core.host_ipc_injector.get().cloned();
        for f in fired {
            crate::hook_handler::trigger::execute_binding(
                &f.binding,
                injector.as_ref(),
                &f.event,
                &f.received,
                surface_id,
            );
            state.enqueue_host_event(crate::state::PendingHostEvent::HookFired {
                hook_id: f.hook_id,
                event_kind: "command-completed".to_string(),
                surface_id,
                // 작업 완료 전략이 성공·실패를 판정할 실제 종료 코드다.
                exit_code,
            });
        }
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    /// 셸 통합 미설치 추정을 배너로 알릴 뿐 자동 설정이나 attention 변경은 하지 않는다.
    fn cascade_terminal_shell_integration_hint(&mut self, source: DispatchSource, surface_id: u32) {
        let (state, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, _)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, None)
            }
        };
        state
            .banners
            .push(crate::adapters::ui::BannerState::persistent(
                crate::adapters::ui::banner::defs::BANNER_SHELL_INTEGRATION_MISSING,
                crate::adapters::ui::BannerScope::Surface(surface_id),
            ));
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    /// OSC 52 복사 안내만 표시한다. 클립보드 쓰기는 Core가 이미 처리했다.
    fn cascade_terminal_clipboard_set(&mut self, source: DispatchSource, surface_id: u32) {
        let state = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                &mut main.state
            }
            DispatchSource::Parked(idx) => {
                let Some((state, _)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                state
            }
        };
        state.toasts.push_info(
            crate::i18n::t("toast.copied_osc52"),
            crate::adapters::ui::ToastScope::Surface(surface_id),
        );
    }

    /// 종료 후 자원 정리·훅·알림은 공용 process_exit 처리로 전달한다.
    fn cascade_terminal_process_exited(&mut self, source: DispatchSource, surface_id: u32) {
        let (state, engine, dirty_main) = match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                (&mut main.state, &mut main.core_state, Some(&mut main.base))
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                (state, engine, None)
            }
        };
        super::process_exit::handle(&mut self.core, state, engine, surface_id);
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    fn dispatch_surface_closed_cascade(&mut self, source: DispatchSource, c: SurfaceCloseCascade) {
        let core = &mut self.core;
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_surface_closed(core, &mut main.state, &mut main.core_state, c);
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_surface_closed(core, state, engine, c);
            }
        }
    }

    fn dispatch_surface_split_cascade(
        &mut self,
        source: DispatchSource,
        origin: &IntentOrigin,
        workspace_index: usize,
        pane_id: u32,
        new_surface_id: u32,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_surface_split(
                    &mut main.state,
                    &mut main.core_state,
                    origin,
                    workspace_index,
                    pane_id,
                    new_surface_id,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_surface_split(
                    state,
                    engine,
                    origin,
                    workspace_index,
                    pane_id,
                    new_surface_id,
                );
            }
        }
    }

    fn dispatch_pane_split_cascade(
        &mut self,
        source: DispatchSource,
        origin: &IntentOrigin,
        c: PaneSplitCascade,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_pane_split(&mut main.state, &mut main.core_state, origin, c);
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_pane_split(state, engine, origin, c);
            }
        }
    }

    fn dispatch_tab_created_cascade(
        &mut self,
        source: DispatchSource,
        pane_id: u32,
        tab_id: u32,
        surface_id: u32,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_tab_created(
                    &mut main.state,
                    &main.core_state,
                    pane_id,
                    tab_id,
                    surface_id,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_tab_created(state, engine, pane_id, tab_id, surface_id);
            }
        }
    }

    fn dispatch_pane_closed_cascade(
        &mut self,
        source: DispatchSource,
        pane_id: u32,
        cleanup_targets: Vec<(u32, Option<String>)>,
        is_user_close: bool,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_pane_closed_full(
                    &mut main.state,
                    &mut main.core_state,
                    pane_id,
                    cleanup_targets,
                    is_user_close,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_pane_closed_full(state, engine, pane_id, cleanup_targets, is_user_close);
            }
        }
    }

    fn dispatch_tab_closed_cascade(
        &mut self,
        source: DispatchSource,
        tab_id: u32,
        pane_id: Option<u32>,
        cleanup_targets: Vec<(u32, Option<String>)>,
        is_user_close: bool,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_tab_closed_full(
                    &mut main.state,
                    &mut main.core_state,
                    tab_id,
                    pane_id,
                    cleanup_targets,
                    is_user_close,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_tab_closed_full(
                    state,
                    engine,
                    tab_id,
                    pane_id,
                    cleanup_targets,
                    is_user_close,
                );
            }
        }
    }

    /// workspace 이동 뒤에도 사용자가 보던 대상을 유지하도록 활성 인덱스를 보정한다.
    fn dispatch_workspace_moved_cascade(
        &mut self,
        source: DispatchSource,
        from_index: usize,
        to_index: usize,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_workspace_moved(&mut main.state, from_index, to_index);
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, _)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_workspace_moved(state, from_index, to_index);
            }
        }
    }

    fn dispatch_workspace_meta_updated_cascade(
        &mut self,
        source: DispatchSource,
        workspace_id: u32,
        name: Option<String>,
        subtitle: Option<String>,
        description: Option<String>,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_workspace_meta_updated(
                    &mut main.state,
                    workspace_id,
                    name,
                    subtitle,
                    description,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, _)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_workspace_meta_updated(state, workspace_id, name, subtitle, description);
            }
        }
    }

    fn dispatch_workspace_created_cascade(
        &mut self,
        source: DispatchSource,
        origin: &IntentOrigin,
        c: WorkspaceCreatedCascade,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let window_id = u64::from(wid);
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_workspace_created(
                    &mut main.state,
                    &mut main.core_state,
                    origin,
                    window_id,
                    c,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_workspace_created(state, engine, origin, 0, c);
            }
        }
    }

    // 플러그인 이벤트는 첫 MainView의 큐에 넣는다. 창이 없으면 이 경로는 큐에 넣지 않는다.

    /// 플러그인 매니저는 App 전체에서 공유하므로 창 source 없이 이벤트를 처리한다.
    pub(crate) fn cascade_plugin_events(&mut self, events: Vec<CoreEvent>) {
        for ev in events {
            match ev {
                CoreEvent::PluginLoaded { plugin_id, version } => {
                    self.cascade_plugin_loaded(plugin_id, version)
                }
                CoreEvent::PluginEnableToggled { plugin_id, enabled } => {
                    self.cascade_plugin_enable_toggled(plugin_id, enabled)
                }
                CoreEvent::PluginUnloaded { plugin_id, reason } => {
                    self.cascade_plugin_unloaded(plugin_id, reason)
                }
                CoreEvent::PluginError {
                    plugin_id,
                    error_kind,
                    message,
                } => self.cascade_plugin_error(plugin_id, error_kind, message),
                CoreEvent::PluginSurfaceKindRegistered {
                    plugin_id,
                    kind,
                    rendering,
                } => self.cascade_plugin_surface_kind_registered(plugin_id, kind, rendering),
                CoreEvent::PluginRegistryChanged { plugin_id, change } => {
                    self.cascade_plugin_registry_changed(plugin_id, change)
                }
                CoreEvent::PluginWindowDeclared {
                    plugin_id,
                    window_id,
                } => self.cascade_plugin_window_declared(plugin_id, window_id),
                other => {
                    tracing::warn!(
                        "cascade_plugin_events: non-plugin CoreEvent received: {:?}",
                        std::mem::discriminant(&other)
                    );
                }
            }
        }
    }

    fn enqueue_plugin_host_event(&mut self, ev: crate::state::PendingHostEvent) {
        let Some(main) = self.view.views.values_mut().find_map(|w| w.as_main_mut()) else {
            return;
        };
        main.state.enqueue_host_event(ev);
    }

    fn cascade_plugin_loaded(&mut self, plugin_id: String, version: String) {
        self.enqueue_plugin_host_event(crate::state::PendingHostEvent::PluginLoaded {
            plugin_id,
            version,
        });
    }

    fn cascade_plugin_enable_toggled(&mut self, plugin_id: String, enabled: bool) {
        self.enqueue_plugin_host_event(crate::state::PendingHostEvent::PluginEnableToggled {
            plugin_id,
            enabled,
        });
    }

    fn cascade_plugin_unloaded(
        &mut self,
        plugin_id: String,
        reason: tasty_plugin_protocol::events::LifecycleReason,
    ) {
        let reason_str = match reason {
            tasty_plugin_protocol::events::LifecycleReason::User => "user",
            tasty_plugin_protocol::events::LifecycleReason::Ipc => "ipc",
            tasty_plugin_protocol::events::LifecycleReason::Crash => "crash",
        };
        // 비활성 플러그인이 선언한 훅 이벤트는 새 등록에 사용할 수 없게 한다.
        self.core_state().plugin_hook_events.unregister(&plugin_id);
        self.enqueue_plugin_host_event(crate::state::PendingHostEvent::PluginUnloaded {
            plugin_id,
            reason: reason_str.to_string(),
        });
    }

    fn cascade_plugin_error(&mut self, plugin_id: String, error_kind: String, message: String) {
        self.enqueue_plugin_host_event(crate::state::PendingHostEvent::PluginError {
            plugin_id,
            error_kind,
            message,
        });
    }

    fn cascade_plugin_surface_kind_registered(
        &mut self,
        plugin_id: String,
        kind: String,
        rendering: String,
    ) {
        self.enqueue_plugin_host_event(
            crate::state::PendingHostEvent::PluginSurfaceKindRegistered {
                plugin_id,
                kind,
                rendering,
            },
        );
    }

    fn cascade_plugin_registry_changed(
        &mut self,
        plugin_id: String,
        change: crate::core::intent::PluginRegistryChange,
    ) {
        use crate::core::intent::PluginRegistryChange;
        let (change_kind, detail) = match change {
            PluginRegistryChange::Installed { version } => {
                ("installed", serde_json::json!({ "version": version }))
            }
            PluginRegistryChange::Removed => ("removed", serde_json::Value::Null),
            PluginRegistryChange::PermissionGranted { permission } => (
                "permission_granted",
                serde_json::json!({ "permission": permission }),
            ),
            PluginRegistryChange::PermissionRevoked { permission } => (
                "permission_revoked",
                serde_json::json!({ "permission": permission }),
            ),
        };
        self.enqueue_plugin_host_event(crate::state::PendingHostEvent::PluginRegistryChanged {
            plugin_id,
            change_kind: change_kind.to_string(),
            detail,
        });
    }

    fn cascade_plugin_window_declared(&mut self, plugin_id: String, window_id: String) {
        self.enqueue_plugin_host_event(crate::state::PendingHostEvent::PluginWindowDeclared {
            plugin_id,
            window_id,
        });
    }

    fn cascade_terminal_mark_set(&mut self, surface_id: u32) {
        for main in self.main_windows_iter_mut() {
            if let Some(t) = main.core_state.find_terminal_by_id_mut(surface_id) {
                t.set_mark();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if let Some(t) = engine.find_terminal_by_id_mut(surface_id) {
                t.set_mark();
                return;
            }
        }
    }

    fn cascade_surface_completion(&mut self, surface_id: u32, kind: AttentionKind) {
        for main in self.main_windows_iter_mut() {
            if main.core_state.has_surface(surface_id) {
                main.core_state.raise_attention(surface_id, kind);
                main.core_state.mark_layout_dirty();
                main.mark_dirty();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.has_surface(surface_id) {
                engine.raise_attention(surface_id, kind);
                engine.mark_layout_dirty();
                return;
            }
        }
    }

    /// 요청 후 다른 kind의 attention이 생겼으면 kind_filter로 남겨 둔다.
    /// IPC 핸들러가 이미 해제했을 수도 있어 필터 결과와 무관하게 화면 갱신을 요청한다.
    fn cascade_surface_attention_clear(
        &mut self,
        surface_id: u32,
        kind_filter: Option<AttentionKind>,
    ) {
        for main in self.main_windows_iter_mut() {
            if main.core_state.has_surface(surface_id) {
                if kind_filter.is_none_or(|k| main.core_state.attention_kind(surface_id) == Some(k))
                {
                    main.core_state.clear_attention(surface_id);
                }
                main.core_state.mark_layout_dirty();
                main.mark_dirty();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.has_surface(surface_id) {
                if kind_filter.is_none_or(|k| engine.attention_kind(surface_id) == Some(k)) {
                    engine.clear_attention(surface_id);
                }
                engine.mark_layout_dirty();
                return;
            }
        }
    }

    fn cascade_surface_cwd_changed(&mut self, surface_id: u32) {
        for main in self.main_windows_iter_mut() {
            if main.core_state.has_surface(surface_id) {
                main.core_state.refresh_tab_display_name(surface_id);
                main.core_state.mark_layout_dirty();
                main.mark_dirty();
                return;
            }
        }
        for (_, engine) in self.parked_states.iter_mut() {
            if engine.has_surface(surface_id) {
                engine.refresh_tab_display_name(surface_id);
                engine.mark_layout_dirty();
                return;
            }
        }
    }

    /// 창과 parked 상태의 설정을 모두 갱신해야 복원된 창이 옛 설정을 쓰지 않는다.
    fn cascade_settings_updated(&mut self, new_settings: Settings) {
        let prev_appearance = self
            .main_windows_iter_mut()
            .next()
            .map(|w| w.core_state.settings.appearance.clone());
        let prev_appearance = prev_appearance.or_else(|| {
            self.parked_states
                .first()
                .map(|(_, e)| e.settings.appearance.clone())
        });
        let prev_theme = prev_appearance.as_ref().map(|a| a.theme.clone());
        let prev_ui_scale = prev_appearance.as_ref().map(|a| a.ui_scale.clone());
        let prev_overrides = prev_appearance.as_ref().map(|a| a.theme_overrides.clone());
        let prev_language = self
            .main_windows_iter_mut()
            .next()
            .map(|w| w.core_state.settings.general.language.clone());
        let prev_language = prev_language.or_else(|| {
            self.parked_states
                .first()
                .map(|(_, e)| e.settings.general.language.clone())
        });
        let prev_categories_enabled = self
            .main_windows_iter_mut()
            .next()
            .map(|w| w.core_state.settings.general.workspace_categories_enabled);
        let prev_categories_enabled = prev_categories_enabled.or_else(|| {
            self.parked_states
                .first()
                .map(|(_, e)| e.settings.general.workspace_categories_enabled)
        });
        let categories_turned_off = prev_categories_enabled == Some(true)
            && !new_settings.general.workspace_categories_enabled;

        for main in self.main_windows_iter_mut() {
            main.core_state.settings = new_settings.clone();
            if categories_turned_off {
                main.core_state.collapse_categories_to_normal();
                main.core_state.layout_dirty.mark_dirty();
            }
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.settings = new_settings.clone();
            if categories_turned_off {
                engine.collapse_categories_to_normal();
                engine.layout_dirty.mark_dirty();
            }
        }
        if let Err(e) = new_settings.save() {
            // 메모리에 적용됐어도 다음 실행에 보존할 수 없는 실패이므로 오류로 남긴다.
            tracing::error!("failed to save settings: {e}");
        }

        let appearance_changed = prev_theme.as_deref()
            != Some(new_settings.appearance.theme.as_str())
            || prev_ui_scale.as_deref() != Some(new_settings.appearance.ui_scale.as_str())
            || prev_overrides.as_ref() != Some(&new_settings.appearance.theme_overrides);
        if appearance_changed {
            use crate::intent::UiIntent;
            if let Some(main) = self.main_windows_iter_mut().next() {
                main.state.dispatch_intent(
                    UiIntent::AppearanceChanged.from_user_menu("settings.appearance.changed"),
                );
            }
        } else {
            // ThemeRuntime을 함께 전달해 배율·모션 설정이 기본값으로 바뀌지 않게 한다.
            tasty_themes::install_global_with_runtime(
                &new_settings.appearance,
                new_settings.theme_runtime(),
            );
        }

        if let Some(mgr) = self.plugin_manager.as_mut() {
            use tasty_plugin_protocol::EventScope;
            use tasty_plugin_protocol::events::payloads::{LanguageChanged, ThemeChanged};
            if prev_theme.as_deref() != Some(new_settings.appearance.theme.as_str()) {
                mgr.emit_host_event(
                    "theme.changed",
                    &ThemeChanged {
                        theme_id: new_settings.appearance.theme.clone(),
                    },
                    EventScope::System,
                );
            }
            if prev_language.as_deref() != Some(new_settings.general.language.as_str()) {
                mgr.emit_host_event(
                    "language.changed",
                    &LanguageChanged {
                        language_code: new_settings.general.language.clone(),
                    },
                    EventScope::System,
                );
            }
        }

        // 바뀐 단축키가 macOS 메뉴 표시에도 반영되도록 재구성한다.
        #[cfg(target_os = "macos")]
        crate::macos_delegate::rebuild_main_menu(&new_settings.keybindings);
    }

    fn cascade_notification_pushed(
        &mut self,
        ws_id: u32,
        surface_id: u32,
        title: String,
        body: String,
        source: String,
    ) {
        let Some(wid) = self.find_main_with_workspace(ws_id) else {
            tracing::warn!(
                ws_id,
                "cascade NotificationPushRequested: workspace not found"
            );
            return;
        };
        let Some(window) = self.view.views.get_mut(&wid) else {
            return;
        };
        let Some(main) = window.as_main_mut() else {
            return;
        };

        let created_id =
            main.core_state
                .notifications
                .add(ws_id, surface_id, title.clone(), body.clone());
        if let Some(nid) = created_id {
            main.core_state
                .raise_attention(surface_id, AttentionKind::Completion);
            // OS 벨과의 중복을 피하려는 TerminalBellRing 표지는 사운드에서 제외한다.
            if main.core_state.settings.notification.sound && source != "TerminalBellRing" {
                self.core.sound_player().play();
            }
            main.state
                .enqueue_host_event(crate::state::PendingHostEvent::NotificationCreated {
                    id: nid,
                    title,
                    body,
                    source,
                });
        }
    }

    /// 창과 parked engine에 읽음 처리를 전달한다. attention 해제 여부는 engine이 정한다.
    fn cascade_notification_read(&mut self, id: u64) {
        for main in self.main_windows_iter_mut() {
            main.core_state.mark_notification_read(id);
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.mark_notification_read(id);
        }
    }

    fn cascade_all_notifications_read(&mut self) {
        for main in self.main_windows_iter_mut() {
            main.core_state.mark_all_notifications_read();
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.mark_all_notifications_read();
        }
    }
}

/// workspace 생성의 창별 후속 처리. 사용자 요청일 때만 활성 workspace를 옮긴다.
pub(crate) fn cascade_workspace_created(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    origin: &IntentOrigin,
    window_id: u64,
    c: WorkspaceCreatedCascade,
) {
    let name = engine
        .workspaces
        .get(c.index)
        .map(|w| w.name.clone())
        .unwrap_or_default();
    state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceCreated {
        workspace_id: c.workspace_id,
        window_id,
        name,
    });

    if c.renamed_name.is_some() || c.renamed_subtitle.is_some() || c.renamed_description.is_some() {
        state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
            workspace_id: c.workspace_id,
            name: c.renamed_name,
            subtitle: c.renamed_subtitle,
            description: c.renamed_description,
            user_direct: false,
        });
    }
    if let Some(surface_id) = c.surface_id {
        cascade_surface_created(state, engine, surface_id);
    }
    if origin.is_user() {
        state.active_workspace = c.index;
    }
}

/// 복원된 구조는 유지하되 사용자 요청에서만 포커스를 옮긴다.
/// 이 origin 검사를 호출 경로가 사용자 전용이라는 가정으로 대신하지 않는다.
pub(crate) fn cascade_closed_item_restored(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    origin: &IntentOrigin,
    kind: crate::core::intent::RestoredKind,
) {
    use crate::core::intent::RestoredKind;
    if !origin.is_user() {
        return;
    }
    match kind {
        RestoredKind::Nothing => {}
        RestoredKind::Workspace { new_ws_index } => {
            state.active_workspace = new_ws_index;
        }
        RestoredKind::TabIntoPane => {}
        RestoredKind::PaneIntoWorkspace { pane_id } => {
            state.active_workspace_mut(engine).focused_pane = pane_id;
        }
    }
}

/// 이동 뒤 활성 인덱스 보정은 AppState의 공용 함수에 맡긴다.
pub(crate) fn cascade_workspace_moved(
    state: &mut crate::state::AppState,
    from_index: usize,
    to_index: usize,
) {
    state.fix_workspace_pointers_after_move(from_index, to_index);
}

pub(crate) fn cascade_workspace_meta_updated(
    state: &mut crate::state::AppState,
    workspace_id: u32,
    name: Option<String>,
    subtitle: Option<String>,
    description: Option<String>,
) {
    if name.is_some() || subtitle.is_some() || description.is_some() {
        state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
            workspace_id,
            name,
            subtitle,
            description,
            user_direct: false,
        });
    }
}

#[cfg(test)]
#[path = "dispatch_domain_restore_tests.rs"]
mod restore_tests;
