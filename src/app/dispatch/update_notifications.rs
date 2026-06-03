//! Drain `UpdateStatus::pending_notify` and dispatch `PushNotification`.
//!
//! The background poller (`crate::state::update_check::spawn_poller`) sets
//! `pending_notify = Some(info)` on a None→Some version transition. The main
//! loop pops it here and turns it into a domain-routed notification, then
//! records `notified_version` so the same release isn't announced again.

use crate::app::App;

impl App {
    pub(crate) fn dispatch_pending_update_notifications(&mut self) {
        // Collect (workspace_id, info) pairs per MainView, releasing the
        // borrow before we dispatch the intent.
        let mut to_push: Vec<(u32, tasty_update::ReleaseInfo)> = Vec::new();
        for main in self.main_windows_iter_mut() {
            let info = {
                let mut guard = main.state.update_status.lock().unwrap();
                guard.pending_notify.take()
            };
            let Some(info) = info else {
                continue;
            };
            if main.core_state.workspaces.is_empty() {
                // No workspace to route to; re-stash and try next frame.
                main.state.update_status.lock().unwrap().pending_notify = Some(info);
                continue;
            }
            let ws_id = main.state.active_workspace(&main.core_state).id;
            // Record notified version up-front so a re-entrant poll won't
            // re-queue while we're mid-dispatch.
            main.state.update_status.lock().unwrap().notified_version = Some(info.version.clone());
            to_push.push((ws_id, info));
        }

        for (ws_id, info) in to_push {
            let title = crate::i18n::t("update.notify.title").to_string();
            let body = crate::i18n::t_fmt("update.notify.body", &format!("v{}", info.version));
            // Re-borrow the matching main to dispatch — we can't keep the
            // mutable borrow across iterations.
            if let Some(main) = self.focused_window_mut() {
                main.state.dispatch_intent(
                    crate::core::intent::DomainIntent::PushNotification {
                        ws_id,
                        surface_id: 0,
                        title,
                        body,
                        source: "update".to_string(),
                    }
                    .from_system(),
                );
            }
        }
    }
}
