//! Full (expanded) sidebar wrapper — props 추출 + view 호출 + action → state
//! mutation 매핑. 시각 / 입력 로직은 [`crate::adapters::ui::sidebar::view`] 에서.

use crate::i18n::t;
use crate::intent::Intent;
use crate::state::AppState;
use crate::theme;

use super::view::{
    CategorySectionView, DragSnapshot, SidebarFullAction, SidebarFullProps, WorkspaceEntryView,
    draw_full_sidebar_view,
};

/// 워크스페이스 1개를 `WorkspaceEntryView` snapshot 으로 변환. collapsed 레일도 공유.
pub(super) fn entry_view(
    engine: &crate::core::CoreState,
    global_idx: usize,
    ws: &crate::model::Workspace,
    active_ws: usize,
) -> WorkspaceEntryView {
    let surface_ids = ws.all_surface_ids();
    WorkspaceEntryView {
        name: ws.name.clone(),
        subtitle: ws.subtitle.clone(),
        description: ws.description.clone(),
        busy_count: engine.busy_count(&surface_ids),
        has_highlight: engine.notifications.has_highlighted_surface(&surface_ids),
        attached: engine.attach.workspace_holder(ws.id).is_some(),
        is_mirror: ws.mirror,
        is_active: global_idx == active_ws,
    }
}

/// 카테고리 토글 on 이면 섹션 그룹 데이터를, off 면 `None` 을 만든다. full/collapsed
/// 사이드바가 공유한다. `categories()` 순서 = 표시 순서(normal 항상 맨 위). 각 섹션의
/// entries 는 `workspaces_in_category()` 가 주는 (전역 인덱스, &ws) 를 보존해 사이드바
/// action 의 전역 인덱스 계약을 지킨다.
pub(super) fn build_category_sections(
    engine: &crate::core::CoreState,
    active_ws: usize,
) -> Option<Vec<CategorySectionView>> {
    if !engine.settings.general.workspace_categories_enabled {
        return None;
    }
    let workspaces_heading = t("sidebar.workspaces_heading").to_string();
    Some(
        engine
            .categories()
            .iter()
            .map(|cat| {
                let entries = engine
                    .workspaces_in_category(cat.id)
                    .into_iter()
                    .map(|(gi, ws)| (gi, entry_view(engine, gi, ws, active_ws)))
                    .collect();
                CategorySectionView {
                    id: cat.id,
                    label: if cat.is_normal() {
                        workspaces_heading.clone()
                    } else {
                        cat.name.clone()
                    },
                    collapsed: cat.collapsed,
                    entries,
                }
            })
            .collect(),
    )
}

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
    plugin_alert: usize,
) -> FullSidebarResult {
    let th = theme::theme();
    let active_ws = state.active_workspace;
    let workspaces: Vec<WorkspaceEntryView> = engine
        .workspaces
        .iter()
        .enumerate()
        .map(|(i, ws)| entry_view(engine, i, ws, active_ws))
        .collect();

    let sections = build_category_sections(engine, active_ws);

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

    // switch-number overlay — 현재 눌린(사용자 입력) modifier 가 workspace_switch_modifier
    // 와 일치하면 워크스페이스 leading 을 숫자 키캡으로 그린다. P2a 탭과 동일하게 공통
    // 모듈 `switch_overlay` 의 판정을 재사용하고, egui ctx.input 의 modifier 만 보므로
    // 에이전트/IPC 로는 표시될 수 없다(사용자 입력 전용).
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
        // 디자인 chrome.jsx Sidebar: 패널 자체엔 좌우 패딩이 없다 — 목록 행 배경 /
        // 구분선이 사이드바 가장자리·우측 보더까지 꽉 차고, 좌우 padding 은 각 섹션
        // (헤더 12 / 헤딩 10 / 행 10 / 버튼 10) 이 스스로 갖는다. egui SidePanel 기본
        // 프레임은 inner_margin symmetric(8,2) 라 모든 내용물을 8px 안쪽으로 밀어
        // 배경/구분선이 가장자리에 닿지 못했다 → inner_margin 0 으로 덮어쓴다.
        // fill 은 기본 panel_fill 과 동일한 mantle(bg_sidebar) 로 유지.
        .frame(egui::Frame::new().fill(th.bg_sidebar().to_egui()))
        .show(ctx, |ui| {
            let props = SidebarFullProps {
                theme: &th,
                workspaces: &workspaces,
                categories: sections.as_deref(),
                drag,
                tools_label,
                collapse_label: &collapse_label,
                plugins_label,
                settings_label,
                new_workspace_label,
                workspaces_heading,
                occupied_hover,
                plugin_alert,
                workspace_switch_held,
            };
            deferred_actions = draw_full_sidebar_view(ui, &props);
        });

    // 우측 경계선 (ui_kit border-right) — 사이드바 안쪽 마지막 px.
    // panel_resp.response.rect.right() 는 exact_width 보다 크다(separator/resize handle
    // 영역 포함, 측정상 190 vs 180). 그대로 쓰면 터미널을 ~10px 침범하므로, 터미널
    // 영역 계산과 동일한 sidebar_width 를 기준으로 그린다.
    let panel_rect = panel_resp.response.rect;
    ctx.layer_painter(egui::LayerId::new(
        egui::Order::Middle,
        egui::Id::new("sidebar_right_border"),
    ))
    .vline(
        sidebar_width - 0.5,
        panel_rect.y_range(),
        egui::Stroke::new(1.0, th.border_default().to_egui()),
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
            SidebarFullAction::DragReleased {
                drop_target,
                target_category,
            } => {
                let from = state.dialogs.ws_drag.as_ref().map(|d| d.ws_idx);
                state.dialogs.ws_drag = None;
                if let Some(from) = from
                    && from < engine.workspaces.len()
                {
                    let src_cat = engine.workspaces[from].category;
                    match target_category {
                        // 다른 카테고리로 드롭 → 소속 이동(전역 인덱스·순서 보존).
                        Some(target_cat) if target_cat != src_cat => {
                            let ws_id = engine.workspaces[from].id;
                            if let Err(e) = engine.set_workspace_category(ws_id, target_cat) {
                                tracing::warn!("drag set_workspace_category failed: {e:?}");
                            }
                            engine.mark_layout_dirty();
                        }
                        // 같은 카테고리(또는 평면) → 순서 변경.
                        _ => {
                            if let Some(to) = drop_target
                                && to < ws_count
                            {
                                state.move_workspace(engine, from, to);
                            }
                        }
                    }
                }
            }
            SidebarFullAction::NewWorkspace => {
                state.dispatch_intent(
                    Intent::NewWorkspace {
                        kind: None,
                        params: serde_json::Value::Null,
                        category: None,
                    }
                    .from_user_menu("sidebar_add_workspace"),
                );
            }
            SidebarFullAction::NewWorkspaceContextMenu { x, y } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::NewWorkspaceButton { x, y });
            }
            SidebarFullAction::CategoryHeaderToggle(cat_id) => {
                // 헤더 클릭 → 접힘 토글 + 영속(mark_layout_dirty). 접힘은 layout.json
                // 대상이고 확장↔레일이 공유하는 per-category 상태(TODO 02 setter).
                engine.toggle_category_collapsed(cat_id);
                engine.mark_layout_dirty();
            }
            SidebarFullAction::CategoryHeaderContextMenu { cat_id, x, y } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::WorkspaceCategoryHeader { cat_id, x, y });
            }
            SidebarFullAction::BackgroundContextMenu { x, y } => {
                state.dialogs.pending_native_menu =
                    Some(crate::state::PendingNativeMenu::SidebarBackground { x, y });
            }
        }
    }

    result
}
