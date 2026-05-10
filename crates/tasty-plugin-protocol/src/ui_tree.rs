//! Plugin이 호스트에 보내는 UI tree 표현 + 호스트가 plugin에 보내는 사용자 이벤트.
//!
//! plugin은 매 surface.create / surface.event 응답에 `tree: UiNode | null`을 포함.
//! null이면 호스트는 이전 트리를 그대로 사용 (변경 없음).
//!
//! 위젯 v1: vbox/hbox/scroll/splitter, label/icon, button/tree/addressbar/text_preview,
//! spacer. 더 풍부한 위젯은 호스트 버전 업과 함께 추가.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum UiNode {
    Vbox {
        #[serde(default)]
        spacing: u32,
        children: Vec<UiNode>,
    },
    Hbox {
        #[serde(default)]
        spacing: u32,
        children: Vec<UiNode>,
    },
    Scroll {
        #[serde(default = "default_true")]
        vertical: bool,
        #[serde(default)]
        horizontal: bool,
        child: Box<UiNode>,
    },
    Splitter {
        direction: SplitDir,
        ratio: f32,
        first: Box<UiNode>,
        second: Box<UiNode>,
    },

    Label {
        text: String,
        #[serde(default)]
        style: LabelStyle,
        /// `text|subtext0|subtext1|blue|green|red|yellow` 또는 `#aabbcc`.
        #[serde(default)]
        color: Option<String>,
    },
    Icon {
        name: String,
    },

    Button {
        id: String,
        label: String,
        #[serde(default = "default_true")]
        enabled: bool,
        #[serde(default)]
        style: ButtonStyle,
        #[serde(default)]
        tooltip_i18n_key: Option<String>,
    },
    Tree {
        id: String,
        nodes: Vec<TreeNode>,
        #[serde(default)]
        selection_mode: SelectionMode,
    },
    Addressbar {
        id: String,
        text: String,
        #[serde(default)]
        placeholder_i18n_key: Option<String>,
    },
    TextPreview {
        content: String,
        #[serde(default)]
        language: String,
    },
    Spacer {
        size: u32,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct TreeNode {
    pub id: String,
    pub label: String,
    #[serde(default)]
    pub icon: Option<String>,
    #[serde(default)]
    pub expanded: bool,
    #[serde(default)]
    pub selected: bool,
    #[serde(default)]
    pub children: Vec<TreeNode>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SplitDir {
    Horizontal,
    Vertical,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum LabelStyle {
    #[default]
    Body,
    Caption,
    Heading,
    Dim,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ButtonStyle {
    #[default]
    Secondary,
    Primary,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SelectionMode {
    #[default]
    Single,
    Multi,
}

fn default_true() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    Click {
        node_id: String,
    },
    Key {
        key: String,
        #[serde(default)]
        mods: Vec<String>,
    },
    TreeSelect {
        node_id: String,
        selected: Vec<String>,
    },
    TreeExpand {
        node_id: String,
        path: String,
        expanded: bool,
    },
    AddressbarChange {
        node_id: String,
        text: String,
    },
    AddressbarSubmit {
        node_id: String,
        text: String,
    },
    ContextMenu {
        node_id: String,
        path: String,
        x: f32,
        y: f32,
    },
    Scroll {
        node_id: String,
        delta_y: f32,
    },
    FocusChanged {
        focused: bool,
    },
    Resize {
        width: u32,
        height: u32,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn label_basic_round_trip() {
        let n = UiNode::Label {
            text: "hello".into(),
            style: LabelStyle::Heading,
            color: Some("blue".into()),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"type\":\"label\""));
        assert!(s.contains("\"style\":\"heading\""));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn vbox_with_children() {
        let n = UiNode::Vbox {
            spacing: 4,
            children: vec![
                UiNode::Label {
                    text: "A".into(),
                    style: LabelStyle::Body,
                    color: None,
                },
                UiNode::Spacer { size: 8 },
                UiNode::Button {
                    id: "btn".into(),
                    label: "Ok".into(),
                    enabled: true,
                    style: ButtonStyle::Primary,
                    tooltip_i18n_key: None,
                },
            ],
        };
        let s = serde_json::to_string(&n).unwrap();
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn splitter_round_trip() {
        let n = UiNode::Splitter {
            direction: SplitDir::Horizontal,
            ratio: 0.35,
            first: Box::new(UiNode::Label {
                text: "L".into(),
                style: LabelStyle::Body,
                color: None,
            }),
            second: Box::new(UiNode::Label {
                text: "R".into(),
                style: LabelStyle::Body,
                color: None,
            }),
        };
        let s = serde_json::to_string(&n).unwrap();
        assert!(s.contains("\"direction\":\"horizontal\""));
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn tree_round_trip() {
        let n = UiNode::Tree {
            id: "files".into(),
            nodes: vec![TreeNode {
                id: "root".into(),
                label: "/".into(),
                icon: Some("📁".into()),
                expanded: true,
                selected: false,
                children: vec![TreeNode {
                    id: "a".into(),
                    label: "a.rs".into(),
                    icon: Some("📄".into()),
                    expanded: false,
                    selected: true,
                    children: vec![],
                }],
            }],
            selection_mode: SelectionMode::Single,
        };
        let s = serde_json::to_string(&n).unwrap();
        let parsed: UiNode = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, n);
    }

    #[test]
    fn ui_event_click() {
        let ev = UiEvent::Click {
            node_id: "btn_open".into(),
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"click\""));
        let parsed: UiEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, ev);
    }

    #[test]
    fn ui_event_tree_select() {
        let ev = UiEvent::TreeSelect {
            node_id: "files".into(),
            selected: vec!["root/a".into()],
        };
        let s = serde_json::to_string(&ev).unwrap();
        assert!(s.contains("\"kind\":\"tree_select\""));
        let parsed: UiEvent = serde_json::from_str(&s).unwrap();
        assert_eq!(parsed, ev);
    }
}
