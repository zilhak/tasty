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
                let granted: Vec<String> = mgr.config.granted_permissions(id).into_iter().collect();
                plugins_ui::PluginEntry {
                    id: id.clone(),
                    name: pkg.manifest.name.clone(),
                    version: pkg.manifest.version.clone(),
                    description: pkg.manifest.description.clone(),
                    authors: pkg.manifest.authors.clone(),
                    homepage: pkg.manifest.homepage.clone(),
                    enabled: !mgr.config.is_disabled(id),
                    running: mgr.is_running(id),
                    builtin: plugin::is_builtin_plugin(id),
                    surface_kinds: pkg
                        .manifest
                        .surface_kinds
                        .iter()
                        .map(|k| k.kind.clone())
                        .collect(),
                    manifest_permissions: pkg.manifest.permissions.clone(),
                    granted_permissions: granted,
                    log_path: mgr.log_path(id).to_string_lossy().into_owned(),
                    install_dir: pkg.dir.to_string_lossy().into_owned(),
                }
            })
            .collect();
        plugins_ui::PluginsSnapshot { plugins }
    }
}
