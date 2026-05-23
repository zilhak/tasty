//! 도구 메뉴 팝업. 사이드바의 "도구" 버튼 위에 떠서 도구 목록을 보여준다.
//!
//! 항목은 모두 plugin 출처 — 활성 + `ui.tool_item` 권한 grant된 plugin이
//! `[[contributes.tool]]`로 선언한 항목. `AppState::tool_registry`에 동기화돼 있다.
//! Clipboard History 등 과거 호스트 빌트인 항목은 builtin plugin으로 이전됨.
//!
//! 클릭 시 dispatch:
//! - `ToolAction::Event` → `state.pending_tool_events`에 enqueue (App 메인 루프가
//!   PluginManager로 발화).
//! - `ToolAction::OpenSurface` → focused pane에 `add_kind_tab`.
//! - `ToolAction::OpenPopup` → pending_popup_opens enqueue.

use crate::i18n::t;
use crate::intent::{Intent, OpenPopupMode};
use crate::plugin::manifest::ToolAction;
use crate::plugin::tool_registry::ToolItem;
use crate::state::AppState;
use crate::theme;
use crate::ui::popup::PopupAction;

/// Built-in tool entries that are not contributed by any plugin.
/// `action` 으로 popup / 별도 winit 윈도우 오픈을 구분한다.
struct BuiltinTool {
    label_key: &'static str,
    action: BuiltinAction,
}

enum BuiltinAction {
    /// 일반 popup 열기.
    OpenPopup(&'static str),
    /// 별도 winit 윈도우 열기. 현재 사용처는 PresetWindow 하나.
    OpenWindow(WindowKind),
}

#[derive(Debug, Clone, Copy)]
enum WindowKind {
    Preset,
}

const BUILTIN_TOOLS: &[BuiltinTool] = &[
    BuiltinTool {
        label_key: "command_palette.tools_menu_item",
        action: BuiltinAction::OpenPopup(super::popup::command_palette::COMMAND_PALETTE_POPUP_ID),
    },
    BuiltinTool {
        label_key: "port_scanner.tools_menu_item",
        action: BuiltinAction::OpenPopup(super::popup::port_scanner::PORT_SCANNER_POPUP_ID),
    },
    BuiltinTool {
        label_key: "update.tools_menu_item",
        action: BuiltinAction::OpenPopup(super::popup::update::UPDATE_POPUP_ID),
    },
    BuiltinTool {
        label_key: "preset.tools.menu_item",
        action: BuiltinAction::OpenWindow(WindowKind::Preset),
    },
];

pub fn draw_tools_menu(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let th = theme::theme();
    let width = ui.available_width();

    // Built-in entries first.
    let mut open_popup: Option<&'static str> = None;
    let mut open_window: Option<WindowKind> = None;
    for entry in BUILTIN_TOOLS {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
        }
        ui.painter().text(
            egui::pos2(rect.min.x + 8.0, rect.center().y),
            egui::Align2::LEFT_CENTER,
            t(entry.label_key),
            egui::FontId::proportional(th.font_size_body.value()),
            if resp.hovered() {
                th.text.into()
            } else {
                th.subtext0.into()
            },
        );
        if resp.clicked() {
            match entry.action {
                BuiltinAction::OpenPopup(id) => open_popup = Some(id),
                BuiltinAction::OpenWindow(k) => open_window = Some(k),
            }
        }
    }
    if let Some(popup_id) = open_popup {
        // 명령 팔레트/포트 스캐너 등은 모달 popup — center + focus 가 자연스러우므로
        // CenteredFocused 로 발화. 기존 코드는 raw `open` 만 호출해 중앙 정렬/포커스가
        // 빠져 있던 버그를 함께 해결한다.
        state.dispatch_intent(
            Intent::OpenPopup {
                id: popup_id,
                mode: OpenPopupMode::CenteredFocused,
            }
            .from_user_menu("tools_menu"),
        );
        return PopupAction::Close;
    }
    if let Some(kind) = open_window {
        match kind {
            WindowKind::Preset => state.dialogs.pending_open_preset_window = true,
        }
        return PopupAction::Close;
    }

    let items = state.tool_registry.visible_items();
    if !items.is_empty() && !BUILTIN_TOOLS.is_empty() {
        ui.separator();
    }

    let mut clicked: Option<ToolItem> = None;
    for item in &items {
        let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, 28.0), egui::Sense::click());
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
            if resp.hovered() {
                th.text.into()
            } else {
                th.subtext0.into()
            },
        );
        if resp.clicked() {
            clicked = Some(item.clone());
        }
    }

    if let Some(item) = clicked {
        invoke_tool(state, engine, &item);
        return PopupAction::Close;
    }
    PopupAction::None
}

/// 도구 항목을 실행한다. plugin 항목의 action 종류별 처리.
///
/// 이 함수는 사용자 클릭에서만 호출된다 (포커스 의존 동작 — focused pane에 surface
/// 추가). IPC 경유 `debug.tool.invoke`는 별도 경로로 pane/tab id를 명시한다.
pub fn invoke_tool(
    state: &mut AppState,
    engine: &mut crate::engine_state::EngineState,
    item: &ToolItem,
) {
    match &item.action {
        ToolAction::Event { event_key } => {
            let payload = serde_json::json!({ "tool_id": item.key });
            state.pending_tool_events.push((event_key.clone(), payload));
        }
        ToolAction::OpenSurface { surface_kind } => {
            state.dispatch_intent(
                crate::intent::Intent::NewTab {
                    kind: Some(surface_kind.clone()),
                    params: serde_json::Value::Null,
                }
                .from_user_menu("tools_menu/open_surface"),
            );
        }
        ToolAction::OpenPopup { popup_id } => {
            // `<plugin_id>/<popup_id>` 형식. split하여 plugin_manager로 dispatch할
            // 수 있도록 pending_popup_opens에 enqueue. App 메인 루프가 drain.
            //
            // 사용자 메뉴 클릭은 활성 surface 컨텍스트에 매여 있으므로 context payload에
            // 활성 surface의 상속 cwd를 실어 plugin이 popup.open 단계에서 사용할 수 있게
            // 한다. cwd 미상이면 `null`.
            if let Some((plugin_id, local_id)) = popup_id.split_once('/') {
                let cwd = state
                    .resolve_inherit_cwd(engine)
                    .map(|p| p.to_string_lossy().into_owned());
                let context = serde_json::json!({ "cwd": cwd });
                state.pending_popup_opens.push((
                    plugin_id.to_string(),
                    local_id.to_string(),
                    context,
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
