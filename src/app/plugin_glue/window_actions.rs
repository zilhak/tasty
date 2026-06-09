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
        trusted_at: current_rfc3339(),
        publisher_fingerprint: publisher_fingerprint.to_string(),
    };
    db.add(plugin_id.to_string(), entry);
    db.save()
        .map_err(|e| anyhow::anyhow!("save known-plugins.toml failed: {e}"))?;

    app.plugin_install(std::path::PathBuf::from(src_path))
}

/// `chrono` 같은 추가 deps 없이 std::time 으로 RFC3339 UTC 타임스탬프 생성.
/// 정밀도는 초 단위 — trust 시점 식별용으로 충분.
fn current_rfc3339() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    // Days from civil date — Howard Hinnant's algorithm. 1970-01-01 = epoch.
    let days = secs.div_euclid(86_400);
    let time_of_day = secs.rem_euclid(86_400);
    let hour = (time_of_day / 3600) as u32;
    let minute = ((time_of_day % 3600) / 60) as u32;
    let second = (time_of_day % 60) as u32;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}T{hour:02}:{minute:02}:{second:02}Z")
}

/// Howard Hinnant `days_from_civil` 역함수 — epoch days → (year, month, day).
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = (y + if m <= 2 { 1 } else { 0 }) as i32;
    (y, m, d)
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

        // 모든 lifecycle action 이후 도구 메뉴를 갱신. install/enable/disable/grant/
        // revoke/uninstall 어떤 경로든 ui.tool_item 권한 또는 plugin 활성 상태가
        // 바뀌었을 수 있으므로 매번 다시 수집한다 (low-cost).
        self.refresh_tool_registry();

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
