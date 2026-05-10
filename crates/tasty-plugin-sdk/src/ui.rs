//! UiNode 빌더 헬퍼.
//!
//! 직접 enum variant를 만들 수도 있지만, 기본값을 쉽게 채우기 위해 자주 쓰이는
//! 모양을 함수로 노출한다.

use tasty_plugin_protocol::ui_tree::{
    ButtonStyle, LabelStyle, SelectionMode, SplitDir, TreeNode, UiNode,
};

pub fn vbox(children: impl IntoIterator<Item = UiNode>) -> UiNode {
    UiNode::Vbox {
        spacing: 4,
        children: children.into_iter().collect(),
    }
}

pub fn vbox_spacing(spacing: u32, children: impl IntoIterator<Item = UiNode>) -> UiNode {
    UiNode::Vbox {
        spacing,
        children: children.into_iter().collect(),
    }
}

pub fn hbox(children: impl IntoIterator<Item = UiNode>) -> UiNode {
    UiNode::Hbox {
        spacing: 4,
        children: children.into_iter().collect(),
    }
}

pub fn hbox_spacing(spacing: u32, children: impl IntoIterator<Item = UiNode>) -> UiNode {
    UiNode::Hbox {
        spacing,
        children: children.into_iter().collect(),
    }
}

pub fn scroll_v(child: UiNode) -> UiNode {
    UiNode::Scroll {
        vertical: true,
        horizontal: false,
        child: Box::new(child),
    }
}

pub fn splitter(direction: SplitDir, ratio: f32, first: UiNode, second: UiNode) -> UiNode {
    UiNode::Splitter {
        direction,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
    }
}

pub fn label(text: impl Into<String>) -> UiNode {
    UiNode::Label {
        text: text.into(),
        style: LabelStyle::default(),
        color: None,
    }
}

pub fn label_styled(text: impl Into<String>, style: LabelStyle) -> UiNode {
    UiNode::Label {
        text: text.into(),
        style,
        color: None,
    }
}

pub fn label_color(text: impl Into<String>, color: impl Into<String>) -> UiNode {
    UiNode::Label {
        text: text.into(),
        style: LabelStyle::default(),
        color: Some(color.into()),
    }
}

pub fn icon(name: impl Into<String>) -> UiNode {
    UiNode::Icon { name: name.into() }
}

pub fn button(id: impl Into<String>, label_text: impl Into<String>) -> UiNode {
    UiNode::Button {
        id: id.into(),
        label: label_text.into(),
        enabled: true,
        style: ButtonStyle::default(),
        tooltip_i18n_key: None,
    }
}

pub fn button_primary(id: impl Into<String>, label_text: impl Into<String>) -> UiNode {
    UiNode::Button {
        id: id.into(),
        label: label_text.into(),
        enabled: true,
        style: ButtonStyle::Primary,
        tooltip_i18n_key: None,
    }
}

pub fn tree_view(
    id: impl Into<String>,
    nodes: Vec<TreeNode>,
    selection_mode: SelectionMode,
) -> UiNode {
    UiNode::Tree {
        id: id.into(),
        nodes,
        selection_mode,
    }
}

pub fn addressbar(id: impl Into<String>, text: impl Into<String>) -> UiNode {
    UiNode::Addressbar {
        id: id.into(),
        text: text.into(),
        placeholder_i18n_key: None,
    }
}

pub fn text_preview(content: impl Into<String>) -> UiNode {
    UiNode::TextPreview {
        content: content.into(),
        language: String::new(),
    }
}

pub fn text_preview_lang(content: impl Into<String>, language: impl Into<String>) -> UiNode {
    UiNode::TextPreview {
        content: content.into(),
        language: language.into(),
    }
}

pub fn spacer(size: u32) -> UiNode {
    UiNode::Spacer { size }
}
