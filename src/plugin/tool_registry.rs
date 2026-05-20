//! 사이드바 도구 메뉴에 표시되는 항목 레지스트리.
//!
//! Plugin이 `[[contributes.tool]]`로 선언한 항목을 정렬된 목록으로 관리한다.
//! Clipboard history 등 과거 호스트 빌트인 항목은 모두 builtin plugin
//! (`com.tasty.clipboard-history` 등)으로 이전되었다.
//!
//! 이 모듈은 **렌더링이나 dispatch 정책을 포함하지 않는다** — 단순 데이터 컨테이너.
//! tools_menu UI는 `visible_items()`로 정렬된 목록을 받아 그리고, 클릭 시
//! `ToolAction`을 보고 호스트가 적절히 실행한다.

use super::manifest::ToolAction;

/// 한 도구 메뉴 항목.
#[derive(Debug, Clone)]
pub struct ToolItem {
    pub source: ToolSource,
    /// 항목 전역 식별자. 빌트인은 `"builtin:<name>"`, plugin 항목은
    /// `"<plugin_id>/<tool_id>"`.
    pub key: String,
    /// `t()`에 전달할 i18n 키. 키가 없으면 원본 문자열 fallback.
    pub label_i18n_key: String,
    pub icon: Option<String>,
    pub action: ToolAction,
    /// 작을수록 위. 호스트 빌트인은 0..=99, plugin 항목은 100 이상 권장.
    pub order_hint: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolSource {
    /// Plugin이 `[[contributes.tool]]`로 등록한 항목. 해당 plugin이 비활성/제거되면
    /// 자동 제거.
    Plugin {
        plugin_id: String,
        /// plugin 내부에서의 tool id (매니페스트의 `[[contributes.tool]].id`).
        tool_id: String,
    },
}

/// Plugin 출처 항목을 관리하는 레지스트리.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    plugin: Vec<ToolItem>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { plugin: Vec::new() }
    }

    /// plugin 출처 항목을 통째로 교체한다.
    pub fn set_plugin_items(&mut self, items: Vec<ToolItem>) {
        self.plugin = items;
    }

    /// 정렬된(order_hint asc, 동률 시 key asc) 전체 항목.
    pub fn visible_items(&self) -> Vec<ToolItem> {
        let mut all = self.plugin.clone();
        all.sort_by(|a, b| {
            a.order_hint
                .cmp(&b.order_hint)
                .then_with(|| a.key.cmp(&b.key))
        });
        all
    }

    /// 특정 key에 매칭되는 항목을 1개 반환한다. tools_menu UI 외에 IPC/CLI에서
    /// invoke할 때 식별용.
    pub fn find(&self, key: &str) -> Option<ToolItem> {
        self.plugin.iter().find(|i| i.key == key).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_plugin_item(plugin_id: &str, tool_id: &str, order: i32) -> ToolItem {
        ToolItem {
            source: ToolSource::Plugin {
                plugin_id: plugin_id.into(),
                tool_id: tool_id.into(),
            },
            key: format!("{plugin_id}/{tool_id}"),
            label_i18n_key: format!("{plugin_id}.{tool_id}"),
            icon: None,
            action: ToolAction::Event {
                event_key: format!("{plugin_id}.test"),
            },
            order_hint: order,
        }
    }

    #[test]
    fn empty_registry_has_no_items() {
        let reg = ToolRegistry::new();
        assert!(reg.visible_items().is_empty());
    }

    #[test]
    fn set_plugin_items_replaces_all_entries() {
        let mut reg = ToolRegistry::new();
        reg.set_plugin_items(vec![make_plugin_item("com.example.a", "x", 100)]);
        assert_eq!(reg.visible_items().len(), 1);
        reg.set_plugin_items(vec![]);
        assert!(reg.visible_items().is_empty());
    }

    #[test]
    fn visible_items_sorted_by_order_hint_then_key() {
        let mut reg = ToolRegistry::new();
        reg.set_plugin_items(vec![
            make_plugin_item("com.example.a", "b", 100),
            make_plugin_item("com.example.a", "a", 100),
            make_plugin_item("com.example.b", "c", 50),
        ]);
        let items = reg.visible_items();
        let keys: Vec<_> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec!["com.example.b/c", "com.example.a/a", "com.example.a/b",]
        );
    }

    #[test]
    fn find_returns_item_by_key() {
        let mut reg = ToolRegistry::new();
        reg.set_plugin_items(vec![make_plugin_item("com.example.a", "x", 100)]);
        assert!(reg.find("com.example.a/x").is_some());
        assert!(reg.find("nope").is_none());
    }
}
