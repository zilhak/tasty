//! 사이드바 도구 메뉴. 본체 항목과 활성·권한 허용된 플러그인의 도구 항목을 표시한다.
//! 플러그인 이벤트·팝업 요청은 큐에 넣고 surface 열기는 사용자 포커스 pane을 대상으로 한다.

use crate::adapters::ui::popup::{self, PopupAction};
use crate::i18n::t;
use crate::intent::{OpenPopupMode, UiIntent};
use crate::plugin::manifest::ToolAction;
use crate::plugin::tool_registry::ToolItem;
use crate::state::AppState;
use crate::theme;
use egui::emath::GuiRounding as _;
use tasty_type_geometry::length::LogicalPx;
use tasty_ui_widgets::menu_separator;

/// Popup width (도구 항목 라벨이 모두 들어가는 baseline). 사이드바 도구 버튼 좌측 정렬.
const POPUP_WIDTH: LogicalPx = LogicalPx(160.0);
/// 도구 항목 한 줄 높이. draw 와 sizer 가 같은 값을 참조해야 잘림 방지.
const ITEM_HEIGHT: LogicalPx = LogicalPx(28.0);

/// Built-in tool entries that are not contributed by any plugin.
/// `action` 으로 popup / 별도 winit 윈도우 오픈을 구분한다.
struct BuiltinTool {
    label_key: &'static str,
    action: BuiltinAction,
}

enum BuiltinAction {
    /// 일반 popup 열기.
    OpenPopup(&'static str),
    /// 별도 winit 윈도우 열기. 현재 사용처는 PresetView 하나.
    OpenWindow(WindowKind),
    /// workspace 스코프 popup 열기. 스코프는 정의가 아니라 **여는 시점**의 활성
    /// workspace 로 정해지므로 `OpenPopup` 과 분기가 다르다 — 이 창은 그 workspace
    /// 를 벗어나면 숨고 돌아오면 다시 뜬다.
    OpenWorkspacePopup(&'static str),
    /// 파일 피커(docs/features/native-file-picker/index.md) — 단순 `OpenPopup` 과 달리 여는 *전* 활성 workspace 의
    /// mirror 여부로 로컬/원격을 판별해 `state.dialogs.file_picker` 를 채워야 하므로
    /// 별도 분기.
    OpenFilePicker,
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
        label_key: "remote_tool.tools_menu_item",
        action: BuiltinAction::OpenPopup(super::popup::remote_tool::REMOTE_TOOL_POPUP_ID),
    },
    BuiltinTool {
        label_key: "preset.tools.menu_item",
        action: BuiltinAction::OpenWindow(WindowKind::Preset),
    },
    BuiltinTool {
        label_key: "tutorial.tools_menu_item",
        action: BuiltinAction::OpenPopup(
            crate::adapters::ui::tutorial::topic_popup::TUTORIAL_TOPICS_POPUP_ID,
        ),
    },
    BuiltinTool {
        label_key: "dag_list.tools_menu_item",
        action: BuiltinAction::OpenWorkspacePopup(super::popup::dag_list::DAG_LIST_POPUP_ID),
    },
    BuiltinTool {
        label_key: "filepicker.tools_menu_item",
        action: BuiltinAction::OpenFilePicker,
    },
];

pub fn draw_tools_menu(
    ui: &mut egui::Ui,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
) -> PopupAction {
    if ui.ctx().input(|i| i.key_pressed(egui::Key::Escape)) {
        return PopupAction::Close;
    }

    let th = theme::theme();
    let width = ui.available_width();

    let mut open_popup: Option<&'static str> = None;
    let mut open_workspace_popup: Option<&'static str> = None;
    let mut open_window: Option<WindowKind> = None;
    let mut open_file_picker = false;
    for entry in BUILTIN_TOOLS {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, ITEM_HEIGHT.value()), egui::Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
        }
        ui.painter().text(
            egui::pos2(rect.min.x + th.spacing_sm.value(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            t(entry.label_key),
            egui::FontId::proportional(th.font_size_body.value()),
            if resp.hovered() {
                th.text_primary().into()
            } else {
                th.text_muted().into()
            },
        );
        if resp.clicked() {
            match entry.action {
                BuiltinAction::OpenPopup(id) => open_popup = Some(id),
                BuiltinAction::OpenWorkspacePopup(id) => open_workspace_popup = Some(id),
                BuiltinAction::OpenWindow(k) => open_window = Some(k),
                BuiltinAction::OpenFilePicker => open_file_picker = true,
            }
        }
    }
    if open_file_picker {
        let start = popup::file_picker::FilePickerStart::from_surface(
            engine,
            state.focused_surface_id(engine),
        );
        popup::file_picker::open(state, engine, None, Vec::new(), start);
        return PopupAction::Close;
    }
    if let Some(popup_id) = open_workspace_popup {
        state.dispatch_intent(
            UiIntent::OpenPopup {
                id: popup_id,
                mode: OpenPopupMode::WithScope(popup::PopupScope::Workspace(
                    state.active_workspace,
                )),
            }
            .from_user_menu("tools_menu"),
        );
        return PopupAction::Close;
    }
    if let Some(popup_id) = open_popup {
        // 본체 팝업은 가운데 열고 포커스를 준다.
        state.dispatch_intent(
            UiIntent::OpenPopup {
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
        menu_separator(ui, &th);
    }

    let mut clicked: Option<ToolItem> = None;
    for item in &items {
        let (rect, resp) =
            ui.allocate_exact_size(egui::vec2(width, ITEM_HEIGHT.value()), egui::Sense::click());
        if resp.hovered() {
            ui.painter()
                .rect_filled(rect, 4.0, th.hover_overlay.to_egui_premultiplied());
        }
        let label = crate::adapters::ui::label_or_raw_key(&item.label_i18n_key);
        ui.painter().text(
            egui::pos2(rect.min.x + th.spacing_sm.value(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            label,
            egui::FontId::proportional(th.font_size_body.value()),
            if resp.hovered() {
                th.text_primary().into()
            } else {
                th.text_muted().into()
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

/// 사용자 클릭의 플러그인 도구를 실행한다. debug.tool.invoke는 대상 ID를 받는 별도 경로다.
pub fn invoke_tool(state: &mut AppState, engine: &mut crate::core::CoreState, item: &ToolItem) {
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
            // plugin/id를 나누어 팝업 요청을 큐에 넣는다. 사용자 포커스 surface의
            // cwd·mirror 정보를 함께 전달하며 local_surface_id는 원격 조회 대상에 사용한다.
            if let Some((plugin_id, local_id)) = popup_id.split_once('/') {
                let origin = state.focused_surface_id(engine);
                let context = state.popup_surface_context(engine, origin);
                state
                    .pending_popup_opens
                    .push(crate::state::PendingPopupOpen {
                        plugin_id: plugin_id.to_string(),
                        popup_id: local_id.to_string(),
                        context,
                        target_surface: origin,
                    });
            } else {
                tracing::warn!(
                    "invoke_tool: open_popup '{}' is not in '<plugin_id>/<id>' form",
                    popup_id
                );
            }
        }
    }
}

/// 실제 렌더링과 같은 항목 간격. Theme에 이미 배율이 적용돼 있다.
fn effective_item_spacing(_engine: &crate::core::CoreState) -> f32 {
    theme::theme().spacing_xs.value().round_ui()
}

/// 본체·플러그인 항목과 구분선 여백을 포함한 메뉴 크기. 타이틀바는 없는 팝업이다.
fn tools_menu_size_for(builtin_count: usize, plugin_count: usize, item_spacing: f32) -> egui::Vec2 {
    let total = builtin_count + plugin_count;
    let total = total.max(1);
    let mut content_h = ITEM_HEIGHT.scaled(total as f32)
        + LogicalPx((total.saturating_sub(1)) as f32 * item_spacing);
    if builtin_count > 0 && plugin_count > 0 {
        content_h += LogicalPx(2.0 * item_spacing);
    }
    // round_ui 누적 오차 / 초기 cursor 미세 padding 흡수용 1 px 마진.
    let safety_margin = 1.0;
    egui::vec2(
        POPUP_WIDTH.value(),
        (popup::content_margin().scaled(2.0) + content_h + LogicalPx(safety_margin)).value(),
    )
}

/// PopupDef.sizer — 매 프레임 plugin tool registry 의 실제 항목 수로 height 재계산.
pub fn tools_menu_sizer(state: &AppState, engine: &crate::core::CoreState) -> egui::Vec2 {
    let plugin_count = state.tool_registry.visible_items().len();
    tools_menu_size_for(
        BUILTIN_TOOLS.len(),
        plugin_count,
        effective_item_spacing(engine),
    )
}

/// 등록 시에는 본체 항목으로 계산하고 렌더링 때 sizer로 갱신한다.
pub fn tools_menu_default_size() -> egui::Vec2 {
    tools_menu_size_for(BUILTIN_TOOLS.len(), 0, theme::theme().spacing_xs.value())
}

/// 메뉴 위치 계산에 사용할 현재 본체·플러그인 항목의 크기.
pub fn tools_menu_current_size(state: &AppState, engine: &crate::core::CoreState) -> egui::Vec2 {
    tools_menu_sizer(state, engine)
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn fits_builtin_only_medium_scale() {
        let size = tools_menu_size_for(4, 0, 4.0);
        let needed =
            popup::content_margin().scaled(2.0) + ITEM_HEIGHT.scaled(4.0) + LogicalPx(3.0 * 4.0);
        assert!(
            LogicalPx(size.y) >= needed,
            "size.y ({}) < needed ({}) for 4 builtin items",
            size.y,
            needed
        );
        assert_eq!(size.x, POPUP_WIDTH.value());
    }

    #[test]
    fn fits_builtin_plus_plugin_with_separator() {
        let size = tools_menu_size_for(4, 3, 4.0);
        let needed = popup::content_margin().scaled(2.0)
            + ITEM_HEIGHT.scaled(7.0)
            + LogicalPx(6.0 * 4.0)  // item_spacing between 7 items
            + LogicalPx(2.0 * 4.0); // menu_separator = 2·spacing_xs
        assert!(
            LogicalPx(size.y) >= needed,
            "size.y ({}) < needed ({}) for 4+3 items",
            size.y,
            needed
        );
    }

    #[test]
    fn fits_plugin_only_no_separator() {
        let size = tools_menu_size_for(0, 5, 4.0);
        let needed =
            popup::content_margin().scaled(2.0) + ITEM_HEIGHT.scaled(5.0) + LogicalPx(4.0 * 4.0);
        assert!(LogicalPx(size.y) >= needed);
    }

    #[test]
    fn empty_does_not_underflow() {
        let size = tools_menu_size_for(0, 0, 4.0);
        assert!(LogicalPx(size.y) >= popup::content_margin().scaled(2.0) + ITEM_HEIGHT);
    }

    #[test]
    fn scales_with_ui_scale_1_2() {
        let size = tools_menu_size_for(4, 0, 4.78);
        let needed =
            popup::content_margin().scaled(2.0) + ITEM_HEIGHT.scaled(4.0) + LogicalPx(3.0 * 4.78);
        assert!(LogicalPx(size.y) >= needed);
    }
}
