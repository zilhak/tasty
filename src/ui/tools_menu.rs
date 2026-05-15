//! 도구 메뉴 팝업. 사이드바의 "도구" 버튼 위에 떠서 도구 목록을 보여준다.
//!
//! 항목 출처:
//! - 호스트 빌트인 (`ToolSource::Builtin`): 컴파일 시 박힌 항목 (예: Clipboard History).
//! - Plugin (`ToolSource::Plugin`): 활성 + `ui.tool_item` 권한 grant된 plugin이
//!   `[[contributes.tool]]`로 선언한 항목. `AppState::tool_registry`에 동기화돼 있다.
//!
//! 클릭 시 dispatch:
//! - 빌트인 Clipboard History → `open_clipboard_viewer_popup`.
//! - `ToolAction::Event` → `state.pending_tool_events`에 enqueue (App 메인 루프가
//!   PluginManager로 발화).
//! - `ToolAction::OpenSurface` → focused pane에 `add_kind_tab`.
//! - `ToolAction::OpenPopup` → phase2-popup 구현 전이므로 warn 후 무시.

use crate::i18n::t;
use crate::plugin::manifest::ToolAction;
use crate::plugin::tool_registry::{ToolItem, ToolSource};
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

pub fn draw_tools_menu(ui: &mut egui::Ui, state: &mut AppState) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let th = theme::theme();
    let width = ui.available_width();
    let items = state.tool_registry.visible_items();

    let mut clicked: Option<ToolItem> = None;
    for item in &items {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
        }
        let label = {
            let translated = t(&item.label_i18n_key);
            // t()는 키가 없으면 키 자체를 반환한다. plugin 작성자가 i18n catalog에
            // 키를 등록하지 않았으면 label_i18n_key 자체를 표시 (fallback).
            if translated == item.label_i18n_key {
                item.label_i18n_key.clone()
            } else {
                translated.to_string()
            }
        };
        ui.painter().text(
            egui::pos2(rect.min.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(th.font_size_body.value()),
            if resp.hovered() { th.text.into() } else { th.subtext0.into() },
        );
        if resp.clicked() {
            clicked = Some(item.clone());
        }
    }

    if let Some(item) = clicked {
        invoke_tool(state, &item);
        return PopupAction::Close;
    }
    PopupAction::None
}

/// 도구 항목을 실행한다. 호스트 빌트인은 직접 분기, plugin 항목은 action 종류별 처리.
///
/// 이 함수는 사용자 클릭에서만 호출된다 (포커스 의존 동작 — focused pane에 surface
/// 추가). IPC 경유 `debug.tool.invoke`는 별도 경로로 pane/tab id를 명시한다.
pub fn invoke_tool(state: &mut AppState, item: &ToolItem) {
    // 빌트인 분기: source가 Builtin이면 key로 분기.
    if matches!(item.source, ToolSource::Builtin) {
        match item.key.as_str() {
            "builtin:clipboard_history" => {
                crate::clipboard_viewer_ui::open_clipboard_viewer_popup(state);
            }
            other => {
                tracing::warn!("invoke_tool: unknown builtin tool key '{}'", other);
            }
        }
        return;
    }

    // Plugin 항목: action 종류별 처리.
    match &item.action {
        ToolAction::Event { event_key } => {
            // payload는 항목 key를 포함해 plugin이 어떤 항목 트리거인지 식별할 수 있게 함.
            let payload = serde_json::json!({ "tool_id": item.key });
            state
                .pending_tool_events
                .push((event_key.clone(), payload));
        }
        ToolAction::OpenSurface { surface_kind } => {
            if let Err(e) = state.add_kind_tab(surface_kind, &serde_json::Value::Null) {
                tracing::warn!(
                    "invoke_tool: open_surface kind='{}' failed: {}",
                    surface_kind,
                    e
                );
            }
        }
        ToolAction::OpenPopup { popup_id } => {
            // `<plugin_id>/<popup_id>` 형식 (manifest validation에서 강제). split하여
            // plugin_manager로 dispatch할 수 있도록 pending_popup_opens에 enqueue.
            // App 메인 루프가 drain해 `open_popup_instance`를 호출한다.
            if let Some((plugin_id, local_id)) = popup_id.split_once('/') {
                state.pending_popup_opens.push((
                    plugin_id.to_string(),
                    local_id.to_string(),
                    serde_json::Value::Null,
                ));
            } else {
                tracing::warn!(
                    "invoke_tool: open_popup '{}' is not in '<plugin_id>/<id>' form",
                    popup_id
                );
            }
        }
    }
}
