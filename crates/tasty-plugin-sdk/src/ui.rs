//! UiNode 빌더 헬퍼.
//!
//! 직접 enum variant를 만들 수도 있지만, 기본값을 쉽게 채우기 위해 자주 쓰이는
//! 모양을 함수로 노출한다.

use tasty_plugin_protocol::SharedBufferId;
use tasty_plugin_protocol::ui_tree::{
    ButtonStyle, LabelStyle, PixelFilter, PixelFormat, SelectionMode, SplitDir, TreeNode, UiNode,
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
        id: None,
    }
}

/// id 지정 splitter — 사용자가 divider를 드래그할 수 있으며, 그때마다
/// `UiEvent::SplitterDrag { node_id, ratio }`가 plugin에 전달된다.
pub fn splitter_id(
    id: impl Into<String>,
    direction: SplitDir,
    ratio: f32,
    first: UiNode,
    second: UiNode,
) -> UiNode {
    UiNode::Splitter {
        direction,
        ratio,
        first: Box::new(first),
        second: Box::new(second),
        id: Some(id.into()),
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

/// Monospace 본문 라벨 — diff/log/code 표시용.
pub fn label_mono(text: impl Into<String>) -> UiNode {
    UiNode::Label {
        text: text.into(),
        style: LabelStyle::Mono,
        color: None,
    }
}

/// Monospace 본문 + 색 — diff `+`/`-` 줄, status 컬럼 prefix 등.
pub fn label_mono_color(text: impl Into<String>, color: impl Into<String>) -> UiNode {
    UiNode::Label {
        text: text.into(),
        style: LabelStyle::Mono,
        color: Some(color.into()),
    }
}

/// 클릭 가능한 행. 자식 노드 그룹을 통째로 한 hit 영역으로 묶고, `selected = true`면
/// 호스트가 강조 배경을 깔아준다. 클릭 시 `UiEvent::Click { node_id: id }` 발화.
pub fn selectable_row(
    id: impl Into<String>,
    selected: bool,
    children: impl IntoIterator<Item = UiNode>,
) -> UiNode {
    UiNode::SelectableRow {
        id: id.into(),
        selected,
        children: children.into_iter().collect(),
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
        block: false,
        tooltip_i18n_key: None,
    }
}

pub fn button_primary(id: impl Into<String>, label_text: impl Into<String>) -> UiNode {
    UiNode::Button {
        id: id.into(),
        label: label_text.into(),
        enabled: true,
        style: ButtonStyle::Primary,
        block: false,
        tooltip_i18n_key: None,
    }
}

/// Full-width(`block`) secondary 버튼 — 목록형 컨테이너에서 폭을 채운다.
pub fn button_block(id: impl Into<String>, label_text: impl Into<String>) -> UiNode {
    UiNode::Button {
        id: id.into(),
        label: label_text.into(),
        enabled: true,
        style: ButtonStyle::default(),
        block: true,
        tooltip_i18n_key: None,
    }
}

/// Full-width(`block`) primary 버튼 — 선택된 목록 항목 강조용.
pub fn button_primary_block(id: impl Into<String>, label_text: impl Into<String>) -> UiNode {
    UiNode::Button {
        id: id.into(),
        label: label_text.into(),
        enabled: true,
        style: ButtonStyle::Primary,
        block: true,
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

/// Plugin Canvas — RGBA8 sRGB + Linear filter 기본. hit-test 비활성.
///
/// SharedBuffer 크기는 `width * height * 4 + tasty_shm::footer::SIZE`이어야 한다.
pub fn canvas(buffer_id: SharedBufferId, width: u32, height: u32) -> UiNode {
    UiNode::Canvas {
        buffer_id,
        width,
        height,
        format: PixelFormat::Rgba8,
        filter: PixelFilter::Linear,
        commit_seq: 0,
        id: None,
    }
}

/// hit-test 가능한 canvas — 마우스 입력이 [`tasty_plugin_protocol::UiEvent::CanvasPointer`]로 전달된다.
pub fn canvas_with_id(
    id: impl Into<String>,
    buffer_id: SharedBufferId,
    width: u32,
    height: u32,
) -> UiNode {
    UiNode::Canvas {
        buffer_id,
        width,
        height,
        format: PixelFormat::Rgba8,
        filter: PixelFilter::Linear,
        commit_seq: 0,
        id: Some(id.into()),
    }
}

/// 포맷/필터를 직접 지정하는 canvas. 일반적으로 [`canvas`]·[`canvas_with_id`]면 충분하다.
pub fn canvas_full(
    id: Option<String>,
    buffer_id: SharedBufferId,
    width: u32,
    height: u32,
    format: PixelFormat,
    filter: PixelFilter,
) -> UiNode {
    UiNode::Canvas {
        buffer_id,
        width,
        height,
        format,
        filter,
        commit_seq: 0,
        id,
    }
}
