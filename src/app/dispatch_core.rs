//! `CoreIntent` 발행 진입점 + `CoreEvent` cascade dispatcher.
//!
//! Phase D 의 *Strangler Fig* 단계:
//! - `Core::apply` 는 *순수 이벤트 발행* (Core 가 도메인 데이터 보유 안 함, 진행 중)
//! - 실제 cascade (settings 적용 / plugin event 발화 / theme install 등) 는 `App`
//!   안에 결합되어 있어 본 dispatcher 가 `handle_core_event` 로 처리한다.
//!
//! 도메인 마이그레이션 진행에 따라 점진 *Core::apply 안으로 이동* 한다.

use tasty_settings::Settings;

use crate::adapters::ui::window::Window as _;
use crate::app::App;
use crate::core::intent::{CoreEvent, CoreIntent};

impl App {
    /// `CoreIntent` 발행. Core 가 *이벤트 목록* 반환 → 각 이벤트 cascade 처리.
    pub(crate) fn dispatch_core_intent(&mut self, intent: CoreIntent) -> anyhow::Result<()> {
        let events = self.core.apply(intent)?;
        for event in events {
            self.handle_core_event(event);
        }
        Ok(())
    }

    /// 모든 windows + parked 의 `AppState.pending_core_intents` 를 drain →
    /// 각 intent 를 `dispatch_core_intent` 로 처리. handler 호출 직후 호출되어
    /// handler 가 enqueue 한 intent 를 즉시 cascade 시킨다.
    pub(crate) fn dispatch_pending_core_intents(&mut self) {
        let mut batch: Vec<CoreIntent> = Vec::new();
        for w in self.windows.values_mut() {
            if let Some(main) = w.as_main_mut() {
                batch.append(&mut main.state.take_pending_core_intents());
            }
        }
        for (s, _) in self.parked_states.iter_mut() {
            batch.append(&mut s.take_pending_core_intents());
        }
        for intent in batch {
            if let Err(e) = self.dispatch_core_intent(intent) {
                tracing::warn!("dispatch_core_intent failed: {e}");
            }
        }
    }

    /// `CoreEvent` 처리 — Phase D 진행 중에는 *옛 cascade 코드의 위치 이동*.
    fn handle_core_event(&mut self, event: CoreEvent) {
        match event {
            CoreEvent::SettingsUpdated(new_settings) => {
                self.cascade_settings_updated(new_settings);
            }
            CoreEvent::NotificationPushRequested {
                ws_id,
                surface_id,
                title,
                body,
                source,
            } => {
                self.cascade_notification_pushed(ws_id, surface_id, title, body, source);
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
}
