//! 작업 스레드가 남긴 시스템 알림을 창의 활성 workspace 알림으로 옮긴다.

use super::App;
use crate::app::dispatch_domain::DispatchSource;

impl App {
    /// 포커스 창, 없으면 첫 창의 활성 workspace가 받는다. 창이 없으면 다음 회차까지 그대로 둔다.
    pub(crate) fn poll_system_notices(&mut self) {
        let Some((engine_id, ws_id)) = self.system_notice_target() else {
            return;
        };
        for notice in crate::system_notice::take() {
            let intent = crate::app::command::DomainIntent::PushNotification {
                level: notice.level,
                ws_id,
                surface_id: 0,
                title: notice.title,
                body: notice.body,
                source: notice.source.to_string(),
            }
            .from_system();
            if let Err(e) = self.dispatch_domain_intent(DispatchSource::Engine(engine_id), intent) {
                tracing::warn!("system notice not stored: {e}");
            }
        }
    }

    fn system_notice_target(&self) -> Option<(crate::runtime::engine_session::EngineId, u32)> {
        let wid = match self.focused_window() {
            Some(_) => self.view.focused_view_id,
            None => self
                .view
                .views
                .iter()
                .find(|(_, w)| w.as_main().is_some())
                .map(|(id, _)| *id),
        }?;
        let (main, engine) = self.engines().window_pair(wid)?;
        let index = main.state.active_workspace_index(engine.core);
        let ws_id = engine.core.workspace_at(index)?.id;
        Some((self.engines.of_window(wid)?, ws_id))
    }
}
