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
    /// 파일 피커(04) — 단순 `OpenPopup` 과 달리 여는 *전* 활성 workspace 의
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

    // Built-in entries first.
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
            egui::pos2(rect.min.x + 8.0, rect.center().y),
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
        popup::file_picker::open(state, engine, None, Vec::new());
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
        // 명령 팔레트/포트 스캐너 등은 모달 popup — center + focus 가 자연스러우므로
        // CenteredFocused 로 발화. 기존 코드는 raw `open` 만 호출해 중앙 정렬/포커스가
        // 빠져 있던 버그를 함께 해결한다.
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

/// 도구 항목을 실행한다. plugin 항목의 action 종류별 처리.
///
/// 이 함수는 사용자 클릭에서만 호출된다 (포커스 의존 동작 — focused pane에 surface
/// 추가). IPC 경유 `debug.tool.invoke`는 별도 경로로 pane/tab id를 명시한다.
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
                let mut context = serde_json::json!({ "cwd": cwd });
                // mirror workspace 판별(`docs/adr/0056-git-viewer-remote-attach-git-query-channel.md`
                // 참고) — cwd 와 마찬가지로 generic 하게 채운다(git-viewer 전용 분기
                // 아님). `inherit_cwd` 설정과 무관하게 항상 판정한다 — "원격 인지"는
                // 그 설정이 꺼져 있어도 필요한 정보다.
                // `local_surface_id` 는 popup 이 이 mirror surface 를 앵커로 원격
                // 조회(`git_viewer.query` IPC)를 트리거할 때 그대로 echo 한다.
                if let Some(sid) = state.focused_surface_id(engine)
                    && let Some((idx, _)) = engine.find_workspace_index_for_surface(sid)
                    && engine.workspaces[idx].mirror
                    && let Some(obj) = context.as_object_mut()
                {
                    obj.insert("mirror".to_string(), serde_json::json!(true));
                    obj.insert("local_surface_id".to_string(), serde_json::json!(sid));
                }
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

/// `tasty_egui_theme` 가 `style.spacing.item_spacing.y` 로 적용하는 값과 동일하게
/// 계산한다 (convert popup 과 동일 패턴). draw 시 `allocate_exact_size` 사이의
/// vertical gap 이 이 값이므로 sizer 도 같은 식을 써야 마지막 항목이 잘리지 않는다.
///
/// Theme 토큰 자체가 host UI zoom 곱셈을 이미 반영하므로 (Z-1/Z-2) 여기서
/// 별도 `ui_scale_factor()` 곱셈 없이 그대로 사용한다.
fn effective_item_spacing(_engine: &crate::core::CoreState) -> f32 {
    theme::theme().spacing_xs.value().round_ui()
}

/// 빌트인 + 플러그인 항목 개수에 맞춘 popup 크기 계산.
///
/// content height = N·ITEM_HEIGHT + (N−1)·item_spacing
///                  + (separator 가 들어가면 2·item_spacing)
/// popup height   = content_margin()·2 + content_h + safety_margin
///                  (headless 이므로 title_bar_height() 는 빠진다)
///
/// separator 는 `menu_separator` 위젯이며 세로로 `add_space(spacing_xs)` 를 상하로
/// 소비한다(hline 은 painter 직접 호출이라 레이아웃 공간을 잡지 않음). item_spacing
/// == spacing_xs 이므로 소비량은 `2·item_spacing` 이고, spacing_xs 가 zoom 곱을
/// 이미 반영하므로 zoom 변화에도 자동 정합된다.
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

/// PopupDef.default_size — register 시점 placeholder. registry 가 비어있을 수 있으므로
/// BUILTIN_TOOLS 만 가정. sizer 가 매 프레임 재계산하므로 실제 렌더링에는 영향 없음.
pub fn tools_menu_default_size() -> egui::Vec2 {
    tools_menu_size_for(BUILTIN_TOOLS.len(), 0, theme::theme().spacing_xs.value())
}

/// 사이드바 도구 버튼이 popup 을 띄울 때 위치 계산용으로 호출한다.
/// `default_size` 대신 *현재 등록된 plugin 도구 수* 까지 반영한 정확한 크기를 받아야
/// popup top 이 버튼 위쪽 정확한 위치에 align 된다.
pub fn tools_menu_current_size(state: &AppState, engine: &crate::core::CoreState) -> egui::Vec2 {
    tools_menu_sizer(state, engine)
}

#[cfg(test)]
mod size_tests {
    use super::*;

    #[test]
    fn fits_builtin_only_medium_scale() {
        // ui_scale=1.0 → spacing_xs(4.0). BUILTIN 4 개, plugin 0 개, separator 없음.
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
        // BUILTIN 4 + plugin 3 → separator 1 개 추가됨.
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
        // BUILTIN 0 + plugin 5 (hypothetical) → separator 없음.
        let size = tools_menu_size_for(0, 5, 4.0);
        let needed =
            popup::content_margin().scaled(2.0) + ITEM_HEIGHT.scaled(5.0) + LogicalPx(4.0 * 4.0);
        assert!(LogicalPx(size.y) >= needed);
    }

    #[test]
    fn empty_does_not_underflow() {
        let size = tools_menu_size_for(0, 0, 4.0);
        // total.max(1) 이 적용되어 최소 한 줄 분량은 확보된다.
        assert!(LogicalPx(size.y) >= popup::content_margin().scaled(2.0) + ITEM_HEIGHT);
    }

    #[test]
    fn scales_with_ui_scale_1_2() {
        // ui_scale=1.2 → spacing ≈ 4.78. 4 항목 기준 spacing 누적이 늘어나도 fit.
        let size = tools_menu_size_for(4, 0, 4.78);
        let needed =
            popup::content_margin().scaled(2.0) + ITEM_HEIGHT.scaled(4.0) + LogicalPx(3.0 * 4.78);
        assert!(LogicalPx(size.y) >= needed);
    }
}
