//! Plugin 메타데이터 조회: extension 재집계, tool / popup contribute 평탄 뷰.

use super::{PluginManager, PluginPopupEntry};

impl PluginManager {
    pub fn recompute_extensions(&mut self) {
        let manifests: Vec<&tasty_plugin_manifest::Manifest> =
            self.packages.iter().map(|p| &p.manifest).collect();
        let cfg = &self.config;
        self.extensions.recompute(
            &manifests,
            &|id| cfg.is_disabled(id),
            &|ext_id, target_id| {
                let token = format!("ext:{target_id}");
                cfg.granted_permissions(ext_id).contains(&token)
            },
        );
    }

    pub fn plugin_tool_items(&self) -> Vec<crate::tool_registry::ToolItem> {
        use crate::tool_registry::{ToolItem, ToolSource};
        let mut out = Vec::new();
        for pkg in &self.packages {
            if self.config.is_disabled(&pkg.manifest.id) {
                continue;
            }
            // ui.tool_item 권한이 grant되어야 메뉴에 노출.
            let granted = self.config.granted_permissions(&pkg.manifest.id);
            if !granted.contains(&"ui.tool_item".to_string()) {
                continue;
            }
            for tool in &pkg.manifest.contributes.tool {
                out.push(ToolItem {
                    source: ToolSource::Plugin {
                        plugin_id: pkg.manifest.id.clone(),
                        tool_id: tool.id.clone(),
                    },
                    key: format!("{}/{}", pkg.manifest.id, tool.id),
                    label_i18n_key: tool.label_i18n_key.clone(),
                    icon: tool.icon.clone(),
                    action: tool.action.clone(),
                    order_hint: tool.order_hint,
                });
            }
        }
        out
    }

    /// `[[contributes.popup]]` 항목을 활성 + `ui.popup` grant된 plugin에서만
    /// 수집해 반환한다. 호스트의 popup 라우터(PR 4)가 trigger 매칭과 IPC 라우팅에
    /// 사용한다.
    pub fn plugin_popup_contributes(&self) -> Vec<PluginPopupEntry> {
        let mut out = Vec::new();
        for pkg in &self.packages {
            if self.config.is_disabled(&pkg.manifest.id) {
                continue;
            }
            let granted = self.config.granted_permissions(&pkg.manifest.id);
            if !granted.contains(&"ui.popup".to_string()) {
                continue;
            }
            for popup in &pkg.manifest.contributes.popup {
                out.push(PluginPopupEntry {
                    plugin_id: pkg.manifest.id.clone(),
                    contribute: popup.clone(),
                });
            }
        }
        out
    }
}
