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
    state: &AppState,
    engine: &crate::core::CoreState,
    sidebar_width: f32,
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

    egui::SidePanel::left("workspace_sidebar")
        .exact_width(sidebar_width)
        .resizable(false)
        .show(ctx, |ui| {
            let props = SidebarCollapsedProps {
                theme: &th,
                workspaces: &workspaces,
                tools_hover,
            };
            deferred_actions = draw_collapsed_sidebar_view(ui, &props);
        });

    for action in deferred_actions {
        match action {
            SidebarCollapsedAction::Expand => result.expand_clicked = true,
            SidebarCollapsedAction::Plugins => result.plugins_clicked = true,
            SidebarCollapsedAction::Settings => result.settings_clicked = true,
            SidebarCollapsedAction::ToolsClicked(rect) => result.tools_rect = Some(rect),
            SidebarCollapsedAction::WorkspaceClicked(i) => result.switch_ws = Some(i),
            SidebarCollapsedAction::NewWorkspace => result.add_ws = true,
        }
    }

    result
}
