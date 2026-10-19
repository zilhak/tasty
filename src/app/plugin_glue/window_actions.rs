//! Plugins 모달에서 요청한 작업을 실행한다.

use crate::app::App;
use crate::{plugin, plugins_ui, window};

fn extract_installed_plugin_id(events: &[crate::app::command::CoreEvent]) -> Option<String> {
    events.iter().find_map(|ev| match ev {
        crate::app::command::CoreEvent::PluginRegistryChanged { plugin_id, .. } => {
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

    /// 사용자가 누른 열기가 실패하면 이 창에 띄울 실패 토스트를 돌려준다. 성공은 알리지 않는다.
    fn handle_open_uri(
        &self,
        uri: &str,
        failure_key: &str,
    ) -> Option<(String, crate::adapters::ui::ToastKind)> {
        let opened = crate::terminal_link::open_uri(uri);
        if !opened {
            tracing::warn!("plugins modal: open failed: {uri}");
        }
        crate::terminal_link::open_failure_message(opened, failure_key, uri)
            .map(|message| (message, crate::adapters::ui::ToastKind::Error))
    }

    /// 설치 결과 토스트와 성공 여부. 실패 토스트는 실제 원인을 그대로 보인다.
    fn handle_install(
        &mut self,
        src_path: &str,
        choice: &super::install_trust::InstallChoice,
        reviewed: &str,
    ) -> ((String, crate::adapters::ui::ToastKind), bool) {
        match self.plugin_install(std::path::PathBuf::from(src_path), choice, Some(reviewed)) {
            Ok(events) => {
                let installed = extract_installed_plugin_id(&events).unwrap_or_default();
                self.cascade_plugin_events(events);
                (
                    (
                        crate::i18n::t_fmt("plugins.add_installed", &installed),
                        crate::adapters::ui::ToastKind::Success,
                    ),
                    true,
                )
            }
            Err(e) => {
                tracing::warn!("plugins modal: install({src_path}) failed: {e}");
                (
                    (
                        crate::i18n::t_fmt("plugins.add_install_failed", &e.to_string()),
                        crate::adapters::ui::ToastKind::Error,
                    ),
                    false,
                )
            }
        }
    }

    /// 확인 필요 탭의 "그래도 등록" 결과 토스트. 실패 토스트는 실제 원인을 보인다.
    fn handle_register_unverified(
        &mut self,
        id: &str,
        reviewed: &str,
    ) -> (String, crate::adapters::ui::ToastKind) {
        match self.plugin_register_unverified(id, reviewed) {
            Ok(events) => {
                self.cascade_plugin_events(events);
                (
                    crate::i18n::t_fmt("plugins.attn_registered", id),
                    crate::adapters::ui::ToastKind::Success,
                )
            }
            Err(e) => {
                tracing::warn!("plugins modal: register_unverified({id}) failed: {e}");
                (
                    crate::i18n::t_fmt("plugins.attn_register_failed", &e.to_string()),
                    crate::adapters::ui::ToastKind::Error,
                )
            }
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
        let Some(modal_id) = self.view.active_modal_id() else {
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
        // Add 탭의 추가가 끝났는지. 성공하면 입력 화면으로, 실패하면 판정을 남긴 채 돌아간다.
        let mut add_result: Option<bool> = None;

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
                    pending_toasts.extend(self.handle_open_uri(&path, "toast.open_folder_failed"));
                }
                plugins_ui::PluginsAction::OpenHomepage { url } => {
                    pending_toasts.extend(self.handle_open_uri(&url, "toast.open_link_failed"));
                }
                plugins_ui::PluginsAction::Install {
                    src_path,
                    manifest_sha256,
                } => {
                    let (toast, ok) = self.handle_install(
                        &src_path,
                        &super::install_trust::InstallChoice::VerifiedOnly,
                        &manifest_sha256,
                    );
                    pending_toasts.push(toast);
                    add_result = Some(ok);
                }
                plugins_ui::PluginsAction::InstallUnverified {
                    src_path,
                    manifest_sha256,
                } => {
                    let (toast, ok) = self.handle_install(
                        &src_path,
                        &super::install_trust::InstallChoice::AllowUnverified,
                        &manifest_sha256,
                    );
                    pending_toasts.push(toast);
                    add_result = Some(ok);
                }
                plugins_ui::PluginsAction::RegisterUnverified {
                    id,
                    manifest_sha256,
                } => {
                    pending_toasts.push(self.handle_register_unverified(&id, &manifest_sha256));
                }
                plugins_ui::PluginsAction::TrustAndInstall {
                    src_path,
                    pubkey_b64,
                    manifest_sha256,
                } => {
                    let (toast, ok) = self.handle_install(
                        &src_path,
                        &super::install_trust::InstallChoice::TrustKey { pubkey_b64 },
                        &manifest_sha256,
                    );
                    pending_toasts.push(toast);
                    add_result = Some(ok);
                }
            }
        }

        self.refresh_tool_registry();
        self.refresh_palette_plugin_commands();

        // 한 번에 하나의 모달만 열 수 있어 Plugins를 먼저 닫은 뒤 Settings를 요청한다.
        if close_modal {
            self.close_active_modal();
            if open_settings_plugin_tab {
                self.state.pending_settings_plugin_tab = true;
                crate::shortcuts::send_app_event(&self.view.proxy, crate::AppEvent::OpenSettings);
            }
            return;
        }

        let snapshot = self.snapshot_plugins();
        if let Some(modal) = self.view.views.get_mut(&modal_id)
            && let Some(plugins_window) = modal.as_any_mut().downcast_mut::<window::PluginsView>()
        {
            plugins_window.refresh_snapshot(snapshot);
            if let Some(ok) = add_result {
                plugins_window.finish_add(ok);
            }
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
