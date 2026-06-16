//! Plugins 모달이 큐에 쌓아둔 lifecycle 액션 (install/enable/grant/...) 을 매니저에 적용.

use crate::app::App;
use crate::{plugin, plugins_ui, window};

/// `TrustAndInstall` 의 핵심 절차 — known-plugins.toml 에 trust entry 추가 후
/// 일반 `plugin_install` 흐름 진행. trust 저장에 실패하면 install 자체 중단
/// (출처 미상 plugin 이 trust DB 미반영 상태로 disk 에 남아 다음 discover 에서
/// silent-loaded 되는 시나리오 차단).
fn record_trust_then_install(
    app: &mut App,
    src_path: &str,
    plugin_id: &str,
    pubkey_b64: &str,
    permissions: &[String],
    publisher_fingerprint: &str,
) -> anyhow::Result<Vec<crate::core::intent::CoreEvent>> {
    use tasty_host_plugin::known_plugins::{KnownPluginEntry, KnownPlugins};

    let mut db =
        KnownPlugins::load().map_err(|e| anyhow::anyhow!("load known-plugins.toml failed: {e}"))?;
    let entry = KnownPluginEntry {
        pubkey: pubkey_b64.to_string(),
        permissions: permissions.to_vec(),
        trusted_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        publisher_fingerprint: publisher_fingerprint.to_string(),
    };
    db.add(plugin_id.to_string(), entry);
    db.save()
        .map_err(|e| anyhow::anyhow!("save known-plugins.toml failed: {e}"))?;

    app.plugin_install(std::path::PathBuf::from(src_path))
}

impl App {
    /// Drain pending actions from the plugins modal and apply them to the manager.
    /// Refreshes the modal's snapshot after applying.
    pub(crate) fn process_plugins_window_actions(&mut self) {
        let Some(modal_id) = self.view.active_modal_id else {
            return;
        };
        let Some(modal) = self.view.views.get_mut(&modal_id) else {
            return;
        };
        let Some(plugins_window) = modal.as_any_mut().downcast_mut::<window::PluginsView>() else {
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
        // X 닫기 / Configure 진입점은 모달을 닫는다 (Configure 는 추가로 Settings 오픈).
        let mut close_modal = false;
        let mut open_settings_plugin_tab = false;

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
                plugins_ui::PluginsAction::OpenSettings => {
                    close_modal = true;
                    open_settings_plugin_tab = true;
                }
                plugins_ui::PluginsAction::Close => {
                    close_modal = true;
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
                plugins_ui::PluginsAction::TrustAndInstall {
                    src_path,
                    plugin_id,
                    pubkey_b64,
                    permissions,
                    publisher_fingerprint,
                } => {
                    let toast = match record_trust_then_install(
                        self,
                        &src_path,
                        &plugin_id,
                        &pubkey_b64,
                        &permissions,
                        &publisher_fingerprint,
                    ) {
                        Ok(events) => {
                            let installed = events
                                .iter()
                                .find_map(|ev| match ev {
                                    crate::core::intent::CoreEvent::PluginRegistryChanged {
                                        plugin_id,
                                        ..
                                    } => Some(plugin_id.clone()),
                                    _ => None,
                                })
                                .unwrap_or(plugin_id.clone());
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

        // 모든 lifecycle action 이후 도구 메뉴를 갱신. install/enable/disable/
        // uninstall 어떤 경로든 ui.tool_item 권한 또는 plugin 활성 상태가
        // 바뀌었을 수 있으므로 매번 다시 수집한다 (low-cost).
        self.refresh_tool_registry();

        // Close / Configure: 모달을 닫는다. 단일 모달 불변식상 Settings 를 열려면
        // 먼저 plugins 모달을 닫아야 한다. Configure 는 닫은 뒤 Settings 오픈 이벤트
        // 를 발행하고, open_settings_modal 이 Plugin 탭으로 진입한다.
        if close_modal {
            self.close_active_modal();
            if open_settings_plugin_tab {
                self.pending_settings_plugin_tab = true;
                crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
            }
            return;
        }

        let snapshot = self.snapshot_plugins();
        if let Some(modal) = self.view.views.get_mut(&modal_id)
            && let Some(plugins_window) = modal.as_any_mut().downcast_mut::<window::PluginsView>()
        {
            plugins_window.refresh_snapshot(snapshot);
            for (msg, kind) in pending_toasts {
                plugins_window.push_toast(msg, kind);
            }
        }
    }
}

#[cfg(test)]
mod rfc3339_tests {
    /// 초 정밀도 UTC RFC3339 형식 (`YYYY-MM-DDTHH:MM:SSZ`) 정규식 검증.
    /// `KnownPluginEntry.trusted_at` 가 toml 직렬화/외부 비교를 견디려면
    /// 항상 동일한 모양이어야 한다.
    #[test]
    fn now_rfc3339_is_seconds_z_utc() {
        let s = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        // 예: "2026-06-09T10:54:38Z"
        assert_eq!(s.len(), 20, "unexpected length: {s}");
        assert!(s.ends_with('Z'), "must end with Z: {s}");
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[7], b'-');
        assert_eq!(s.as_bytes()[10], b'T');
        assert_eq!(s.as_bytes()[13], b':');
        assert_eq!(s.as_bytes()[16], b':');
    }
}
