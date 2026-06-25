//! Collapsed sidebar wrapper — props 추출 + view 호출 + action → result 매핑.
//! 시각 / 입력 로직은 [`crate::adapters::ui::sidebar::view`] 에서.

use crate::i18n::t;
use crate::state::AppState;
use crate::theme;

use super::view::{
    SidebarCollapsedAction, SidebarCollapsedProps, WorkspaceEntryView, draw_collapsed_sidebar_view,
};

pub struct CollapsedSidebarResult {
    pub expand_clicked: bool,
    pub plugins_clicked: bool,
    pub settings_clicked: bool,
    pub tools_rect: Option<egui::Rect>,
    pub switch_ws: Option<usize>,
    pub add_ws: bool,
}

pub fn draw_collapsed_sidebar(
    ctx: &egui::Context,
    state: &mut AppState,
    engine: &crate::core::CoreState,
    sidebar_width: f32,
    plugin_alert: usize,
) -> CollapsedSidebarResult {
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

    let tools_hover = t("sidebar.tools_button");

    let mut result = CollapsedSidebarResult {
        expand_clicked: false,
        plugins_clicked: false,
        settings_clicked: false,
        tools_rect: None,
        switch_ws: None,
        add_ws: false,
    };

    let mut deferred_actions: Vec<SidebarCollapsedAction> = Vec::new();

    // switch-number overlay — 사용자가 workspace_switch_modifier 를 누르고 있으면 rail 의
    // letter avatar 를 숫자 키캡으로 그린다 (full 사이드바·탭과 동일 공통 배선).
    let workspace_switch_held = {
        let mods = ctx.input(|i| i.modifiers);
        crate::adapters::ui::switch_overlay::workspace_switch_held(
            mods,
            &engine.settings.keybindings,
        )
    };

    let panel_resp = egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show_separator_line(false)
        .show(ctx, |ui| {
            let props = SidebarCollapsedProps {
                theme: &th,
                workspaces: &workspaces,
                tools_hover,
                plugin_alert,
                workspace_switch_held,
            };
            deferred_actions = draw_collapsed_sidebar_view(ui, &props);
        });

    // 우측 경계선 (ui_kit border-right) — sidebar_width 기준 (panel rect 는 separator
    // 영역 때문에 더 커서 터미널을 침범한다).
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

    for action in deferred_actions {
        match action {
            SidebarCollapsedAction::Expand => result.expand_clicked = true,
            SidebarCollapsedAction::Plugins => result.plugins_clicked = true,
            SidebarCollapsedAction::Settings => result.settings_clicked = true,
            SidebarCollapsedAction::ToolsClicked(rect) => result.tools_rect = Some(rect),
            SidebarCollapsedAction::WorkspaceClicked(i) => result.switch_ws = Some(i),
            SidebarCollapsedAction::NewWorkspace => result.add_ws = true,
            SidebarCollapsedAction::NewWorkspaceContextMenu { x, y } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::NewWorkspaceButton { x, y });
            }
        }
    }

    result
}
