//! Plugin 메타데이터 조회: extension 재집계, tool / popup contribute 평탄 뷰.

use super::{PluginManager, PluginPopupEntry};

impl PluginManager {
    /// spawn 이 윈도우 내 반복 실패하여 자동 비활성화된 plugin 인지.
    /// 자동 비활성화는 수동 `enable` 전까지 더 이상 spawn 을 시도하지 않는
    /// 영구적 error 상태이므로, plugins 창의 error 표시 기준이 된다.
    pub fn is_auto_disabled(&self, plugin_id: &str) -> bool {
        self.auto_disabled.contains(plugin_id)
    }

    /// "확인 필요" plugin 개수 — trust gate 거부분 + enable 상태인데 자동
    /// 비활성화된(health error) plugin. 사이드바 경고 배지 / Attention 탭 카운트가
    /// 쓴다. `snapshot_plugins` 의 attention 목록과 동일 기준으로 센다.
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

    /// Command palette에 노출할 plugin 전역(`CommandScope::Global`) command
    /// 목록. 비활성 plugin은 제외한다 — `command_registry`(`iter_global`) 자체는
    /// disabled 여부와 무관하게 모든 발견된 plugin의 command를 담고 있다(설정 UI가
    /// 비활성 plugin도 단축키를 미리 바인딩할 수 있어야 하므로 의도된 동작,
    /// `discover_and_start` 문서 참고). 하지만 팔레트는 "지금 실행 가능한" 명령만
    /// 보여줘야 하는 실행 UI이므로 `plugin_tool_items`(Tools 메뉴)와 동일하게
    /// `is_disabled` 로 필터링한다. `[[contributes.tool]]`의 `ui.tool_item`과 달리
    /// `[[contributes.commands]]`는 별도 permission 게이트가 없다(매니페스트
    /// validator 참고) — 그래서 permission 검사는 하지 않는다.
    pub fn plugin_palette_commands(&self) -> Vec<crate::command_registry::PluginCommandEntry> {
        self.command_registry
            .iter_global()
            .filter(|e| !self.config.is_disabled(&e.plugin_id))
            .cloned()
            .collect()
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
