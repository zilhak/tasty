//! UiNode tree 빌더 — master-detail popup 본체.
//!
//! 좌측 = 가용 클립보드 타입 목록(Button), 우측 = 선택 타입 상세(TextPreview).
//! 타입 목록을 `Button` 으로 두는 것은 디자인 명세(요청 §1.5 "버튼 시각")를 따른 것이다:
//! 유휴 = secondary(외곽선), 선택 = primary(accent) 강조, 둘 다 full-width(`block`).
//! 호스트 `UiNode::Button` 렌더가 `tasty_ui_widgets::Button` 으로 토큰화돼 있다.

use tasty_plugin_sdk::Translator;
use tasty_plugin_sdk::ui::{
    button_block, button_primary_block, center, label_color, scroll_v, splitter, text_preview, vbox,
};
use tasty_plugin_sdk::{SplitDir, UiNode};

use crate::clipboard::{ClipboardType, ContentRepr};

/// 좌측 타입 버튼 id 접두사 (`type-{key}`).
pub const TYPE_PREFIX: &str = "type-";

pub struct ViewModel<'a> {
    pub available: &'a [(ClipboardType, ContentRepr)],
    pub read_error: Option<&'a str>,
    pub selected: Option<ClipboardType>,
}

/// 단일 인스턴스 가드 placeholder.
pub fn already_open_tree(tr: &Translator) -> UiNode {
    center(label_color(
        tr.t("clipboard_viewer.popup.already_open"),
        "subtext0",
    ))
}

pub fn main_tree(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    // C-G1(빈/실패 중앙정렬): center() 로 popup 본문 양축 중앙에 한 줄 배치한다.
    // 색은 의미 토큰(empty=text_muted≈subtext0 / fail=accent_danger≈red).
    // 클립보드 핸들 자체 실패 → read 실패 상태.
    if vm.read_error.is_some() {
        return center(label_color(
            tr.t("clipboard_viewer.popup.read_failed"),
            "red",
        ));
    }
    // 가용 타입 0개 → 빈 상태.
    if vm.available.is_empty() {
        return center(label_color(
            tr.t("clipboard_viewer.popup.empty"),
            "subtext0",
        ));
    }

    let left = build_type_list(vm, tr);
    let right = build_detail(vm);
    splitter(SplitDir::Horizontal, 0.3, left, right)
}

fn build_type_list(vm: &ViewModel<'_>, tr: &Translator) -> UiNode {
    let mut rows: Vec<UiNode> = Vec::with_capacity(vm.available.len());
    for (ty, _) in vm.available {
        let id = format!("{TYPE_PREFIX}{}", ty.key());
        let label = tr.t(ty.label_i18n_key()).to_string();
        if vm.selected == Some(*ty) {
            rows.push(button_primary_block(id, label));
        } else {
            rows.push(button_block(id, label));
        }
    }
    scroll_v(vbox(rows))
}

fn build_detail(vm: &ViewModel<'_>) -> UiNode {
    let selected = vm
        .selected
        .and_then(|ty| vm.available.iter().find(|(t, _)| *t == ty));
    match selected {
        Some((_, ContentRepr::Text(content))) => scroll_v(text_preview(content.clone())),
        None => scroll_v(vbox([])),
    }
}
