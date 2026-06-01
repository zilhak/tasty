//! Plugins 모달이 큐에 쌓아둔 lifecycle 액션 (install/enable/grant/...) 을 매니저에 적용.

use crate::app::App;
use crate::{plugin, plugins_ui, window};

impl App {
    /// Drain pending actions from the plugins modal and apply them to the manager.
    /// Refreshes the modal's snapshot after applying.
    pub(crate) fn process_plugins_window_actions(&mut self) {
        let Some(modal_id) = self.view.active_modal_id else {
            return;
        };
        let Some(modal) = self.view.windows.get_mut(&modal_id) else {
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

        if self.plugin_manager.is_none() {
            return;
        }

        let mut pending_toasts: Vec<(String, crate::adapters::ui::ToastKind)> = Vec::new();

        for action in actions {
            match action {
                plugins_ui::PluginsAction::SetEnabled { id, enabled } => {
                    let result = if enabled {
                        self.plugin_enable(id.clone())
                    } else {
                        self.plugin_disable(id.clone())
                    };
                    match result {
                        Ok(events) => self.cascade_plugin_events(events),
                        Err(e) => tracing::warn!(
                            "plugins modal: set_enabled({id}, {enabled}) failed: {e}"
                        ),
                    }
                }
                plugins_ui::PluginsAction::Grant { id, permission } => {
                    match self.plugin_grant(id.clone(), permission.clone()) {
                        Ok(events) => self.cascade_plugin_events(events),
                        Err(e) => {
                            tracing::warn!("plugins modal: grant({id}, {permission}) failed: {e}")
                        }
                    }
                }
                plugins_ui::PluginsAction::Revoke { id, permission } => {
                    match self.plugin_revoke(id.clone(), permission.clone()) {
                        Ok(events) => self.cascade_plugin_events(events),
                        Err(e) => {
                            tracing::warn!("plugins modal: revoke({id}, {permission}) failed: {e}")
                        }
                    }
                }
                plugins_ui::PluginsAction::Uninstall { id } => {
                    match self.plugin_remove(id.clone()) {
                        Ok(events) => {
                            self.cascade_plugin_events(events);
                            if let Some(mgr) = self.plugin_manager.as_mut() {
                                plugin::mark_builtin_removed(mgr, &id);
                            }
                        }
                        Err(e) => {
                            tracing::warn!("plugins modal: uninstall({id}) failed: {e}");
                        }
                    }
                }
                plugins_ui::PluginsAction::OpenInstallDir { path } => {
                    if !crate::terminal_link::open_uri(&path) {
                        tracing::warn!("plugins modal: open install dir failed: {path}");
                    }
                }
                plugins_ui::PluginsAction::Install { src_path } => {
                    let toast = match self.plugin_install(std::path::PathBuf::from(&src_path)) {
                        Ok(events) => {
                            // CoreEvent::PluginRegistryChanged 의 plugin_id 추출.
                            let installed = events
                                .iter()
                                .find_map(|ev| match ev {
                                    crate::core::intent::CoreEvent::PluginRegistryChanged {
                                        plugin_id,
                                        ..
                                    } => Some(plugin_id.clone()),
                                    _ => None,
                                })
                                .unwrap_or_default();
                            self.cascade_plugin_events(events);
                            (
                                crate::i18n::t_fmt("plugins.add_installed", &installed),
                                crate::adapters::ui::ToastKind::Success,
                            )
                        }
                        Err(e) => (
                            crate::i18n::t_fmt("plugins.add_install_failed", &e.to_string()),
                            crate::adapters::ui::ToastKind::Error,
                        ),
                    };
                    pending_toasts.push(toast);
                }
            }
        }

        // 모든 lifecycle action 이후 도구 메뉴를 갱신. install/enable/disable/grant/
        // revoke/uninstall 어떤 경로든 ui.tool_item 권한 또는 plugin 활성 상태가
        // 바뀌었을 수 있으므로 매번 다시 수집한다 (low-cost).
        self.refresh_tool_registry();

        let snapshot = self.snapshot_plugins();
        if let Some(modal) = self.view.windows.get_mut(&modal_id) {
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
