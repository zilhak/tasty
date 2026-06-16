//! Plugins 모달이 표시할 현재 설치된 plugin 들의 snapshot.

use crate::app::App;
use crate::{plugin, plugins_ui};

impl App {
    /// Build a snapshot of currently installed plugins for the plugins modal.
    pub(crate) fn snapshot_plugins(&self) -> plugins_ui::PluginsSnapshot {
        let Some(mgr) = self.plugin_manager.as_ref() else {
            return plugins_ui::PluginsSnapshot::default();
        };
        let plugins = mgr
            .packages
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
                    // spawn 반복 실패로 자동 비활성화된 plugin → error 상태로 표시.
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
                            // 효과 단축키 = override 우선, 없으면 매니페스트 default.
                            // (단축키 하드코딩 금지 — 모두 선언/설정에서 도출.)
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
        plugins_ui::PluginsSnapshot { plugins }
    }
}
