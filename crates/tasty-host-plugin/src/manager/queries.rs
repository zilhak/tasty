//! Plugin 메타데이터 조회: extension 재집계, tool / popup contribute 평탄 뷰.

use super::{PluginManager, PluginPackage, PluginPopupEntry};

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

    /// 설치된 plugin 패키지 목록 — 읽기 전용 창구.
    ///
    /// 이 값은 **디스크에서 재발견되는 원본**이고, 여기서 `ipc_namespaces` ·
    /// `extensions` 가 유도된다. 밖에서 목록만 바꾸면 그 유도들이 낡으므로
    /// (실제로 `plugin.remove` 가 그렇게 해서 지운 plugin 의 prefix 가 소유 표에
    /// 남았다) 쓰기는 크레이트 밖으로 열지 않는다. 바꾸려면
    /// [`PluginManager::refresh_packages`] 를 거친다.
    pub fn packages(&self) -> &[PluginPackage] {
        &self.packages
    }

    /// 이 메서드 이름의 namespace 를 가진 plugin id — 없으면 host 것이다.
    ///
    /// 소유 표(`ipc_namespaces`)는 **설치된 매니페스트에서 유도되는 값**이라 밖에서
    /// 바뀌면 낡는다(ADR-0173). 그래서 표 자체는 크레이트 밖으로 안 열고, 밖이
    /// 실제로 묻는 것 하나만 창구로 낸다 — 이 물음은 **소유**만 답한다. "지금 떠
    /// 있는가" 는 다른 물음이고 `is_running` 이 답한다.
    pub fn namespace_owner(&self, method: &str) -> Option<&str> {
        self.ipc_namespaces.resolve(method)
    }

    /// 한 extension 의 상태 — 밖에서 `extensions` 를 직접 만지지 않게 하는 읽기 창구.
    ///
    /// 이 필드는 packages + config 에서 **유도되는 값**이라 밖에서 바뀌면 낡는다.
    /// 그래서 필드를 크레이트 밖으로 열지 않고, 읽기만 이렇게 내보낸다 — 잊을 수 있는
    /// 규율을 가드로 지키는 것보다 애초에 못 하게 하는 쪽이 싸다.
    pub fn extension_state(
        &self,
        extension_id: &str,
    ) -> Option<&crate::extension_registry::ExtensionState> {
        self.extensions.state(extension_id)
    }

    /// 전체 extension 상태 순회. [`Self::extension_state`] 와 같은 이유로 존재한다.
    pub fn extensions_iter(
        &self,
    ) -> impl Iterator<Item = (&str, &crate::extension_registry::ExtensionState)> {
        self.extensions.iter()
    }

    pub fn recompute_extensions(&mut self) {
        let fresh = self.freshly_computed_extensions();
        self.extensions = fresh;
    }

    /// 지금의 packages + config 로 확장 집합을 **처음부터** 계산한다.
    ///
    /// `recompute_extensions` 와 아래 디버그 단정이 같은 계산을 쓰게 하려고 뽑았다 —
    /// 둘이 갈라지면 단정이 자기 사본을 상대로 성립해 아무것도 안 지킨다.
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

    /// 유도된 확장 집합이 **지금의 원본과 일치하는가** — debug 빌드에서만 판정한다.
    ///
    /// 텍스트 가드는 "유도를 안 불렀다" 는 잡지만 "불렀는데 원본이 그 뒤에 또
    /// 바뀌었다" 는 못 잡는다(흐름 판정이라 줄 단위 스캔의 범위 밖이다). 그 부류를
    /// 실행 시점으로 옮긴다 — lifecycle 조작 끝에서 부르면, 순서가 어긋난 그 자리에서
    /// 바로 터진다. release 에서는 본문이 통째로 사라져 비용이 0 이다.
    ///
    /// 실제로 있었다: `plugin_install` 이 `recompute_extensions()` 를
    /// `config.set_granted()` **앞**에서 불렀고, 뒤따르는 `enable` 이 한 번 더
    /// 계산하는 덕에 가려져 있었다. 그 `enable` 은 `is_disabled` 분기에서 건너뛴다.
    pub fn debug_assert_extensions_fresh(&self) {
        #[cfg(debug_assertions)]
        {
            let fresh = self.freshly_computed_extensions();
            assert!(
                fresh == self.extensions,
                "확장 집합이 낡았다 — 유도(`recompute_extensions`) 뒤에 원본(packages 또는 \
                 config)이 또 바뀌었다. 유도를 원본의 마지막 쓰기 뒤로 옮겨라"
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
