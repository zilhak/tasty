//! JournalApplication이 모은 복원·저장 사실을 알림 항목으로 옮긴다.
use crate::app::App;
use crate::app::journal::notices::{self, RecoveryNotice};
use crate::notification::NotificationLevel;

/// 알림 항목의 출처 표시. 사용자가 보는 출처 이름이 아니라 구분용 값이다.
const SOURCE: &str = "tasty.recovery";

impl App {
    /// 쌓인 알림을 받을 엔진의 첫 workspace에 넣는다. 아직 workspace가 없는 엔진의 알림은 다음
    /// 차례로 미룬다.
    pub(crate) fn deliver_recovery_notices(&mut self) {
        let mut pending = std::mem::take(&mut self.pending_recovery_notices);
        pending.extend(self.journal.take_notices());
        if pending.is_empty() {
            return;
        }
        let fallback = self
            .view
            .views
            .keys()
            .find_map(|window| self.engines.of_window(*window));
        for notice in notices::merge_same_level(pending) {
            let workspace = notice
                .engine
                .or(fallback)
                .and_then(|id| self.engines.get(id))
                .and_then(|engine| engine.core.local_workspaces().first().map(|ws| ws.id));
            let Some(workspace) = workspace else {
                self.pending_recovery_notices.push(notice);
                continue;
            };
            log(&notice);
            self.cascade_notification_pushed(crate::app::command::NotificationPush {
                ws_id: workspace,
                surface_id: 0,
                title: notice.title,
                body: notice.body,
                source: SOURCE.into(),
                level: notice.level,
                merge: crate::app::command::NotificationMerge::Separate,
            });
        }
    }
}

fn log(notice: &RecoveryNotice) {
    match notice.level {
        NotificationLevel::Error => tracing::error!("{}: {}", notice.title, notice.body),
        _ => tracing::warn!("{}: {}", notice.title, notice.body),
    }
}
