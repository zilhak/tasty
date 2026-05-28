//! Plugins 모달이 큐에 쌓아둔 lifecycle 액션 (install/enable/grant/...) 을 매니저에 적용.

use crate::app::App;
use crate::{ipc, plugin, plugins_ui, window};

impl App {
    /// Drain pending actions from the plugins modal and apply them to the manager.
    /// Refreshes the modal's snapshot after applying.
    pub(crate) fn process_plugins_window_actions(&mut self) {
        let Some(modal_id) = self.view.active_modal_id else {
            return;
        };
        let Some(modal) = self.windows.get_mut(&modal_id) else {
            return;
        };
        let Some(plugins_window) = modal.as_any_mut().downcast_mut::<window::PluginsWindow>()
        else {
            return;
        };
        let actions = std::mem::take(&mut plugins_window.pending_actions);
        if actions.is_empty() {
            return;
        }

        let Some(mgr) = self.plugin_manager.as_mut() else {
            return;
        };

        let mut pending_toasts: Vec<(String, crate::ui::ToastKind)> = Vec::new();

        for action in actions {
            match action {
                plugins_ui::PluginsAction::SetEnabled { id, enabled } => {
                    let result = if enabled {
                        mgr.enable(&id)
                    } else {
                        mgr.disable(&id)
                    };
                    if let Err(e) = result {
                        tracing::warn!("plugins modal: set_enabled({id}, {enabled}) failed: {e}");
                    }
                }
                plugins_ui::PluginsAction::Grant { id, permission } => {
                    let resp = ipc::handler::plugin::handle_grant(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "id": id, "permission": permission }),
                    );
                    if resp.error.is_some() {
                        tracing::warn!(
                            "plugins modal: grant({id}, {permission}) failed: {:?}",
                            resp.error
                        );
                    }
                }
                plugins_ui::PluginsAction::Revoke { id, permission } => {
                    let resp = ipc::handler::plugin::handle_revoke(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "id": id, "permission": permission }),
                    );
                    if resp.error.is_some() {
                        tracing::warn!(
                            "plugins modal: revoke({id}, {permission}) failed: {:?}",
                            resp.error
                        );
                    }
                }
                plugins_ui::PluginsAction::Uninstall { id } => {
                    let resp = ipc::handler::plugin::handle_remove(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "id": id }),
                    );
                    if resp.error.is_some() {
                        tracing::warn!("plugins modal: uninstall({id}) failed: {:?}", resp.error);
                    } else {
                        plugin::mark_builtin_removed(mgr, &id);
                    }
                }
                plugins_ui::PluginsAction::OpenInstallDir { path } => {
                    if !crate::terminal_link::open_uri(&path) {
                        tracing::warn!("plugins modal: open install dir failed: {path}");
                    }
                }
                plugins_ui::PluginsAction::Install { src_path } => {
                    let resp = ipc::handler::plugin::handle_install(
                        Some(mgr),
                        serde_json::json!(0),
                        &serde_json::json!({ "path": src_path }),
                    );
                    pending_toasts.push(match (resp.error, resp.result) {
                        (Some(err), _) => (
                            crate::i18n::t_fmt("plugins.add_install_failed", &err.message),
                            crate::ui::ToastKind::Error,
                        ),
                        (None, Some(result)) => {
                            let installed = result
                                .get("installed")
                                .and_then(|v| v.as_str())
                                .unwrap_or("")
                                .to_string();
                            (
                                crate::i18n::t_fmt("plugins.add_installed", &installed),
                                crate::ui::ToastKind::Success,
                            )
                        }
                        (None, None) => (
                            crate::i18n::t_fmt("plugins.add_install_failed", "unknown error"),
                            crate::ui::ToastKind::Error,
                        ),
                    });
                }
            }
        }

        // 모든 lifecycle action 이후 도구 메뉴를 갱신. install/enable/disable/grant/
        // revoke/uninstall 어떤 경로든 ui.tool_item 권한 또는 plugin 활성 상태가
        // 바뀌었을 수 있으므로 매번 다시 수집한다 (low-cost).
        self.refresh_tool_registry();

        let snapshot = self.snapshot_plugins();
        if let Some(modal) = self.windows.get_mut(&modal_id) {
            if let Some(plugins_window) = modal.as_any_mut().downcast_mut::<window::PluginsWindow>()
            {
                plugins_window.refresh_snapshot(snapshot);
                for (msg, kind) in pending_toasts {
                    plugins_window.push_toast(msg, kind);
                }
            }
        }
    }
}
