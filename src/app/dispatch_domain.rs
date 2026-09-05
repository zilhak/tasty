//! `DomainIntent` 발행 진입점 + `CoreEvent` cascade dispatcher.
//!
//! Phase D 의 *Strangler Fig* 단계:
//! - `Core::apply` 는 *순수 이벤트 발행* (Core 가 도메인 데이터 보유 안 함, 진행 중)
//! - 실제 cascade (settings 적용 / plugin event 발화 / theme install 등) 는 `App`
//!   안에 결합되어 있어 본 dispatcher 가 `handle_core_event` 로 처리한다.
//!
//! 도메인 마이그레이션 진행에 따라 점진 *Core::apply 안으로 이동* 한다.

use tasty_settings::Settings;
use winit::window::WindowId;

use crate::app::App;
use crate::core::AttentionKind;
use crate::core::intent::CoreEvent;
use crate::intent::{DispatchedIntent, Intent, IntentOrigin};
use crate::view::ui::View as _;

/// Domain intent 발화 source. `dispatch_pending_intents` 가 per-window /
/// per-parked 분리해 origin 과 함께 보존한다. cascade 가 *어느 engine 에
/// 발화됐는지* 알아야 하는 경우 (예: workspace.create) 에 사용한다.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DispatchSource {
    Main(WindowId),
    Parked(usize),
}

/// `CoreEvent::SurfaceClosed` / `MoveSurfaceApplied` 가 공유하는 cascade 결과 —
/// 하나의 close 판정에서 나온 개념적 단위라 필드를 낱개로 끌고 다니지 않고 묶는다.
pub(crate) struct SurfaceCloseCascade {
    pub(crate) cascade_level: crate::core::intent::CascadeLevel,
    pub(crate) cleanup_targets: Vec<(u32, Option<String>)>,
    pub(crate) closed_tab_ids: Vec<u32>,
    pub(crate) closed_pane_ids: Vec<u32>,
    /// workspace 째 사라졌다면 그 **(인덱스, id)** — 짝을 타입이 강제한다.
    pub(crate) workspace_purged: Option<(usize, u32)>,
    pub(crate) workspaces_now_empty: bool,
    pub(crate) is_user_close: bool,
}

/// `CoreEvent::PaneSplit` 의 cascade 페이로드.
pub(crate) struct PaneSplitCascade {
    pub(crate) workspace_index: usize,
    pub(crate) original_pane_id: u32,
    pub(crate) new_pane_id: u32,
    pub(crate) new_surface_id: u32,
    pub(crate) direction: crate::model::SplitDirection,
}

/// `CoreEvent::WorkspaceCreated` 의 cascade 페이로드. `window_id` 는 발화
/// source(Main/Parked)에 따라 dispatcher 가 계산하는 라우팅값이라 별도 인자로 둔다.
pub(crate) struct WorkspaceCreatedCascade {
    pub(crate) workspace_id: u32,
    pub(crate) index: usize,
    pub(crate) surface_id: Option<u32>,
    pub(crate) renamed_name: Option<String>,
    pub(crate) renamed_subtitle: Option<String>,
    pub(crate) renamed_description: Option<String>,
}

impl App {
    /// `DomainIntent` 발행. Core 가 *이벤트 목록* 반환 → 각 이벤트 cascade 처리.
    ///
    /// `source` 는 발화 origin engine (main window 또는 parked state). Core::apply
    /// 가 해당 engine 을 mutate 하고, cascade 도 같은 engine/state 컨텍스트에서
    /// 후처리한다.
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

    /// `Core::process_pty_output` 의 결과 (CoreEvent) 를 *System origin* 으로
    /// cascade dispatch. PTY emit 은 사용자/에이전트 발화가 아니므로 항상 System.
    pub(crate) fn handle_core_event_system(&mut self, source: DispatchSource, event: CoreEvent) {
        let origin = IntentOrigin::System;
        self.handle_core_event(source, &origin, event);
    }

    /// `CoreEvent` 처리 — Phase D 진행 중에는 *옛 cascade 코드의 위치 이동*.
    /// `source` / `origin` 은 *발화 컨텍스트가 필요한 cascade* (workspace.create
    /// 의 host event + active 전환 등) 에서만 사용. 전역 cascade (settings,
    /// clipboard 등) 는 무시한다.
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
                // 추가 cascade 없음 — mark_layout_dirty 는 Core::apply 가 이미 처리.
                // main.mark_dirty 만 redraw 위해.
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
            CoreEvent::SurfaceClosed {
                surface_id: _,
                closed,
                cascade_level,
                cleanup_targets,
                closed_tab_ids,
                closed_pane_ids,
                workspace_purged,
                workspaces_now_empty,
            } => {
                if closed {
                    let is_user_close = origin.is_user();
                    self.dispatch_surface_closed_cascade(
                        source,
                        SurfaceCloseCascade {
                            cascade_level,
                            cleanup_targets,
                            closed_tab_ids,
                            closed_pane_ids,
                            workspace_purged,
                            workspaces_now_empty,
                            is_user_close,
                        },
                    );
                }
            }
            CoreEvent::SurfaceConverted {
                surface_id,
                replaced,
            } => {
                // mark_layout_dirty 와 send_fast_init 은 Core::apply 가 이미 처리.
                // egui-mesh(markdown 등)로 제자리 변환 시 같은 surface_id 에 stale frame
                // 이 남아 새 `surface.create`(새 file params)가 재발송되지 않는다 — frame
                // 을 버려 재-bootstrap 을 강제한다. egui-mesh 가 아닌 surface_id 면 no-op.
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
            CoreEvent::MoveSurfaceApplied {
                moved,
                b_cleanup,
                cascade_level,
                closed_tab_ids,
                closed_pane_ids,
                workspace_purged,
                workspaces_now_empty,
            } => {
                // 이동(replace) 완료. 의미상 "B 닫힘 + A 옛자리 구조 cascade" 라
                // `SurfaceClosed` cascade 를 그대로 재사용한다: cleanup_targets 는
                // 닫히는 B 하나 (PTY kill + surface.closed), 나머지 구조 필드는 A 의
                // 옛 tab/pane/workspace 닫힘 정보. A 의 surface 는 절대 cleanup 대상에
                // 넣지 않는다(살아서 이동). 슬롯 비움은 Core::apply 가 이미 처리.
                if moved {
                    let is_user_close = origin.is_user();
                    let cleanup_targets: Vec<(u32, Option<String>)> =
                        b_cleanup.into_iter().collect();
                    self.dispatch_surface_closed_cascade(
                        source,
                        SurfaceCloseCascade {
                            cascade_level,
                            cleanup_targets,
                            closed_tab_ids,
                            closed_pane_ids,
                            workspace_purged,
                            workspaces_now_empty,
                            is_user_close,
                        },
                    );
                }
            }
            CoreEvent::SurfaceSent { .. } => {
                // terminal output 은 PTY → AppEvent 경로로 자동 redraw 유도.
                // 추가 cascade 없음.
            }
            CoreEvent::TerminalRespawned { .. } => {
                // 추가 cascade 없음. handler 가 events 직접 받아서 response 처리.
            }
            CoreEvent::ClosedItemRestored { restored, kind } => {
                if restored {
                    self.dispatch_closed_item_restored_cascade(source, kind);
                }
            }

            // ─── Terminal cascade (D.3.C.C.8) — PTY emit 변환 ───
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
                // osc_title 은 layout.json 영속 대상 아님 → mark_layout_dirty 호출 X.
                // tab bar 표시 갱신을 위한 mark_dirty 만.
                if let DispatchSource::Main(wid) = source
                    && let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut())
                {
                    main.mark_dirty();
                }
            }
            CoreEvent::LayoutSaved => {
                // 추가 cascade 없음 — disk I/O + layout_dirty.clear() 는 Core::apply 에서 완료.
            }
            CoreEvent::LayoutRestored { .. } => {
                // caller 가 events 직접 검사하는 패턴 (D.3.C.D.4) — bootstrap context
                // 에서 active_workspace 추출해 state.switch_workspace 수행. 큐 경로
                // 라우팅이 아닌 직접 Core::apply 호출이라 본 arm 은 비워둔다.
            }
            // ─── Plugin lifecycle (D.3.C.G.2) ───
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

    /// `ClosedItemRestored` cascade — (Workspace kind 이면) active_workspace 갱신.
    /// TabIntoPane 은 engine 안 이미 attach 되었으므로 mark_dirty 만.
    fn dispatch_closed_item_restored_cascade(
        &mut self,
        source: DispatchSource,
        kind: crate::core::intent::RestoredKind,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.view.views.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_closed_item_restored(&mut main.state, &mut main.core_state, kind);
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_closed_item_restored(state, engine, kind);
            }
        }
    }

    /// `TerminalNotification` cascade — settings.notification gate 체크 → ws_id
    /// 추출 후 `PushNotification` Intent 큐잉 + hook 발화.
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

    /// `TerminalBellRing` cascade — settings.notification gate + Bell hook 발화.
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
        // 벨 토스트는 전역 notification gate 위에 벨 전용 토글을 한 겹 더 얹는다.
        // off 면 벨 토스트(제목 `t("notification.bell_title")`)/소리를 억제하되, 아래 hook 발화는
        // 그대로 둔다 —
        // 훅은 사용자가 명시적으로 등록한 자동화라 수동 반응(토스트)과 구분한다.
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

    /// `TerminalOutputMatch` cascade — 완성된 라인 하나를 등록된 `OutputMatch`
    /// 훅과 비교해 발화한다. Bell/Notification 과 달리 토스트/설정
    /// gate 가 없다 — 훅은 사용자가 명시적으로 등록한 자동화라 항상 그대로 둔다.
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

    /// `TerminalTitleChanged` cascade — SurfaceTitleChanged host event 발화
    /// (옛 동작 보존, plugin 호환) + 후속 `UpdateTabName` Intent 큐잉.
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

    /// `TerminalCwdChanged` cascade — `SurfaceCwdChanged` Intent 큐잉. 본 Intent
    /// 의 cascade 가 refresh_tab_display_name + mark_layout_dirty 처리.
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

    /// `TerminalCommandCompleted` cascade — OSC 133 D phase. 두 경로가
    /// 공존한다(상세 `docs/features/surface-highlight/index.md`):
    /// - **자동 경로**: exit code 무관하게 항상 `raise_attention` — 설정 없이
    ///   즉시 동작하는 기본 완료 신호. `notifications.add()` 를 호출하지 않으므로
    ///   `AttentionKind::Completion` 의 `effects_of().panel_item == false` 대로
    ///   알림 패널에는 아이템이 쌓이지 않는다.
    /// - **커스터마이즈 경로**: exit code 필터링 없이 항상 `HookEvent::CommandCompleted`
    ///   로 훅을 발화한다(등록된 패턴이 `None` 이면 임의 exit code 에, `Some(n)` 이면
    ///   그 값에만 매치). Bell/Notification 과 달리 설정 gate 가 없다 — 훅은 사용자가
    ///   명시적으로 등록한 자동화라 항상 그대로 둔다(`cascade_terminal_output_match`
    ///   와 동형).
    ///
    /// 두 경로 모두 같은 `exit_code` 를 그대로 참조만 할 뿐 소비하지 않는다 — 실제
    /// exit code 보존은 `command_index::on_boundary` 의 memory 기록이 담당.
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
        // 자동 경로 — 성공/실패(exit_code) 무관하게 항상 highlight. exit_code
        // 자체는 이 호출로 소비되지 않고 아래 hook 이벤트(`HookEvent::CommandCompleted`)
        // payload 와 `command_index::on_boundary` 의 memory 기록(`tasty.commands.*`
        // 의 `exit_code` 필드)에 그대로 보존된다 — highlight 발동이 그 정보를
        // 대체하거나 지우지 않는다.
        tracing::debug!(
            surface_id,
            exit_code = ?exit_code,
            "command completed — raising surface attention"
        );
        engine.raise_attention(surface_id, AttentionKind::Completion);
        engine.mark_layout_dirty();
        // 커스터마이즈 경로 — `HookEvent::CommandCompleted` 발화도 함께 배선되어
        // 있다. 위 자동 highlight 와 상호 배타적이지 않다 — 사용자가
        // `tasty set hook --event command-completed[:N] --command "..."` 로 원하는
        // exit code 만 골라 별도 동작(알림음 등)을 추가로 걸 수 있다.
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
                // 실제 관측된 exit code. push 완료 전략의 성공/실패
                // 판정(exit 0 → Succeeded, 비-0 → Failed)이 여기서만 값을 받는다.
                exit_code,
            });
        }
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    /// `TerminalShellIntegrationHint` cascade — OSC 133 셸 통합 미설치 추정.
    /// 마우스 캡처 안내 배너와 동일한 형태로 자동 조치 없이 설명만 하는 배너를 1 회
    /// 띄운다. highlight 는 여기서 전혀 건드리지 않는다.
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

    /// `TerminalClipboardSet` cascade — OSC 52 write 가시화 토스트(`toast.copied_osc52`).
    /// 시스템 clipboard 쓰기는 `Core::process_pty_output` 이 이미 처리. 같은 surface 의
    /// 반복 OSC 52 는 토스트 매니저의 동일-메시지-동일-스코프 coalesce 로 합쳐져
    /// 스택되지 않는다.
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

    /// `TerminalProcessExited` cascade — 옛 redraw.rs:130-160 의 PTY exit 처리를
    /// 그대로 이동. hook 발화 + ProcessExited host event + close + plugin
    /// lifecycle. *Intent 우회* — closed_items snapshot 은 push 하지 않고
    /// (옛 close_surface_by_id_no_snapshot 과 동등) lifecycle 큐만 push.
    /// is_user_close=true 분류는 옛 주석 (redraw.rs:155) 정책 그대로.
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
        let fired = engine
            .hook_manager
            .check_and_fire(surface_id, &[tasty_hooks::HookEvent::ProcessExit]);
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
                event_kind: "process-exit".to_string(),
                surface_id,
                exit_code: None,
            });
        }
        state.enqueue_host_event(crate::state::PendingHostEvent::ProcessExited { surface_id });
        // close_surface_by_id_no_snapshot 내부 (Case 1~5) 에서 cleanup_targets 전체에 대한
        // enqueue_surface_closed 가 발화된다 (R1 leak fix). 반환값(이미 닫힌 surface 여부)은
        // cascade 흐름에 영향 없어 의도적 무시.
        // intent-exempt: 처리 핸들러 본문의 cascade — Intent 는 흐름의 시작점에만 둔다
        state.close_surface_by_id_no_snapshot(engine, surface_id, true);
        if let Some(base) = dirty_main {
            base.dirty = true;
        }
    }

    /// `SurfaceClosed` cascade — cleanup_surface 각각 + (Case 4 면) workspace
    /// memory scope purge + active_workspace 보정 + (workspaces_now_empty 면)
    /// 새 default workspace 자동 생성.
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

    /// `SurfaceSplit` cascade — (User origin 이면) tab 의 focused_surface 변경.
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

    /// `PaneSplit` cascade — host event 발화 + (User origin 이면) focused_pane 변경.
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

    /// `TabCreated` cascade — host event (`tab.created` + `surface.created`)
    /// enqueue + polling baseline 동기화 + main.mark_dirty. workspace_id / kind
    /// 는 engine 에서 직접 lookup.
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

    /// `PaneClosed` cascade — cleanup_targets 별 `surface.closed` lifecycle
    /// enqueue + `pane.closed` host event enqueue + main.mark_dirty.
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

    /// `TabClosed` cascade — cleanup_targets 별 `surface.closed` lifecycle
    /// enqueue + `tab.closed` host event enqueue + polling baseline 동기화.
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

    /// `WorkspaceMoved` cascade — 발화 source 의 `active_workspace` 보정.
    /// 사용자가 보던 ws 가 계속 active 유지되도록 인덱스만 조정한다.
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

    /// `WorkspaceMetaUpdated` cascade 의 source 라우터. host event 발화만
    /// 처리하므로 origin 무시 (Update 는 Agent 가 IPC 로만 발화 — 사용자
    /// 단축키 경로 없음).
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

    /// `WorkspaceCreated` cascade 의 source 라우터. main/parked 분기 후
    /// `cascade_workspace_created` (free function) 호출.
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

    // ─── Plugin lifecycle cascade (D.3.C.G.2.b) ───
    //
    // 모두 *첫 main window* 의 state 에 PendingHostEvent 를 enqueue. 본 큐는
    // `dispatch/host_events.rs` 가 drain → `misc::emit_plugin_*` helper 호출 →
    // PluginManager.event_bus broadcast. 단일 발화점.

    /// `App::plugin_<op>` 가 반환한 CoreEvent 목록을 cascade 로 dispatch.
    /// Plugin lifecycle 은 `DispatchSource` 와 무관하므로 (PluginManager 가
    /// App-level singleton) 본 helper 가 source 결정 없이 cascade 직접 호출.
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
        // plugin 이 멈추면 선언했던 hook 이벤트도 검증 집합에서 제거 — 비활성
        // plugin 의 이벤트 hook 등록은 거부돼야 한다(dead-setting 방지).
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

    /// 특정 surface 의 read mark 를 설정한다. 응답 없는 fire-and-forget.
    /// surface 보유 engine (main/parked) 의 첫 매칭에 적용.
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

    /// Surface completion cascade — completion/needs-input producer(IPC/CLI,
    /// Claude 플러그인 훅)가 발동한 attention 신호. surface 를 보유한 engine 의
    /// `raise_attention(surface_id, kind)` 으로 attention 을 발동하고, main window
    /// 면 redraw 를 요청해 소비처(테두리·탭·개수 배지)가 즉시 갱신되게 한다.
    /// `cascade_terminal_mark_set` 미러.
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

    /// Surface attention 해제 cascade — `cascade_surface_completion` 의 역방향.
    /// surface 를 보유한 engine 의 `clear_attention(surface_id)` 로 attention 을
    /// 지우고, main window 면 redraw 를 요청해 소비처(테두리·탭·개수 배지)가 즉시
    /// 갱신되게 한다.
    ///
    /// `kind_filter` 가 `Some(k)` 면 **현재 기록된 kind 가 `k` 일 때만** 지운다 —
    /// 해제 요청을 만든 시점과 적용 시점 사이에 다른 producer 가 더 급한 kind 로
    /// 다시 발동했을 수 있고, 그것까지 지워버리면 "지금 답하지 않으면 멈추는" 신호가
    /// 조용히 사라진다. 필터가 걸려 아무것도 지우지 않은 경우에도 engine 탐색은
    /// 여기서 끝난다(surface 는 한 engine 에만 있다).
    ///
    /// redraw 요청(`mark_layout_dirty`/`mark_dirty`)은 필터 결과와 무관하게 건다 —
    /// IPC 경로는 핸들러(`ipc/handler/surface/attention.rs`)가 owner engine 에 해제를
    /// 이미 적용한 뒤 이 cascade 를 태우므로(headless 에는 cascade 자체가 없어 핸들러가
    /// 적용 주체다), 여기서 "지울 게 남아 있는지" 로 redraw 를 게이트하면 정작 방금
    /// 바뀐 화면이 갱신되지 않는다. 프레임 한 번의 비용이라 무조건 거는 쪽이 안전하다.
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

    /// Surface 의 cwd 가 바뀌었을 때 cascade. 모든 main window 를 순회해 해당
    /// surface 를 보유한 main 의 engine 에 적용 — `refresh_tab_display_name`
    /// (탭 이름 prefix 갱신) + `mark_layout_dirty` (다음 capture 가 새 cwd 반영).
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

    /// Settings cascade — main/parked 의 settings 갱신, 디스크 저장, theme 적용,
    /// plugin event 발화. `modal.rs` 의 close_active_modal 에서 추출.
    ///
    /// INVARIANT: settings 는 main + parked 두 곳 모두 갱신해야 한다. parked 만
    /// 있는 상태에서 settings 변경 후 윈도우가 복원되면 옛 settings 로 살아나는 버그.
    fn cascade_settings_updated(&mut self, new_settings: Settings) {
        // SettingsView 는 단일 SoT — prev/new 글로벌 비교로 충분.
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
        // Colors picker 가 채우는 색 override 도 라이브 반영 대상 — theme/ui_scale 과
        // 함께 비교해 broadcast 여부를 정한다.
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
        // S-WSCAT — 워크스페이스 카테고리 토글 전환 감지(§4-2). on→off 면 normal 외
        // 모든 카테고리를 제거하고 워크스페이스를 normal 로 귀속한다(전역 인덱스 불변).
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
            // 사용자가 설정 화면에서 방금 바꾼 값이라 저장 실패는 그 작업이 통째로
            // 사라진다는 뜻이다 — error-handling.md 표의 "설정 저장 실패" 그 자체.
            tracing::error!("failed to save settings: {e}");
        }

        // Appearance (theme 색상 or host UI zoom) 변경 시 `UiIntent::AppearanceChanged`
        // 발화 → dispatcher 가 모든 윈도우 (main + modal) 의 GpuState 에 broadcast.
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
            // 색/zoom 외 변화 (예: tab_width / font 등) 면 broadcast 불필요하나,
            // 전역 Theme 가 settings 의 다른 partial 색 override 를 반영해야 할 수도
            // 있어 install_global 로 그대로 갱신. 설정에서 오는 런타임 값은
            // `Settings::theme_runtime()` 이 통째로 낸다 — 예전처럼 값을 하나씩
            // 인자로 넘기면 하나를 빠뜨렸을 때 전역 Theme 가 기본값으로 리셋된다.
            tasty_themes::install_global_with_runtime(
                &new_settings.appearance,
                new_settings.theme_runtime(),
            );
        }

        // Event Bus 1.0: theme/language 변경 발화.
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

        // macOS NSMenu 의 key equivalent 표시는 KeybindingSettings 의 quit /
        // new_window 에서 가져오므로, 변경 시 NSMenu 를 rebuild 해야 표시가 stale
        // 상태로 남지 않는다. cascade_settings_updated 는 single entry-point 이므로
        // 본 위치 1 곳만으로 모든 settings save 경로를 커버. 다른 OS 는 no-op.
        #[cfg(target_os = "macos")]
        crate::macos_delegate::rebuild_main_menu(&new_settings.keybindings);
    }

    /// Notification cascade — workspace 라우팅 후 store.add + host event enqueue.
    /// 옛 IPC handler (`handler/notification.rs`) 의 mutate 경로 이동.
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
            // toast producer — 신규 알림(coalesce 아님)이면 그 surface 에 attention 을
            // 발동. 옛날엔 NotificationStore.add() 내부 insert 였으나 attention 이
            // producer 중립 공유 상태로 이전되면서 producer 측에서 발동한다.
            main.core_state
                .raise_attention(surface_id, AttentionKind::Completion);
            // sound gate — coalesce 가 묶지 않은 신규 발화일 때만 재생.
            // Bell 경로는 OS 가 \a 처리 시점에 자체 beep 할 수 있어 안전 default
            // 로 skip — 향후 실측 후 정책 완화 가능.
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

    /// 특정 알림 읽음 처리 cascade — 알림을 보유한 첫 main/parked engine 에 적용.
    /// NotificationId 는 모든 engine 에 걸쳐 unique 라 첫 매칭만 처리.
    /// highlight clear 엣지 케이스(같은 surface 의 다른 알림이 안읽음이면 유지)는
    /// `CoreState::mark_notification_read`가 처리한다.
    fn cascade_notification_read(&mut self, id: u64) {
        for main in self.main_windows_iter_mut() {
            main.core_state.mark_notification_read(id);
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.mark_notification_read(id);
        }
    }

    /// 모든 알림 읽음 처리 cascade — main/parked 모두 적용.
    /// highlight clear 는 `CoreState::mark_all_notifications_read`가 처리한다.
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

/// `CoreEvent::WorkspaceCreated` 의 외부 cascade. *해당 engine + state*
/// 만 만지므로 App 메서드가 아닌 free function — IPC handler 도 events
/// 받아서 직접 호출 가능 (Step 6).
///
/// - `renamed_*` 가 하나라도 `Some` 이면 `PendingHostEvent::WorkspaceRenamed`
///   enqueue (plugin event bus 발화 경로).
/// - `origin` 이 User 면 `state.active_workspace = index` 로 active 전환.
/// - engine 의 `mark_layout_dirty` / `send_fast_init` 는 `Core::apply` 안에서
///   이미 처리됐다.
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

/// `CoreEvent::ClosedItemRestored` 의 외부 cascade.
/// - `Workspace`: `state.active_workspace = new_ws_index` (사용자가 복원한 ws 로
///   포커스 이동 — restore 는 사용자 단축키 only 라 origin 분기 불요).
/// - `TabIntoPane`: 별도 mutate 없음 (engine 안 이미 push 완료).
/// - `PaneIntoWorkspace`: 복원된 pane 으로 `focused_pane` 이동 — Workspace
///   케이스와 같은 취지, 대상이 워크스페이스 전체 대신 그 안의 pane 일 뿐.
/// - `Nothing`: no-op.
///
/// Parked 경로도 동일 함수 호출 — Parked engine 의 `state.active_workspace` 도
/// 같은 의미라 일관 처리 (현재 호출처는 사용자 단축키 only 라 Main 만 도달하지만
/// 다른 cascade 와 시그니처 정렬을 위해 Parked 분기 유지).
pub(crate) fn cascade_closed_item_restored(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    kind: crate::core::intent::RestoredKind,
) {
    use crate::core::intent::RestoredKind;
    match kind {
        RestoredKind::Nothing => {}
        RestoredKind::Workspace { new_ws_index } => {
            state.active_workspace = new_ws_index;
        }
        RestoredKind::TabIntoPane => {
            // engine 안에서 이미 attach 완료. AppState 측 변경 없음.
        }
        RestoredKind::PaneIntoWorkspace { pane_id } => {
            state.active_workspace_mut(engine).focused_pane = pane_id;
        }
    }
}

/// `CoreEvent::SurfaceClosed` 의 외부 cascade.
/// 1. 각 cleanup_target 에 `AppState::cleanup_surface` 호출
/// 2. cascade_level 별 host event (`tab.closed` / `pane.closed` / `workspace.closed`)
///    enqueue + baseline 동기화. `surface.closed` 자체는 별 큐
///    (`pending_lifecycle_events`) 가 처리하므로 여기선 안 다룸.
/// 3. workspace_purged 가 Some 이면 memory scope purge
/// 4. 같은 경우 활성 포인터(`active_workspace` · 카테고리 last-active)를 제거 위치
///    기준으로 보정 — 범위 초과 clamp 만으로는 사용자가 보던 것보다 **앞쪽**
///    workspace 가 빠질 때 인덱스가 유효한 채 다른 workspace 를 가리킨다(원칙 1)
pub(crate) fn cascade_surface_closed(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    c: SurfaceCloseCascade,
) {
    // C5 — 1. 각 cleanup_target 에 `AppState::cleanup_surface` 호출
    let surfaces = c.cleanup_targets.len();
    cleanup_closed_surfaces(state, engine, c.cleanup_targets, c.is_user_close);

    // 2. cascade_level 별 host event (`tab.closed` / `pane.closed`) enqueue +
    //    baseline 동기화. `surface.closed` 자체는 별 큐
    //    (`pending_lifecycle_events`) 가 처리하므로 여기선 안 다룸.
    enqueue_closed_tab_events(state, &c.closed_tab_ids, &c.closed_pane_ids);
    enqueue_closed_pane_events(state, &c.closed_pane_ids);

    // workspace 가 통째로 사라진 경우에만 도는 두 단계. 하나의 `Option<(usize, u32)>`
    // 라 "purge 는 했는데 포인터 보정은 안 했다" 가 성립하지 않는다.
    //    C4 — 3. memory scope purge + `workspace.closed` host event
    //    4. 인덱스 SoT 인 활성 포인터를 제거 위치 기준으로 보정
    // `workspace_purged` 는 Workspace level cascade 에서만 실린다 — 둘이 어긋나면
    // 보정이 조용히 건너뛰어져 사용자 화면이 밀리므로 debug 에서 고정한다.
    debug_assert_eq!(
        matches!(
            c.cascade_level,
            crate::core::intent::CascadeLevel::Workspace
        ),
        c.workspace_purged.is_some(),
        "workspace level cascade 와 제거 위치는 함께 실려야 한다"
    );
    if let Some((removed_idx, workspace_id)) = c.workspace_purged {
        // 제거 후 공통 뒷정리는 초크포인트 하나가 한다 — 제거 경로 셋이 각자 쏘던
        // 때 인라인 cascade 가 `workspace.closed` 를 빠뜨렸다.
        state.after_workspace_removed(workspace_id, "cascade");
        state.fix_workspace_pointers_after_removal(removed_idx, engine.workspaces.len());
    }

    recreate_workspace_if_now_empty(core, state, engine, c.workspaces_now_empty);

    // close_total — workspace level cascade 일 때만. `Core::close_case_workspace`
    // 가 무장한 t0 을 여기서 소비하므로, tab/pane level cascade 는 무장 자체가
    // 없어 `None` 으로 빠진다.
    if let Some((t0, snapshot)) = crate::close_trace::take_cascade() {
        crate::close_trace::log_total(t0, surfaces, snapshot, "cascade");
    }
}

/// `cascade_surface_closed` 1 단계: cleanup_targets 의 sibling 들이 plugin
/// lifecycle 큐에 빠짐없이 들어가야 ClaudeState child registry leak 이 발생하지
/// 않음 (R1 분석 참조). `Core::apply_close_surface` 가 이미 layout mutate 후
/// cleanup_targets 를 채우므로 surface_kind 가 None 일 수 있음 — payload 변환에서
/// 빈 문자열로 폴백한다.
fn cleanup_closed_surfaces(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    let t_loop = std::time::Instant::now();
    let mut sums = crate::close_trace::CleanupSums::default();
    for (sid, pid) in cleanup_targets {
        let kind = state.surface_kind(engine, sid);
        state.cleanup_surface_traced(engine, sid, pid, &mut sums);
        state.enqueue_surface_closed(sid, kind, is_user_close);
    }
    sums.log(t_loop.elapsed(), "cascade");
}

/// `cascade_surface_closed` 2 단계 (tab): 닫힌 tab 마다 `tab.closed` host event
/// enqueue + baseline 에서 제거. pane_id 는 close 후 못 찾으므로 closed_pane_ids
/// 의 첫 항목을 사용한다 (Tab level cascade 는 pane 안 닫혀 closed_pane_ids 비어
/// 있음 — 이때는 baseline 에서 lookup).
fn enqueue_closed_tab_events(
    state: &mut crate::state::AppState,
    closed_tab_ids: &[u32],
    closed_pane_ids: &[u32],
) {
    for tab_id in closed_tab_ids {
        let pane_id = closed_pane_ids.first().copied().unwrap_or_else(|| {
            state
                .last_tab_locations
                .as_ref()
                .and_then(|m| m.get(tab_id))
                .map(|(p, _, _)| *p)
                .unwrap_or(0)
        });
        state.enqueue_host_event(crate::state::PendingHostEvent::TabClosed {
            tab_id: *tab_id,
            pane_id,
        });
        state.lifecycle_baseline_remove_tab(*tab_id);
    }
}

/// `cascade_surface_closed` 2 단계 (pane): 닫힌 pane 마다 `pane.closed` host
/// event enqueue.
fn enqueue_closed_pane_events(state: &mut crate::state::AppState, closed_pane_ids: &[u32]) {
    for pane_id in closed_pane_ids {
        state.enqueue_host_event(crate::state::PendingHostEvent::PaneClosed { pane_id: *pane_id });
    }
}

/// `cascade_surface_closed` 마지막 단계: 마지막 surface 가 닫혀 workspaces 가
/// 비면 invariant 복구 위해 새 workspace 자동 생성. 사용자/에이전트/시스템 누구의
/// close 든 origin 분기 없이 동일 처리 — 빈 화면 redraw panic 방지가 목적. 옛
/// *세 호출처* (intent/surface.rs, ipc/close.rs,
/// pane.rs::close_surface_by_id_no_snapshot) 의 중복 분기를 단일 지점으로 통합.
fn recreate_workspace_if_now_empty(
    core: &mut crate::core::Core,
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    workspaces_now_empty: bool,
) {
    if !workspaces_now_empty {
        return;
    }
    match core.create_default_workspace(engine) {
        Ok(idx) => state.active_workspace = idx,
        Err(e) => tracing::warn!("auto-recreate workspace after SurfaceClosed failed: {e}"),
    }
}

/// `CoreEvent::SurfaceSplit` 의 외부 cascade. host event (`surface.created`)
/// 발화 + (User origin 이면) 해당 pane 의 active tab 의 focused_surface 를
/// new_surface_id 로 변경.
pub(crate) fn cascade_surface_split(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    origin: &IntentOrigin,
    workspace_index: usize,
    pane_id: u32,
    new_surface_id: u32,
) {
    cascade_surface_created(state, engine, new_surface_id);
    if !origin.is_user() {
        return;
    }
    if let Some(ws) = engine.workspaces.get_mut(workspace_index)
        && let Some(pane) = ws.pane_layout_mut().find_pane_mut(pane_id)
        && let Some(tab) = pane.active_tab_mut()
    {
        tab.focused_surface = new_surface_id;
    }
}

/// `CoreEvent::PaneSplit` 의 외부 cascade. host events (`pane.split` +
/// `pane.created`) 발화 + polling baseline 동기화 + (User origin 이면)
/// workspace 의 focused_pane 을 new_pane_id 로 변경.
pub(crate) fn cascade_pane_split(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    origin: &IntentOrigin,
    c: PaneSplitCascade,
) {
    state.enqueue_host_event(crate::state::PendingHostEvent::PaneSplit {
        original_pane: c.original_pane_id,
        new_pane: c.new_pane_id,
        direction: c.direction,
    });
    let workspace_id = engine.workspaces.get(c.workspace_index).map(|w| w.id);
    if let Some(workspace_id) = workspace_id {
        state.enqueue_host_event(crate::state::PendingHostEvent::PaneCreated {
            pane_id: c.new_pane_id,
            workspace_id,
        });
    }
    cascade_surface_created(state, engine, c.new_surface_id);
    if origin.is_user()
        && let Some(ws) = engine.workspaces.get_mut(c.workspace_index)
    {
        ws.focused_pane = c.new_pane_id;
    }
}

/// `CoreEvent::PaneClosed` 의 외부 cascade. host event (`pane.closed`) enqueue.
pub(crate) fn cascade_pane_closed(state: &mut crate::state::AppState, pane_id: u32) {
    state.enqueue_host_event(crate::state::PendingHostEvent::PaneClosed { pane_id });
}

/// `CoreEvent::PaneClosed` 의 full cascade — cleanup_targets 별 surface 자원
/// 정리 + `surface.closed` lifecycle enqueue + `pane.closed` host event enqueue.
/// `cascade_surface_closed` 와 동일하게 surface 별 kind 캡쳐는 cleanup 호출 전.
/// dispatcher (`App::dispatch_pane_closed_cascade`) 와 IPC handler 양쪽이 공유.
pub(crate) fn cascade_pane_closed_full(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    pane_id: u32,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    for (sid, pid) in cleanup_targets {
        let kind = state.surface_kind(engine, sid);
        state.cleanup_surface(engine, sid, pid);
        state.enqueue_surface_closed(sid, kind, is_user_close);
    }
    cascade_pane_closed(state, pane_id);
}

/// 새 surface 생성 시 공통 host event 발화 — TabCreated / PaneSplit / SurfaceSplit
/// / WorkspaceCreated cascade 가 모두 사용. `surface_id` 의 위치 정보를 engine
/// 에서 lookup 해 `PendingHostEvent::SurfaceCreated` enqueue.
pub(crate) fn cascade_surface_created(
    state: &mut crate::state::AppState,
    engine: &crate::core::CoreState,
    surface_id: u32,
) {
    let Some((tab_id, pane_id, workspace_id, kind)) = find_surface_location(engine, surface_id)
    else {
        return;
    };
    state.enqueue_host_event(crate::state::PendingHostEvent::SurfaceCreated {
        surface_id,
        kind,
        tab_id,
        pane_id,
        workspace_id,
        created_by_plugin: None,
    });
}

/// `surface_id` 의 host event 발화에 필요한 위치 + kind 를 모든 workspace 순회로
/// 찾는다 (focused 의존 없음). 못 찾으면 `None` — surface 가 아직 layout 에 안
/// 들어가 있거나 lazy init 인 케이스.
fn find_surface_location(
    engine: &crate::core::CoreState,
    surface_id: u32,
) -> Option<(u32, u32, u32, &'static str)> {
    for ws in &engine.workspaces {
        let workspace_id = ws.id;
        for pane_id in ws.pane_layout().all_pane_ids() {
            let Some(pane) = ws.pane_layout().find_pane(pane_id) else {
                continue;
            };
            for tab in &pane.tabs {
                let Some(layout) = tab.layout_if_initialized() else {
                    continue;
                };
                if let Some(s) = layout.find_surface(surface_id) {
                    return Some((tab.id, pane_id, workspace_id, s.kind()));
                }
            }
        }
    }
    None
}

/// `CoreEvent::WorkspaceMoved` 의 외부 cascade. 사용자 포커스 보존을 위해 인덱스로
/// 저장된 활성 포인터를 보정한다 — 규칙 자체는
/// `AppState::fix_workspace_pointers_after_move` 하나에만 있다(같은 규칙을 여기에
/// 복제하면 `move_workspace` 와 갈린다).
///
/// IPC handler 도 직접 호출 가능.
pub(crate) fn cascade_workspace_moved(
    state: &mut crate::state::AppState,
    from_index: usize,
    to_index: usize,
) {
    state.fix_workspace_pointers_after_move(from_index, to_index);
}

/// `CoreEvent::TabCreated` 의 외부 cascade. host events (`tab.created` +
/// `surface.created`) enqueue + polling baseline 동기화. workspace_id / kind 는
/// engine lookup.
pub(crate) fn cascade_tab_created(
    state: &mut crate::state::AppState,
    engine: &crate::core::CoreState,
    pane_id: u32,
    tab_id: u32,
    surface_id: u32,
) {
    let workspace_id = engine
        .workspaces
        .iter()
        .find(|w| w.pane_layout().find_pane(pane_id).is_some())
        .map(|w| w.id);
    let kind = engine
        .find_pane_by_id(pane_id)
        .and_then(|p| p.tabs.iter().find(|t| t.id == tab_id))
        .and_then(|t| t.focused_surface_id())
        .and_then(|sid| engine.find_surface_by_id(sid))
        .map(|s| s.kind().to_string())
        .unwrap_or_else(|| "unknown".to_string());
    if let Some(workspace_id) = workspace_id {
        state.enqueue_host_event(crate::state::PendingHostEvent::TabCreated {
            tab_id,
            pane_id,
            workspace_id,
            kind: kind.clone(),
        });
        state.lifecycle_baseline_insert_tab(tab_id, pane_id, workspace_id, kind);
    }
    cascade_surface_created(state, engine, surface_id);
}

/// `CoreEvent::TabClosed` 의 외부 cascade. host event (`tab.closed`) enqueue +
/// polling baseline 동기화. `pane_id` 가 `None` 이면 close 가 실패한 케이스
/// (find 못 함) — 아무것도 안 함.
pub(crate) fn cascade_tab_closed(
    state: &mut crate::state::AppState,
    tab_id: u32,
    pane_id: Option<u32>,
) {
    let Some(pane_id) = pane_id else {
        return;
    };
    state.enqueue_host_event(crate::state::PendingHostEvent::TabClosed { tab_id, pane_id });
    state.lifecycle_baseline_remove_tab(tab_id);
}

/// `CoreEvent::TabClosed` 의 full cascade — cleanup_targets 별 surface 자원
/// 정리 + `surface.closed` lifecycle enqueue + `tab.closed` host event enqueue
/// + baseline 동기화. dispatcher 와 IPC handler 양쪽이 공유.
pub(crate) fn cascade_tab_closed_full(
    state: &mut crate::state::AppState,
    engine: &mut crate::core::CoreState,
    tab_id: u32,
    pane_id: Option<u32>,
    cleanup_targets: Vec<(u32, Option<String>)>,
    is_user_close: bool,
) {
    for (sid, pid) in cleanup_targets {
        let kind = state.surface_kind(engine, sid);
        state.cleanup_surface(engine, sid, pid);
        state.enqueue_surface_closed(sid, kind, is_user_close);
    }
    cascade_tab_closed(state, tab_id, pane_id);
}

/// `CoreEvent::WorkspaceMetaUpdated` 의 외부 cascade. host event 발화 (rename
/// 필드 하나라도 Some 이면 `WorkspaceRenamed` enqueue). IPC handler 도 직접
/// 호출 가능.
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
