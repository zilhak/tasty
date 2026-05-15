//! 사이드바 도구 메뉴에 표시되는 항목 레지스트리.
//!
//! 호스트 빌트인 항목(예: Clipboard History)과 plugin이 `[[contributes.tool]]`로
//! 선언한 항목을 하나의 정렬된 목록으로 관리한다. plugin 활성/비활성 시 호스트가
//! `set_plugin_items`로 plugin 항목만 갈아끼우면 빌트인은 유지된다.
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
    /// 호스트 빌트인 항목. 빌드 시 컴파일되며 plugin 비활성과 무관하게 항상 보임.
    Builtin,
    /// Plugin이 `[[contributes.tool]]`로 등록한 항목. 해당 plugin이 비활성/제거되면
    /// 자동 제거.
    Plugin {
        plugin_id: String,
        /// plugin 내부에서의 tool id (매니페스트의 `[[contributes.tool]].id`).
        tool_id: String,
    },
}

/// 두 출처의 항목을 합쳐 관리하는 레지스트리.
#[derive(Debug, Default)]
pub struct ToolRegistry {
    builtin: Vec<ToolItem>,
    plugin: Vec<ToolItem>,
}

impl ToolRegistry {
    /// 빌트인 항목만 초기화된 새 레지스트리. plugin 항목은 매니저 init 후
    /// `set_plugin_items`로 채워진다.
    pub fn with_builtins() -> Self {
        Self {
            builtin: builtin_items(),
            plugin: Vec::new(),
        }
    }

    /// plugin 출처 항목을 통째로 교체한다. 빌트인은 보존.
    pub fn set_plugin_items(&mut self, items: Vec<ToolItem>) {
        self.plugin = items;
    }

    /// 정렬된(order_hint asc, 동률 시 key asc) 전체 항목.
    pub fn visible_items(&self) -> Vec<ToolItem> {
        let mut all = Vec::with_capacity(self.builtin.len() + self.plugin.len());
        all.extend(self.builtin.iter().cloned());
        all.extend(self.plugin.iter().cloned());
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
        self.builtin
            .iter()
            .chain(self.plugin.iter())
            .find(|i| i.key == key)
            .cloned()
    }
}

/// 호스트가 제공하는 빌트인 항목 목록.
///
/// 현재 1개: Clipboard History (단축키 `Ctrl+Shift+H`와 동일한 popup 트리거).
/// builtin 항목의 action은 `OpenPopup { popup_id: "<builtin>:<id>" }` 규약으로
/// 적어두고, tools_menu invoke 측에서 prefix 매칭으로 분기한다.
fn builtin_items() -> Vec<ToolItem> {
    vec![ToolItem {
        source: ToolSource::Builtin,
        key: "builtin:clipboard_history".into(),
        label_i18n_key: "tools_menu.clipboard_history".into(),
        icon: None,
        action: ToolAction::OpenPopup {
            popup_id: "builtin:clipboard_history".into(),
        },
        order_hint: 0,
    }]
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
    fn with_builtins_includes_clipboard_history() {
        let reg = ToolRegistry::with_builtins();
        let items = reg.visible_items();
        assert!(items.iter().any(|i| i.key == "builtin:clipboard_history"));
    }

    #[test]
    fn set_plugin_items_replaces_only_plugin_entries() {
        let mut reg = ToolRegistry::with_builtins();
        reg.set_plugin_items(vec![make_plugin_item("com.example.a", "x", 100)]);
        let after = reg.visible_items();
        assert_eq!(after.len(), 2);
        // 다시 갈아끼우면 plugin 항목만 교체되고 빌트인은 유지.
        reg.set_plugin_items(vec![]);
        let cleared = reg.visible_items();
        assert_eq!(cleared.len(), 1);
        assert_eq!(cleared[0].key, "builtin:clipboard_history");
    }

    #[test]
    fn visible_items_sorted_by_order_hint_then_key() {
        let mut reg = ToolRegistry::with_builtins();
        reg.set_plugin_items(vec![
            make_plugin_item("com.example.a", "b", 100),
            make_plugin_item("com.example.a", "a", 100),
            make_plugin_item("com.example.b", "c", 50),
        ]);
        let items = reg.visible_items();
        let keys: Vec<_> = items.iter().map(|i| i.key.as_str()).collect();
        assert_eq!(
            keys,
            vec![
                "builtin:clipboard_history",
                "com.example.b/c",
                "com.example.a/a",
                "com.example.a/b",
            ]
        );
    }

    #[test]
    fn find_returns_item_by_key() {
        let mut reg = ToolRegistry::with_builtins();
        reg.set_plugin_items(vec![make_plugin_item("com.example.a", "x", 100)]);
        assert!(reg.find("builtin:clipboard_history").is_some());
        assert!(reg.find("com.example.a/x").is_some());
        assert!(reg.find("nope").is_none());
    }
}
