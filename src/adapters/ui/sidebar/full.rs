//! Full (expanded) sidebar wrapper — props 추출 + view 호출 + action → state
//! mutation 매핑. 시각 / 입력 로직은 [`crate::adapters::ui::sidebar::view`] 에서.

use crate::i18n::t;
use crate::intent::Intent;
use crate::state::AppState;
use crate::theme;

use super::view::{
    DragSnapshot, SidebarFullAction, SidebarFullProps, WorkspaceEntryView, draw_full_sidebar_view,
};

pub struct FullSidebarResult {
    pub collapse_clicked: bool,
    pub plugins_clicked: bool,
    pub settings_clicked: bool,
    pub tools_rect: Option<egui::Rect>,
}

pub fn draw_full_sidebar(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &mut crate::core::CoreState,
    sidebar_width: f32,
) -> FullSidebarResult {
    let th = theme::theme();
    let active_ws = state.active_workspace;
    let workspaces: Vec<WorkspaceEntryView> = engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| {
            let surface_ids = ws.all_surface_ids();
            WorkspaceEntryView {
                name: ws.name.clone(),
                subtitle: ws.subtitle.clone(),
                description: ws.description.clone(),
                busy_count: engine.busy_count(&surface_ids),
                has_highlight: engine.notifications.has_highlighted_surface(&surface_ids),
                attached: engine.attach.workspace_holder(ws.id).is_some(),
                is_mirror: ws.mirror,
                is_active: i == active_ws,
            }
        })
        .collect();

    let drag = state.dialogs.ws_drag.as_ref().map(|d| DragSnapshot {
        ws_idx: d.ws_idx,
        current_y: d.current_y,
    });

    let tools_label = t("sidebar.tools_button");
    let collapse_label = t("sidebar.collapse_button").to_string();
    let plugins_label = t("button.plugins");
    let settings_label = t("button.settings");
    let new_workspace_label = t("button.new_workspace");
    let workspaces_heading = t("sidebar.workspaces_heading");
    let occupied_hover = t("attach.occupied_workspace");

    let mut result = FullSidebarResult {
        collapse_clicked: false,
        plugins_clicked: false,
        settings_clicked: false,
        tools_rect: None,
    };

    let mut deferred_actions: Vec<SidebarFullAction> = Vec::new();

    let panel_resp = egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let props = SidebarFullProps {
                theme: &th,
                workspaces: &workspaces,
                drag,
                tools_label,
                collapse_label: &collapse_label,
                plugins_label,
                settings_label,
                new_workspace_label,
                workspaces_heading,
                occupied_hover,
            };
            deferred_actions = draw_full_sidebar_view(ui, &props);
        });

    // 우측 경계선 (ui_kit border-right) — 사이드바 안쪽 마지막 px.
    // panel_resp.response.rect.right() 는 exact_width 보다 크다(separator/resize handle
    // 영역 포함, 측정상 190 vs 180). 그대로 쓰면 터미널을 ~10px 침범하므로, 터미널
    // 영역 계산과 동일한 sidebar_width 를 기준으로 그린다.
    let panel_rect = panel_resp.response.rect;
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        egui::Id::new("sidebar_right_border"),
    ))
    .vline(
        sidebar_width - 0.5,
        panel_rect.y_range(),
        egui::Stroke::new(1.0, th.surface0.to_egui()),
    );

    let ws_count = engine.workspaces.len();

    for action in deferred_actions {
        match action {
            SidebarFullAction::Collapse => result.collapse_clicked = true,
            SidebarFullAction::Plugins => result.plugins_clicked = true,
            SidebarFullAction::Settings => result.settings_clicked = true,
            SidebarFullAction::ToolsClicked(rect) => result.tools_rect = Some(rect),
            SidebarFullAction::WorkspaceClicked(i) => {
                state.switch_workspace(engine, i);
            }
            SidebarFullAction::WorkspaceContextMenu { ws_idx, x, y } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::Workspace { ws_idx, x, y });
            }
            SidebarFullAction::DragStart { ws_idx, y } => {
                state.dialogs.ws_drag = Some(crate::state::WsDragState {
                    ws_idx,
                    current_y: y,
                });
            }
            SidebarFullAction::DragUpdate { y } => {
                if let Some(drag) = state.dialogs.ws_drag.as_mut() {
                    drag.current_y = y;
                }
            }
            SidebarFullAction::DragReleased { drop_target } => {
                let from = state.dialogs.ws_drag.as_ref().map(|d| d.ws_idx);
                state.dialogs.ws_drag = None;
                if let (Some(from), Some(to)) = (from, drop_target)
                    && to < ws_count
                {
                    state.move_workspace(engine, from, to);
                }
            }
            SidebarFullAction::NewWorkspace => {
                state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
                    }
                    .from_user_menu("sidebar_add_workspace"),
                );
            }
            SidebarFullAction::NewWorkspaceContextMenu { x, y } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::NewWorkspaceButton { x, y });
            }
        }
    }

    result
}
