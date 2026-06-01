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

use crate::adapters::ui::window::Window as _;
use crate::app::App;
use crate::core::intent::CoreEvent;
use crate::intent::{DispatchedIntent, Intent, IntentOrigin};

/// Domain intent 발화 source. `dispatch_pending_intents` 가 per-window /
/// per-parked 분리해 origin 과 함께 보존한다. cascade 가 *어느 engine 에
/// 발화됐는지* 알아야 하는 경우 (예: workspace.create) 에 사용한다.
#[derive(Debug, Clone, Copy)]
pub(crate) enum DispatchSource {
    Main(WindowId),
    Parked(usize),
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
                let Some(main) = self.windows.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    anyhow::bail!("dispatch_domain_intent: main window {wid:?} not found");
                };
                core.apply(&mut main.engine_state, intent)?
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
            CoreEvent::InternalClipboardCopyRecorded { text } => {
                self.cascade_internal_clipboard_copy(text);
            }
            CoreEvent::WorkspaceCreated {
                id,
                index,
                surface_id: _,
                renamed_name,
                renamed_subtitle,
                renamed_description,
            } => {
                self.dispatch_workspace_created_cascade(
                    source,
                    origin,
                    id,
                    index,
                    renamed_name,
                    renamed_subtitle,
                    renamed_description,
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
                let Some(main) = self.windows.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
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
                let Some(main) = self.windows.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
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
        workspace_id: u32,
        index: usize,
        renamed_name: Option<String>,
        renamed_subtitle: Option<String>,
        renamed_description: Option<String>,
    ) {
        match source {
            DispatchSource::Main(wid) => {
                let Some(main) = self.windows.get_mut(&wid).and_then(|w| w.as_main_mut()) else {
                    return;
                };
                cascade_workspace_created(
                    &mut main.state,
                    &mut main.engine_state,
                    origin,
                    workspace_id,
                    index,
                    renamed_name,
                    renamed_subtitle,
                    renamed_description,
                );
                main.mark_dirty();
            }
            DispatchSource::Parked(idx) => {
                let Some((state, engine)) = self.parked_states.get_mut(idx) else {
                    return;
                };
                cascade_workspace_created(
                    state,
                    engine,
                    origin,
                    workspace_id,
                    index,
                    renamed_name,
                    renamed_subtitle,
                    renamed_description,
                );
            }
        }
    }

    /// Internal clipboard copy 를 모든 main + parked engine 의 history 에 기록.
    /// `clipboard_record::record_clipboard_data` 의 broadcast 패턴과 동일.
    fn cascade_internal_clipboard_copy(&mut self, text: String) {
        for main in self.main_windows_iter_mut() {
            main.engine_state.record_internal_copy(&text);
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.record_internal_copy(&text);
        }
    }

    /// 특정 surface 의 read mark 를 설정한다. 응답 없는 fire-and-forget.
    /// surface 보유 engine (main/parked) 의 첫 매칭에 적용.
    fn cascade_terminal_mark_set(&mut self, surface_id: u32) {
        for main in self.main_windows_iter_mut() {
            if let Some(t) = main.engine_state.find_terminal_by_id_mut(surface_id) {
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

    /// Surface 의 cwd 가 바뀌었을 때 cascade. 모든 main window 를 순회해 해당
    /// surface 를 보유한 main 의 engine 에 적용 — `refresh_tab_display_name`
    /// (탭 이름 prefix 갱신) + `mark_layout_dirty` (다음 capture 가 새 cwd 반영).
    fn cascade_surface_cwd_changed(&mut self, surface_id: u32) {
        for main in self.main_windows_iter_mut() {
            if main.engine_state.has_surface(surface_id) {
                main.engine_state.refresh_tab_display_name(surface_id);
                main.engine_state.mark_layout_dirty();
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
        // SettingsWindow 는 단일 SoT — prev/new 글로벌 비교로 충분.
        let prev_theme = self
            .main_windows_iter_mut()
            .next()
            .map(|w| w.engine_state.settings.appearance.theme.clone());
        let prev_theme = prev_theme.or_else(|| {
            self.parked_states
                .first()
                .map(|(_, e)| e.settings.appearance.theme.clone())
        });
        let prev_language = self
            .main_windows_iter_mut()
            .next()
            .map(|w| w.engine_state.settings.general.language.clone());
        let prev_language = prev_language.or_else(|| {
            self.parked_states
                .first()
                .map(|(_, e)| e.settings.general.language.clone())
        });

        for main in self.main_windows_iter_mut() {
            main.engine_state.settings = new_settings.clone();
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.settings = new_settings.clone();
        }
        if let Err(e) = new_settings.save() {
            tracing::warn!("failed to save settings: {e}");
        }
        // 새 settings 의 두 레이어로 전역 Theme 인스턴스 재구성.
        tasty_themes::install_global(&new_settings.appearance);

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
        let Some(window) = self.windows.get_mut(&wid) else {
            return;
        };
        let Some(main) = window.as_main_mut() else {
            return;
        };

        let created_id =
            main.engine_state
                .notifications
                .add(ws_id, surface_id, title.clone(), body.clone());
        if let Some(nid) = created_id {
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
    fn cascade_notification_read(&mut self, id: u64) {
        for main in self.main_windows_iter_mut() {
            main.engine_state.notifications.mark_read(id);
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.notifications.mark_read(id);
        }
    }

    /// 모든 알림 읽음 처리 cascade — main/parked 모두 적용.
    fn cascade_all_notifications_read(&mut self) {
        for main in self.main_windows_iter_mut() {
            main.engine_state.notifications.mark_all_read();
            main.mark_dirty();
        }
        for (_, engine) in self.parked_states.iter_mut() {
            engine.notifications.mark_all_read();
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
    engine: &mut crate::engine_state::CoreState,
    origin: &IntentOrigin,
    workspace_id: u32,
    index: usize,
    renamed_name: Option<String>,
    renamed_subtitle: Option<String>,
    renamed_description: Option<String>,
) {
    let _ = engine; // host event 발화 + active 전환만 — engine 은 Core::apply 가 이미 mutate.
    if renamed_name.is_some() || renamed_subtitle.is_some() || renamed_description.is_some() {
        state.enqueue_host_event(crate::state::PendingHostEvent::WorkspaceRenamed {
            workspace_id,
            name: renamed_name,
            subtitle: renamed_subtitle,
            description: renamed_description,
            user_direct: false,
        });
    }
    if origin.is_user() {
        state.active_workspace = index;
    }
}

/// `CoreEvent::WorkspaceMoved` 의 외부 cascade. 사용자 포커스 보존을 위해
/// active_workspace 인덱스를 보정한다.
/// - 이동한 ws 가 active 였으면 따라간다 (`active = to`).
/// - from 과 to 사이를 자기 위치가 통과하면 shift 보정.
/// IPC handler 도 직접 호출 가능.
pub(crate) fn cascade_workspace_moved(
    state: &mut crate::state::AppState,
    from_index: usize,
    to_index: usize,
) {
    if state.active_workspace == from_index {
        state.active_workspace = to_index;
    } else if from_index < to_index
        && state.active_workspace > from_index
        && state.active_workspace <= to_index
    {
        state.active_workspace -= 1;
    } else if from_index > to_index
        && state.active_workspace >= to_index
        && state.active_workspace < from_index
    {
        state.active_workspace += 1;
    }
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
