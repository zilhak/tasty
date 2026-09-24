//! Plugins 모달에서 요청한 작업을 실행한다.

use crate::app::App;
use crate::{plugin, plugins_ui, window};

/// 신뢰 정보 저장이 실패하면 설치하지 않는다. 신뢰 기록 없이 플러그인 파일만 남지 않게 한다.
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

fn extract_installed_plugin_id(events: &[crate::core::intent::CoreEvent]) -> Option<String> {
    events.iter().find_map(|ev| match ev {
        crate::core::intent::CoreEvent::PluginRegistryChanged { plugin_id, .. } => {
            Some(plugin_id.clone())
        }
        _ => None,
    })
}

impl App {
    /// 활성 상태 변경 실패는 toast 없이 로그로만 알린다.
    fn handle_set_enabled(&mut self, id: String, enabled: bool) {
        let result = if enabled {
            self.plugin_enable(id.clone())
        } else {
            self.plugin_disable(id.clone())
        };
        match result {
            Ok(events) => self.cascade_plugin_events(events),
            Err(e) => {
                tracing::warn!("plugins modal: set_enabled({id}, {enabled}) failed: {e}")
            }
        }
    }

    fn handle_uninstall(&mut self, id: String) {
        match self.plugin_remove(id.clone()) {
            Ok(events) => {
                self.cascade_plugin_events(events);
            }
            Err(e) => {
                tracing::warn!("plugins modal: uninstall({id}) failed: {e}");
            }
        }
    }

    fn handle_reapprove(&mut self, id: String) -> (String, crate::adapters::ui::ToastKind) {
        match self.reapprove_plugin(&id) {
            Ok(()) => (
                crate::i18n::t_fmt("plugins.attn_reapproved", &id),
                crate::adapters::ui::ToastKind::Success,
            ),
            Err(e) => {
                tracing::warn!("plugins modal: reapprove({id}) failed: {e}");
                (
                    crate::i18n::t_fmt("plugins.attn_reapprove_failed", &e.to_string()),
                    crate::adapters::ui::ToastKind::Error,
                )
            }
        }
    }

    fn handle_open_install_dir(&self, path: &str) {
        if !crate::terminal_link::open_uri(path) {
            tracing::warn!("plugins modal: open install dir failed: {path}");
        }
    }

    fn handle_install(&mut self, src_path: &str) -> (String, crate::adapters::ui::ToastKind) {
        match self.plugin_install(std::path::PathBuf::from(src_path)) {
            Ok(events) => {
                let installed = extract_installed_plugin_id(&events).unwrap_or_default();
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
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn handle_trust_and_install(
        &mut self,
        src_path: &str,
        plugin_id: &str,
        pubkey_b64: &str,
        permissions: &[String],
        publisher_fingerprint: &str,
    ) -> (String, crate::adapters::ui::ToastKind) {
        match record_trust_then_install(
            self,
            src_path,
            plugin_id,
            pubkey_b64,
            permissions,
            publisher_fingerprint,
        ) {
            Ok(events) => {
                let installed =
                    extract_installed_plugin_id(&events).unwrap_or_else(|| plugin_id.to_string());
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
        }
    }

    /// 기존 신뢰 키는 유지하고 현재 매니페스트의 권한을 다시 승인한다.
    /// 저장 후 재검색·enable을 시도하며, 이 함수가 서명 유효성을 보장하는 것은 아니다.
    fn reapprove_plugin(&mut self, id: &str) -> anyhow::Result<()> {
        use tasty_host_plugin::known_plugins::{KnownPluginEntry, KnownPlugins};

        let dir = plugin::plugin_root()
            .ok_or_else(|| anyhow::anyhow!("plugin root unresolved"))?
            .join(id);
        let manifest = plugin::Manifest::load(&dir)
            .map_err(|e| anyhow::anyhow!("load manifest failed: {e}"))?;

        let mut db = KnownPlugins::load()
            .map_err(|e| anyhow::anyhow!("load known-plugins.toml failed: {e}"))?;
        let prev = db
            .lookup(id)
            .ok_or_else(|| anyhow::anyhow!("no known-plugins entry for {id}"))?;
        let entry = KnownPluginEntry {
            pubkey: prev.pubkey.clone(),
            permissions: manifest.permissions.clone(),
            trusted_at: chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
            publisher_fingerprint: prev.publisher_fingerprint.clone(),
        };
        db.add(id.to_string(), entry);
        db.save()
            .map_err(|e| anyhow::anyhow!("save known-plugins.toml failed: {e}"))?;

        let mgr = self
            .plugin_manager
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("no plugin manager"))?;
        mgr.config.set_granted(id, manifest.permissions.clone());
        if let Err(e) = mgr.config.save() {
            tracing::warn!("reapprove: plugins config save failed: {e}");
        }
        mgr.refresh_packages();
        mgr.command_registry.register_plugin(&manifest);
        mgr.enable(id)?;
        Ok(())
    }

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
        let mut close_modal = false;
        let mut open_settings_plugin_tab = false;

        for action in actions {
            match action {
                plugins_ui::PluginsAction::SetEnabled { id, enabled } => {
                    self.handle_set_enabled(id, enabled);
                }
                plugins_ui::PluginsAction::Uninstall { id } => {
                    self.handle_uninstall(id);
                }
                plugins_ui::PluginsAction::OpenSettings => {
                    close_modal = true;
                    open_settings_plugin_tab = true;
                }
                plugins_ui::PluginsAction::Reapprove { id } => {
                    pending_toasts.push(self.handle_reapprove(id));
                }
                plugins_ui::PluginsAction::Close => {
                    close_modal = true;
                }
                plugins_ui::PluginsAction::OpenInstallDir { path } => {
                    self.handle_open_install_dir(&path);
                }
                plugins_ui::PluginsAction::Install { src_path } => {
                    pending_toasts.push(self.handle_install(&src_path));
                }
                plugins_ui::PluginsAction::TrustAndInstall {
                    src_path,
                    plugin_id,
                    pubkey_b64,
                    permissions,
                    publisher_fingerprint,
                } => {
                    pending_toasts.push(self.handle_trust_and_install(
                        &src_path,
                        &plugin_id,
                        &pubkey_b64,
                        &permissions,
                        &publisher_fingerprint,
                    ));
                }
            }
        }

        self.refresh_tool_registry();
        self.refresh_palette_plugin_commands();

        // 한 번에 하나의 모달만 열 수 있어 Plugins를 먼저 닫은 뒤 Settings를 요청한다.
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
    /// UTC 초 단위 형식의 길이·접미사·구분자 위치를 확인한다.
    #[test]
    fn now_rfc3339_is_seconds_z_utc() {
        let s = chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true);
        assert_eq!(s.len(), 20, "unexpected length: {s}");
        assert!(s.ends_with('Z'), "must end with Z: {s}");
        assert_eq!(s.as_bytes()[4], b'-');
        assert_eq!(s.as_bytes()[7], b'-');
        assert_eq!(s.as_bytes()[10], b'T');
        assert_eq!(s.as_bytes()[13], b':');
        assert_eq!(s.as_bytes()[16], b':');
    }
}
