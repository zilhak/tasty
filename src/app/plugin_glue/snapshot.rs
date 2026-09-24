//! Plugins 모달에 설치 목록과 확인이 필요한 플러그인을 표시한다.

use crate::app::App;
use crate::{plugin, plugins_ui};

impl App {
    pub(crate) fn snapshot_plugins(&self) -> plugins_ui::PluginsSnapshot {
        let Some(mgr) = self.plugin_manager.as_ref() else {
            return plugins_ui::PluginsSnapshot::default();
        };
        let plugins = mgr
            .packages()
            .iter()
            .map(|pkg| {
                let id = &pkg.manifest.id;
                plugins_ui::PluginEntry {
                    id: id.clone(),
                    name: pkg.manifest.name.clone(),
                    version: pkg.manifest.version.clone(),
                    description: pkg.manifest.description.clone(),
                    authors: pkg.manifest.authors.clone(),
                    homepage: pkg.manifest.homepage.clone(),
                    enabled: !mgr.config.is_disabled(id),
                    running: mgr.is_running(id),
                    health_error: mgr.is_auto_disabled(id),
                    builtin: plugin::is_builtin_plugin(id),
                    surface_kinds: pkg
                        .manifest
                        .surface_kinds
                        .iter()
                        .map(|k| k.kind.clone())
                        .collect(),
                    manifest_permissions: pkg.manifest.permissions.clone(),
                    commands: pkg
                        .manifest
                        .contributes
                        .commands
                        .iter()
                        .map(|cmd| {
                            let keybinding = match mgr.config.shortcut_override(id, &cmd.id) {
                                Some(ov) => {
                                    crate::plugin::registry_state::shortcut_override_display(Some(
                                        ov,
                                    ))
                                }
                                None => cmd.default_keybinding.clone(),
                            };
                            plugins_ui::PluginCommandEntry {
                                title_key: cmd.title_i18n_key.clone(),
                                keybinding,
                            }
                        })
                        .collect(),
                    log_path: mgr.log_path(id).to_string_lossy().into_owned(),
                    install_dir: pkg.dir.to_string_lossy().into_owned(),
                }
            })
            .collect();

        // 자동 비활성화된 플러그인은 설치 목록과 확인 필요 목록에 함께 표시한다.
        use crate::plugin::discovery::RejectionReason;
        let mut attention: Vec<plugins_ui::AttentionEntry> = mgr
            .rejected
            .iter()
            .map(|r| plugins_ui::AttentionEntry {
                id: r.id.clone(),
                name: r.name.clone(),
                version: r.version.clone(),
                authors: r.authors.clone(),
                builtin: r.builtin,
                kind: match r.reason {
                    RejectionReason::UnknownKey => plugins_ui::AttentionKind::UnknownKey,
                    RejectionReason::SignatureInvalid => {
                        plugins_ui::AttentionKind::SignatureInvalid
                    }
                    RejectionReason::PermissionsChanged => {
                        plugins_ui::AttentionKind::PermissionsChanged
                    }
                },
                fingerprint: r.fingerprint.clone(),
                permissions_added: r.permissions_added.clone(),
                permissions_removed: r.permissions_removed.clone(),
                health_detail: None,
            })
            .collect();
        attention.extend(mgr.packages().iter().filter_map(|pkg| {
            let id = &pkg.manifest.id;
            // 사용자가 직접 끈 플러그인은 오류로 표시하지 않는다.
            if !mgr.is_auto_disabled(id) || mgr.config.is_disabled(id) {
                return None;
            }
            Some(plugins_ui::AttentionEntry {
                id: id.clone(),
                name: pkg.manifest.name.clone(),
                version: pkg.manifest.version.clone(),
                authors: pkg.manifest.authors.clone(),
                builtin: plugin::is_builtin_plugin(id),
                kind: plugins_ui::AttentionKind::HealthError,
                fingerprint: None,
                permissions_added: Vec::new(),
                permissions_removed: Vec::new(),
                health_detail: Some(mgr.log_path(id).to_string_lossy().into_owned()),
            })
        }));

        plugins_ui::PluginsSnapshot { plugins, attention }
    }
}
