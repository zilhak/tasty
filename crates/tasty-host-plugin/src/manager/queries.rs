//! Plugin 메타데이터 조회: extension 재집계, tool / popup contribute 평탄 뷰.

use tasty_ipc::ipc_namespace::IpcNamespaceRegistry;

use super::{PluginManager, PluginPackage, PluginPopupEntry};

const NAMESPACES_WHAT: &str = "the plugin IPC namespace table";
static NAMESPACES_POISON_REPORTED: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

impl PluginManager {
    /// 반복된 기동 실패로 자동 비활성화됐는지 확인한다. 수동 enable로 해제할 수 있다.
    pub fn is_auto_disabled(&self, plugin_id: &str) -> bool {
        self.auto_disabled.contains(plugin_id)
    }

    /// 신뢰 검사를 통과하지 못했거나 자동 비활성화돼 확인이 필요한 플러그인 수.
    pub fn attention_count(&self) -> usize {
        let health = self
            .packages
            .iter()
            .filter(|p| {
                self.is_auto_disabled(&p.manifest.id) && !self.config.is_disabled(&p.manifest.id)
            })
            .count();
        self.rejected.len() + health
    }

    /// 설치 패키지의 읽기 전용 목록. 변경할 때는 refresh_packages를 거쳐
    /// namespace 소유자와 확장 상태도 함께 갱신해야 한다.
    pub fn packages(&self) -> &[PluginPackage] {
        &self.packages
    }

    /// 이 메서드의 namespace를 소유한 플러그인이 있는지 확인한다. 실행 여부는 별개다.
    pub fn owns_namespace(&self, method: &str) -> bool {
        self.namespaces_read().resolve(method).is_some()
    }

    /// 자기 자신이 아닌 다른 플러그인의 namespace인지 한 번의 잠금으로 확인한다.
    pub fn namespace_belongs_to_other(&self, method: &str, plugin_id: &str) -> bool {
        self.namespaces_read()
            .resolve(method)
            .is_some_and(|owner| owner != plugin_id)
    }

    /// IPC 메서드 조회에 같은 namespace 표를 공유한다. 한 번만 설치할 수 있다.
    /// 이미 다른 표가 설치돼 있으면 이 매니저의 갱신은 조회에 반영되지 않으므로 경고한다.
    pub fn install_namespace_table_once(&self) {
        if !tasty_ipc::method_meta::install_namespace_table(std::sync::Arc::clone(
            &self.ipc_namespaces,
        )) {
            tracing::warn!(
                "namespace 소유 표가 이미 설치돼 있어 이 매니저의 표를 IPC 조회에 사용하지 않는다"
            );
        }
    }

    /// 잠금이 poison 상태여도 기존 namespace 표를 재사용한다.
    pub(crate) fn namespaces_read(&self) -> std::sync::RwLockReadGuard<'_, IpcNamespaceRegistry> {
        tasty_utils::poison::recover_read(
            self.ipc_namespaces.read(),
            NAMESPACES_WHAT,
            &NAMESPACES_POISON_REPORTED,
        )
    }

    /// 쓰기 잠금도 poison 상태에서 기존 표를 재사용한다.
    pub(crate) fn namespaces_write(&self) -> std::sync::RwLockWriteGuard<'_, IpcNamespaceRegistry> {
        tasty_utils::poison::recover_write(
            self.ipc_namespaces.write(),
            NAMESPACES_WHAT,
            &NAMESPACES_POISON_REPORTED,
        )
    }

    /// 설치 목록과 설정으로 계산한 확장 상태를 조회한다. 직접 변경할 수는 없다.
    pub fn extension_state(
        &self,
        extension_id: &str,
    ) -> Option<&crate::extension_registry::ExtensionState> {
        self.extensions.state(extension_id)
    }

    /// 전체 확장 상태를 읽는다.
    pub fn extensions_iter(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::extension_registry::ExtensionState)> {
        self.extensions.iter()
    }

    pub fn recompute_extensions(&mut self) {
        let fresh = self.freshly_computed_extensions();
        self.extensions = fresh;
    }

    /// 설치 목록과 설정으로 확장 상태를 계산한다. 갱신과 debug 검사가 같은 계산을 쓴다.
    fn freshly_computed_extensions(&self) -> super::super::extension_registry::ExtensionRegistry {
        let manifests: Vec<&tasty_plugin_manifest::Manifest> =
            self.packages.iter().map(|p| &p.manifest).collect();
        let cfg = &self.config;
        let mut out = self.extensions.clone();
        out.recompute(
            &manifests,
            &|id| cfg.is_disabled(id),
            &|ext_id, target_id| {
                let token = format!("ext:{target_id}");
                cfg.granted_permissions(ext_id).contains(&token)
            },
        );
        out
    }

    /// 설치 목록·설정을 마지막으로 바꾼 뒤 확장 상태도 갱신했는지 debug 빌드에서 확인한다.
    pub fn debug_assert_extensions_fresh(&self) {
        #[cfg(debug_assertions)]
        {
            let fresh = self.freshly_computed_extensions();
            assert!(
                fresh == self.extensions,
                "확장 집합이 낡았다 — packages 또는 config의 마지막 변경 뒤 recompute_extensions를 호출해야 한다"
            );
        }
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
            if !granted.contains("ui.tool_item") {
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

    /// 팔레트에 표시할 전역 명령. 단축키 설정과 달리 비활성 플러그인은 제외한다.
    /// contributes.commands에는 ui.tool_item 같은 별도 권한이 없다.
    pub fn plugin_palette_commands(&self) -> Vec<crate::command_registry::PluginCommandEntry> {
        self.command_registry
            .iter_global()
            .filter(|e| !self.config.is_disabled(&e.plugin_id))
            .cloned()
            .collect()
    }

    /// 활성 상태이며 ui.popup 권한이 있는 플러그인의 팝업 선언을 모은다.
    pub fn plugin_popup_contributes(&self) -> Vec<PluginPopupEntry> {
        let mut out = Vec::new();
        for pkg in &self.packages {
            if self.config.is_disabled(&pkg.manifest.id) {
                continue;
            }
            let granted = self.config.granted_permissions(&pkg.manifest.id);
            if !granted.contains("ui.popup") {
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

#[cfg(test)]
mod palette_commands_tests {
    use super::*;
    use tasty_plugin_manifest::{
        BindingMode, CommandDecl, CommandScope, Contributes, Entry, Manifest,
    };

    fn mgr() -> PluginManager {
        PluginManager::new(std::sync::Arc::new(
            tasty_terminal::waker_factory::NoopWakerFactory,
        ))
    }

    fn manifest_with_commands(id: &str, cmds: Vec<CommandDecl>) -> Manifest {
        Manifest {
            manifest_version: 1,
            id: id.to_string(),
            name: id.to_string(),
            version: "0.1".to_string(),
            authors: vec![],
            description: String::new(),
            homepage: String::new(),
            api_version: "1".to_string(),
            entry: Entry::Process {
                command: "x".to_string(),
                args: vec![],
            },
            surface_kinds: vec![],
            permissions: vec![],
            event_subscribe: vec![],
            event_publish: vec![],
            events_emitted: vec![],
            contributes: Contributes {
                commands: cmds,
                ..Default::default()
            },
            extends: None,
            lang_dir: "lang".to_string(),
            bundle: true,
        }
    }

    fn cmd(id: &str, scope: CommandScope) -> CommandDecl {
        CommandDecl {
            id: id.to_string(),
            title_i18n_key: format!("{id}.title"),
            default_keybinding: None,
            binding_mode: BindingMode::Independent,
            scope,
            action: None,
        }
    }

    #[test]
    fn excludes_disabled_plugin_commands() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd("a.open", CommandScope::Global)],
        ));
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.b",
            vec![cmd("b.open", CommandScope::Global)],
        ));
        m.config.disable("com.example.b");

        let commands = m.plugin_palette_commands();
        let ids: Vec<&str> = commands.iter().map(|e| e.command_id.as_str()).collect();
        assert_eq!(ids, vec!["a.open"]);
    }

    #[test]
    fn excludes_surface_scope_commands() {
        let mut m = mgr();
        m.command_registry.register_plugin(&manifest_with_commands(
            "com.example.a",
            vec![cmd("a.surface_only", CommandScope::Surface)],
        ));
        assert!(m.plugin_palette_commands().is_empty());
    }

    #[test]
    fn empty_registry_yields_empty_list() {
        let m = mgr();
        assert!(m.plugin_palette_commands().is_empty());
    }
}
