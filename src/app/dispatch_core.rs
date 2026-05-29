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
            } => {
                self.cascade_notification_pushed(ws_id, surface_id, title, body);
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
                    source: "host".to_string(),
                });
        }
    }
}
