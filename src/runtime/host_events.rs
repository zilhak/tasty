//! Engine delivery queues. View selection only produces values; it owns no lifecycle executor.
use crate::core::host_event::PendingHostEvent;
use crate::runtime::engine_access::EngineMut;
impl EngineMut<'_> {
    pub fn enqueue_host_event(&mut self, event: PendingHostEvent) {
        self.runtime.pending_host_events.push(event);
    }

    pub fn take_pending_host_events(&mut self) -> Vec<PendingHostEvent> {
        std::mem::take(&mut self.runtime.pending_host_events)
    }

    /// 특정 surface에 속하지 않는 호스트 알림을 이 engine의 첫 workspace 출처로 남긴다.
    /// workspace가 없으면 알림 저장소에 넣을 출처가 없으므로 warn 로그만 남기고 false를 돌려준다.
    pub(crate) fn push_host_notification(
        &mut self,
        level: crate::notification::NotificationLevel,
        title: String,
        body: String,
    ) -> bool {
        let Some(ws_id) = self.workspace_at(0).map(|ws| ws.id) else {
            tracing::warn!("host notification skipped: no workspace ({title}: {body})");
            return false;
        };
        if let Some(id) =
            self.live
                .notifications
                .add_with_level(ws_id, 0, title.clone(), body.clone(), level)
        {
            self.enqueue_host_event(PendingHostEvent::NotificationCreated {
                id,
                level,
                title,
                body,
                source: "host".to_string(),
            });
        }
        true
    }
}
